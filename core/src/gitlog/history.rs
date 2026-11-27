//! One file's history, followed across renames (Polylog's File History).
//!
//! `--follow` cannot page with `--skip`, and it stops following when a filter drops the rename
//! commit. So the first page runs one unfiltered, unpaged `git log --follow` that only learns
//! every name the file has had (`history_paths_args`, `parse_history_paths`), and every page then
//! logs all of those names as literal pathspecs, without `--follow`, paged with `--skip` like the
//! log (`history_args`, `parse_history`).
//!
//! Deliberately unlike Polylog: a commit that lists the file's names only for *another* file (a
//! new, unrelated file that reuses an old name after the rename) is counted for `--skip` but
//! shows no row, so it neither joins this file's history nor moves the name being followed.
//!
//! Both parsers read NUL-terminated tokens by position: a path can hold a newline, an RS byte or
//! a string that looks like a status letter, and a subject can hold RS too.

use super::parse::{is_sha, Commit, FileStatus};

/// RS *starts* each record, because `--name-status -z` output follows the format:
/// `RS <sha> NUL <ct> NUL <an> NUL <ae> NUL <s> NUL <parents> NUL ["\n" <STATUS> NUL <path> NUL [<new path> NUL]]…`.
pub const HISTORY_FORMAT: &str = "--format=%x1e%H%x00%ct%x00%an%x00%ae%x00%s%x00%P";

/// The fields of one `HISTORY_FORMAT` header.
const HISTORY_FIELDS: usize = 6;

/// The unpaged `--follow` walk that only collects the names a file has had. `heads` are the
/// revisions to walk (after `--end-of-options`); none means HEAD.
pub fn history_paths_args(path: &str, heads: &[String]) -> Vec<String> {
    let mut a: Vec<String> = ["log", "--follow", "--no-show-signature", "--root", "--name-status", "-z", "-M", "--format=%x1e"].map(String::from).to_vec();
    if !heads.is_empty() {
        a.push("--end-of-options".into());
        a.extend(heads.iter().cloned());
    }
    a.push("--".into());
    a.push(format!(":(literal){path}"));
    a
}

/// A `--name-status` status token: leading newlines, then one of `AMDRCT` and an optional score.
fn status_letter(tok: &str) -> Option<char> {
    let t = tok.trim_start_matches('\n');
    let mut c = t.chars();
    let letter = c.next().filter(|l| "AMDRCT".contains(*l))?;
    c.all(|d| d.is_ascii_digit()).then_some(letter)
}

/// A header token: an RS byte, after git's newline between records.
fn is_header(tok: &str) -> bool {
    tok.trim_start_matches('\n').starts_with('\x1e')
}

/// The file's names, newest first; always starts with `path`, its current name. Read
/// positionally: each status token is followed by its one or two path tokens, so a path is never
/// taken for a status or a record start.
pub fn parse_history_paths(out: &str, path: &str) -> Vec<String> {
    let mut names = vec![path.to_string()];
    let mut add = |p: &str| {
        if !p.is_empty() && !names.iter().any(|n| n == p) {
            names.push(p.to_string());
        }
    };
    let mut toks: Vec<&str> = out.split('\0').collect();
    // What follows the last NUL is git's trailing newline or a token cut off by a kill.
    toks.pop();
    let mut i = 0;
    while i < toks.len() {
        let Some(letter) = status_letter(toks[i]) else {
            i += 1;
            continue;
        };
        let n = if matches!(letter, 'R' | 'C') { 2 } else { 1 };
        for p in toks.iter().skip(i + 1).take(n) {
            add(p);
        }
        i += 1 + n;
    }
    names
}

/// `git log` argv for one page of one file's history: `page` records starting `skip` records
/// into git's walk, over every name in `paths`. No `Filter`: search, authors, dates and the path
/// filter do not apply to a file's history. No `--follow` (`--no-follow` also overrides a user's
/// `log.follow`): it cannot page with `--skip`.
pub fn history_args(heads: &[String], page: usize, skip: usize, paths: &[String]) -> Vec<String> {
    let mut a = vec!["log".to_string(), HISTORY_FORMAT.to_string(), format!("--max-count={page}")];
    if skip > 0 {
        a.push(format!("--skip={skip}"));
    }
    // `--root` overrides a user's `log.showRoot=false`, which would drop the root commit's status.
    for f in ["--no-follow", "--no-show-signature", "--root", "--name-status", "-z", "-M"] {
        a.push(f.into());
    }
    if !heads.is_empty() {
        a.push("--end-of-options".into());
        a.extend(heads.iter().cloned());
    }
    a.push("--".into());
    // Literal: a route folder like `app/[id]/` is a name, not a glob.
    a.extend(paths.iter().map(|p| format!(":(literal){p}")));
    a
}

