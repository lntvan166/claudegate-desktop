//! The log filter as git argv. There is no index and no cache: every filter is a git flag
//! or a pathspec, and git does the filtering.

use super::parse::LOG_FORMAT;

/// What the user filtered by. Times are epoch seconds; the caller fixes "now" at the first
/// page and derives `since` from it, so paging never drifts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Literal, case-insensitive text in the commit message.
    pub search: Option<String>,
    /// Author chips, matched literally against "Name <email>"; any of them matches.
    pub authors: Vec<String>,
    /// "Me": one more author, each repository's own `user.email`.
    pub mine: bool,
    /// A file, a folder or a glob, relative to each repository's root.
    pub path: Option<String>,
    /// A branch to show in every repository that has it.
    pub branch: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
}

/// Branch mode: how many repositories have the branch, and how many fell back to their
/// current HEADs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchUse {
    pub branch: String,
    pub found: usize,
    pub fallback: usize,
}

impl BranchUse {
    /// Count, for one query, which repositories have `branch` (`true`) and which do not.
    pub fn tally(branch: &str, has: impl IntoIterator<Item = bool>) -> BranchUse {
        let mut u = BranchUse { branch: branch.to_string(), found: 0, fallback: 0 };
        for h in has {
            if h {
                u.found += 1;
            } else {
                u.fallback += 1;
            }
        }
        u
    }
}

/// ISO-8601 UTC, which git reads exactly. git does not read `@<unix>` in `--since`/`--until`
/// consistently across versions, so the epoch seconds are written out as a date.
pub fn git_date(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// git's ref-name rules (`git check-ref-format`): any real branch is accepted, options and
/// revision syntax are not. The ref is also passed after `--end-of-options` and before `--`.
pub fn is_valid_ref(r: &str) -> bool {
    if r.is_empty() || r == "@" || r.starts_with('-') || r.starts_with('/') || r.ends_with('/') || r.ends_with('.') {
        return false;
    }
    if r.chars().any(|c| (c as u32) <= 0x20 || c == '\x7f' || "~^:?*[\\".contains(c)) {
        return false;
    }
    if r.contains("..") || r.contains("@{") || r.contains("//") {
        return false;
    }
    r.split('/').all(|p| !p.is_empty() && !p.starts_with('.') && !p.ends_with(".lock"))
}

/// The refs to log in one repository. Without a branch filter: its current HEADs (the main
/// checkout and its worktrees). With one: `[branch]` when the repository has it, else the
/// current HEADs. An invalid branch name counts as no filter.
pub fn heads_for(branch: Option<&str>, has_branch: bool, current: &[String]) -> Vec<String> {
    match branch {
        Some(b) if has_branch && is_valid_ref(b) => vec![b.to_string()],
        _ => current.to_vec(),
    }
}

/// A path filter as typed, made safe for a pathspec: trimmed, forward slashes, no leading
/// `./` or trailing `/`. `None` when it could leave the repository (absolute, `..`), switch
/// on other pathspec magic (a leading `:`), or holds control characters.
pub fn normalize_path(raw: &str) -> Option<String> {
    if raw.chars().any(|c| (c as u32) < 0x20) {
        return None;
    }
    let mut p = raw.trim().replace('\\', "/");
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    let p = p.trim_end_matches('/');
    let b = p.as_bytes();
    if p.is_empty() || p.starts_with('/') || p.starts_with(':') || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':') {
        return None;
    }
    if p.split('/').any(|seg| seg == "..") {
        return None;
    }
    Some(p.to_string())
}

/// The pathspec for a normalized path: literal, or git's glob magic when it has a glob
/// character (`*`, `?`, `[`). A glob without a folder (`*.sql`) matches at any depth, as plain
/// `git log -- '*.sql'` and .gitignore do; under `:(glob)` alone it would match only at the root.
pub fn pathspec_of(path: &str) -> String {
    if !path.contains(['*', '?', '[']) {
        format!(":(literal){path}")
    } else if path.contains('/') {
        format!(":(glob){path}")
    } else {
        format!(":(glob)**/{path}")
    }
}

/// Control characters become spaces: under `-F` a newline splits the pattern into "any of"
/// several, and a NUL cannot pass through argv at all.
fn clean_text(s: &str) -> String {
    s.chars().map(|c| if (c as u32) < 0x20 { ' ' } else { c }).collect()
}

/// Every author pattern for one repository: the chips, and Me (that repository's
/// `user.email`, if it has one). Trimmed, blank-free, de-duplicated ignoring case.
pub fn author_patterns(f: &Filter, me: Option<&str>) -> Vec<String> {
    let mine = if f.mine { me } else { None };
    let mut seen = std::collections::HashSet::new();
    f.authors
        .iter()
        .map(String::as_str)
        .chain(mine)
        .map(|a| clean_text(a).trim().to_string())
        .filter(|a| !a.is_empty() && seen.insert(a.to_lowercase()))
        .collect()
}

/// Whether one repository is searched at all. Me alone needs that repository's
/// `user.email`: without it, nothing there is "mine". With other authors picked too, the
/// repository is still searched for them. A chip that is blank once `author_patterns` cleans
/// it (control characters only) is no author.
pub fn repo_searched(f: &Filter, me: Option<&str>) -> bool {
    !f.mine || f.authors.iter().any(|a| !clean_text(a).trim().is_empty()) || me.is_some_and(|m| !clean_text(m).trim().is_empty())
}

/// `git log` argv for one page of one repository: `page` records starting `skip` records into
/// git's walk. `me` is that repository's `user.email`.
pub fn log_args(f: &Filter, heads: &[String], me: Option<&str>, page: usize, skip: usize) -> Vec<String> {
    let mut a = vec!["log".to_string(), LOG_FORMAT.to_string(), format!("--max-count={page}")];
    let search = f.search.as_deref().map(|t| clean_text(t).trim().to_string()).filter(|t| !t.is_empty());
    if let Some(t) = &search {
        a.push(format!("--grep={t}"));
    }
    let authors = author_patterns(f, me);
    for au in &authors {
        a.push(format!("--author={au}"));
    }
    // Literal and case-insensitive for --grep and --author alike; git ANDs --author with
    // --grep and ORs several --author flags.
    if search.is_some() || !authors.is_empty() {
        a.push("-i".into());
        a.push("-F".into());
    }
    if let Some(s) = f.since {
        a.push(format!("--since={}", git_date(s)));
    }
    if let Some(u) = f.until {
        a.push(format!("--until={}", git_date(u)));
    }
    if skip > 0 {
        a.push(format!("--skip={skip}"));
    }
    // After --end-of-options: revisions, never options. After --: paths, never revisions,
    // so a branch named like a folder is not ambiguous.
    if !heads.is_empty() {
        a.push("--end-of-options".into());
        a.extend(heads.iter().cloned());
    }
    a.push("--".into());
    if let Some(p) = f.path.as_deref().and_then(normalize_path) {
        a.push(pathspec_of(&p));
    }
    a
}