/// The file as one commit of its history saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryFile {
    /// Its name after the commit.
    pub path: String,
    /// The name it was renamed or copied from.
    pub from: Option<String>,
    /// `None` when git listed no change for it (a merge).
    pub status: Option<FileStatus>,
}

fn file_of(letter: char, paths: &[&str]) -> HistoryFile {
    let two = matches!(letter, 'R' | 'C');
    let path = paths[if two { 1 } else { 0 }].to_string();
    let from = two.then(|| paths[0].to_string());
    let status = match letter {
        'A' => FileStatus::Added,
        'M' => FileStatus::Modified,
        'D' => FileStatus::Deleted,
        'T' => FileStatus::TypeChanged,
        'R' => FileStatus::Renamed { from: paths[0].to_string() },
        _ => FileStatus::Copied { from: paths[0].to_string() },
    };
    HistoryFile { path, from, status: Some(status) }
}

/// One file's commits from `history_args` output, each with the file as that commit saw it, and
/// the number of records git emitted (every header whose six fields were read, parseable or not),
/// which advances `--skip`.
///
/// Positional on NUL tokens: a record starts at a token beginning with RS, and the next five
/// tokens are its fields, taken verbatim. Then come zero or more status tokens, each with its one
/// or two path tokens. The entry naming the file as the previous row left it (its `from`, else its
/// `path`; `current` before the first row) is this file's. A record that lists entries but none
/// for that name is another file reusing an old name: it is counted but gives no row (Polylog
/// took its first entry). A record with no status (a merge) keeps that name. A record cut off before its
/// sixth field is not counted, and one cut off inside its status is dropped.
pub fn parse_history(out: &str, current: &str) -> (Vec<(Commit, HistoryFile)>, usize) {
    let mut toks: Vec<&str> = out.split('\0').collect();
    // What follows the last NUL is git's trailing newline or a token cut off by a kill.
    toks.pop();
    let mut rows: Vec<(Commit, HistoryFile)> = Vec::new();
    let mut count = 0;
    let mut i = 0;
    while i < toks.len() {
        if !is_header(toks[i]) {
            i += 1;
            continue;
        }
        if i + HISTORY_FIELDS > toks.len() {
            break; // cut off mid-fields
        }
        let f = &toks[i..i + HISTORY_FIELDS];
        count += 1;
        i += HISTORY_FIELDS;
        let expect = rows.last().map(|(_, h)| h.from.clone().unwrap_or_else(|| h.path.clone())).unwrap_or_else(|| current.to_string());
        let mut entries: Vec<HistoryFile> = Vec::new();
        let mut truncated = false;
        while i < toks.len() && !is_header(toks[i]) {
            let Some(letter) = status_letter(toks[i]) else {
                i += 1;
                continue;
            };
            let n = if matches!(letter, 'R' | 'C') { 2 } else { 1 };
            if i + 1 + n > toks.len() || toks[i + 1..i + 1 + n].iter().any(|p| p.is_empty()) {
                truncated = true;
                i = toks.len();
                break;
            }
            entries.push(file_of(letter, &toks[i + 1..i + 1 + n]));
            i += 1 + n;
        }
        if truncated {
            continue;
        }
        let sha = f[0].trim_start_matches('\n').trim_start_matches('\x1e');
        let Ok(time) = f[1].parse::<i64>() else { continue };
        if !is_sha(sha) {
            continue;
        }
        let file = match entries.iter().position(|e| e.path == expect) {
            Some(k) => entries.swap_remove(k),
            // Another file under one of this file's old names: not this file's history.
            None if !entries.is_empty() => continue,
            None => HistoryFile { path: expect, from: None, status: None },
        };
        let commit = Commit {
            repo: String::new(),
            sha: sha.to_string(),
            time,
            author: f[2].to_string(),
            email: f[3].to_string(),
            subject: f[4].to_string(),
            parents: f[5].split(' ').filter(|p| !p.is_empty()).map(String::from).collect(),
        };
        rows.push((commit, file));
    }
    (rows, count)
}
