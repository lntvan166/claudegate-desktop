//! Compare Branches: every git argument and parser, ported from Polylog's `compareModel.ts`.
//! Pure: no process is spawned here.
//!
//! Where this differs from Polylog: branch names go after `--end-of-options` and revisions are
//! followed by `--`; logs and diffs never run signatures, textconv or an external diff; the side
//! log's fields are NUL-terminated and counted (as [`super::LOG_FORMAT`] is), because a subject
//! can hold US or RS and forge a field or a record.

use std::collections::HashSet;

use super::filter::is_valid_ref;
use super::parse::{is_sha, ChangedFile, FileStatus};

/// The two branch names, as the user picked them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Commits shown per side before "Show 500 more".
pub const COMMIT_PAGE: usize = 500;

/// Names cross the webview boundary: only what git accepts as a branch name reaches git.
pub fn valid_pair(p: &Pair) -> bool {
    is_valid_ref(&p.left) && is_valid_ref(&p.right)
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Four NUL-terminated fields per ref: refname, object type, object id, tree id.
pub const RESOLVE_FORMAT: &str = "--format=%(refname)%00%(objecttype)%00%(objectname)%00%(tree)%00";
const RESOLVE_FIELDS: usize = 4;

/// Both tips and their trees in one spawn, looked up by exact refname: `refs/heads/<name>` or
/// `refs/remotes/<name>`, never a tag and never git's DWIM order (where `refs/tags/x` shadows a
/// branch `x`, and a local `origin/x` shadows the remote one). for-each-ref matches its patterns
/// as prefixes too, so [`parse_resolve`] keeps exact matches only.
pub fn resolve_args(p: &Pair) -> Vec<String> {
    let mut v: Vec<String> = vec!["for-each-ref".into(), RESOLVE_FORMAT.into(), "--end-of-options".into()];
    for n in [&p.left, &p.right] {
        v.push(format!("refs/heads/{n}"));
        v.push(format!("refs/remotes/{n}"));
    }
    v
}

/// One name's `(commit, tree)` from `resolve_args` output: the local branch when there is one,
/// else the remote-tracking one. A ref that is not a commit counts as absent.
fn resolve_one(refs: &[(&str, &str, &str, &str)], name: &str) -> Option<(String, String)> {
    for full in [format!("refs/heads/{name}"), format!("refs/remotes/{name}")] {
        if let Some((_, kind, id, tree)) = refs.iter().find(|r| r.0 == full) {
            if *kind == "commit" && is_sha(id) && is_sha(tree) {
                return Some((id.to_string(), tree.to_string()));
            }
            return None;
        }
    }
    None
}

/// `(left, right, same_tree)`, or `None` when either name is no branch of the repository.
pub fn parse_resolve(out: &str, p: &Pair) -> Option<(String, String, bool)> {
    let mut fields: Vec<&str> = out.split('\0').collect();
    fields.pop();
    let refs: Vec<(&str, &str, &str, &str)> = fields.chunks_exact(RESOLVE_FIELDS).map(|f| (f[0].trim_start_matches('\n'), f[1], f[2], f[3])).collect();
    let (l, lt) = resolve_one(&refs, &p.left)?;
    let (r, rt) = resolve_one(&refs, &p.right)?;
    Some((l, r, lt == rt))
}

/// The merge base of two commit ids. Exit 1 with no output: no common history.
pub fn merge_base_args(l: &str, r: &str) -> Vec<String> {
    strings(&["merge-base", "--end-of-options", l, r])
}

fn side_flag(side: Side) -> &'static str {
    match side {
        Side::Left => "--left-only",
        Side::Right => "--right-only",
    }
}

/// A side's commits the other lacks, and (counted apart) its commits whose change the other has
/// too. Merges carry no change of their own (Files shows what they brought): they are not counted.
pub fn side_count_args(side: Side, l: &str, r: &str) -> Vec<String> {
    vec!["rev-list".into(), side_flag(side).into(), "--cherry-mark".into(), "--no-merges".into(), "--count".into(), format!("{l}...{r}"), "--".into()]
}

/// `(only, same)` from `rev-list --cherry-mark --count`: `"2\t1"`. Missing numbers are 0.
pub fn parse_side_count(out: &str) -> (u32, u32) {
    let mut it = out.split_whitespace().map(|n| n.parse::<u32>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// What a side changed since the branches split: merge base → its tip, statuses and counts in
/// one spawn.
pub fn files_args(base: &str, tip: &str) -> Vec<String> {
    strings(&["diff", "--raw", "--numstat", "-z", "-M", "--no-ext-diff", "--no-textconv", base, tip, "--"])
}

/// `git diff --raw --numstat -z` output: the `--raw` records (`:modes shas STATUS\0path\0`, two
/// paths for a rename or copy), then the `--numstat` records (`a\td\tpath\0`, or
/// `a\td\t\0old\0new\0`). Parsed by position, never by content, so a path is taken verbatim
/// whatever it holds. A count of `-` is binary (`None`).
pub fn parse_diff_z(out: &str) -> Vec<ChangedFile> {
    let toks: Vec<&str> = out.split('\0').collect();
    let mut i = 0;
    let mut files: Vec<ChangedFile> = Vec::new();
    // --raw: a header always begins with ':'. A numstat record begins with a digit or '-'.
    while i < toks.len() && toks[i].starts_with(':') {
        let letter = toks[i].rsplit(' ').next().and_then(|st| st.chars().next()).unwrap_or('M');
        let two = matches!(letter, 'R' | 'C');
        let need = if two { 2 } else { 1 };
        if i + need >= toks.len() {
            break; // cut off
        }
        let (first, path) = (toks[i + 1].to_string(), toks[i + need].to_string());
        let status = match letter {
            'A' => FileStatus::Added,
            'D' => FileStatus::Deleted,
            'T' => FileStatus::TypeChanged,
            'R' => FileStatus::Renamed { from: first },
            'C' => FileStatus::Copied { from: first },
            _ => FileStatus::Modified,
        };
        files.push(ChangedFile { path, status, added: None, removed: None });
        i += need + 1;
    }
    // --numstat, in the same order as --raw.
    let num = |s: &str| if s == "-" { Some(None) } else { s.parse::<u32>().ok().map(Some) };
    let mut stats: Vec<(String, Option<u32>, Option<u32>)> = Vec::new();
    while i < toks.len() {
        let mut it = toks[i].splitn(3, '\t');
        let (Some(a), Some(d), Some(p)) = (it.next(), it.next(), it.next()) else { break };
        let (Some(a), Some(d)) = (num(a), num(d)) else { break };
        if p.is_empty() {
            if i + 2 >= toks.len() {
                break;
            }
            stats.push((toks[i + 2].to_string(), a, d));
            i += 3;
        } else {
            stats.push((p.to_string(), a, d));
            i += 1;
        }
    }
    if files.is_empty() {
        // Numstat alone (as in tests that build one side): each file as modified.
        return stats.into_iter().map(|(path, added, removed)| ChangedFile { path, status: FileStatus::Modified, added, removed }).collect();
    }
    for (k, f) in files.iter_mut().enumerate() {
        let hit = match stats.get(k) {
            Some(s) if s.0 == f.path => Some(s),
            _ => stats.iter().find(|s| s.0 == f.path),
        };
        if let Some((_, a, d)) = hit {
            f.added = *a;
            f.removed = *d;
        }
    }
    files
}

/// The paths both sides changed, matched on each side's new path, sorted.
pub fn both_paths(a: &[ChangedFile], b: &[ChangedFile]) -> Vec<String> {
    let other: HashSet<&str> = b.iter().map(|f| f.path.as_str()).collect();
    let mut v: Vec<String> = a.iter().filter(|f| other.contains(f.path.as_str())).map(|f| f.path.clone()).collect();
    v.sort();
    v.dedup();
    v
}

/// The file's path at each tip, `(left, right)`: renamed on this side, the other tip still has it
/// under its old name.
pub fn tip_paths(side: Side, path: &str, from: Option<&str>) -> (String, String) {
    let other = from.unwrap_or(path).to_string();
    match side {
        Side::Left => (path.to_string(), other),
        Side::Right => (other, path.to_string()),
    }
}

/// Six NUL-terminated fields: mark, sha, parents, committer time, author, subject.
pub const SIDE_FORMAT: &str = "--format=%m%x00%H%x00%P%x00%ct%x00%aN%x00%s%x00";
const SIDE_FIELDS: usize = 6;

/// A side's commits, newest first: `max` of them at most, duplicates (`=`) included.
pub fn side_log_args(side: Side, l: &str, r: &str, max: usize) -> Vec<String> {
    vec![
        "log".into(),
        "--no-show-signature".into(),
        side_flag(side).into(),
        "--cherry-mark".into(),
        "--no-merges".into(),
        format!("--max-count={max}"),
        SIDE_FORMAT.into(),
        format!("{l}...{r}"),
        "--".into(),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideCommit {
    /// `'+'`: only on this side; `'='`: the same change is on the other side too.
    pub mark: char,
    pub sha: String,
    pub parents: Vec<String>,
    pub time: i64,
    pub author: String,
    pub subject: String,
}

pub fn parse_side_log(out: &str) -> Vec<SideCommit> {
    let mut fields: Vec<&str> = out.split('\0').collect();
    // What follows the last NUL is git's trailing newline or a field cut off by a killed process.
    fields.pop();
    let mut v = Vec::new();
    for f in fields.chunks_exact(SIDE_FIELDS) {
        let mark = f[0].trim_start_matches('\n');
        let Ok(time) = f[3].parse::<i64>() else { continue };
        if !is_sha(f[1]) {
            continue;
        }
        v.push(SideCommit {
            mark: if mark == "=" { '=' } else { '+' },
            sha: f[1].to_string(),
            parents: f[2].split(' ').filter(|p| !p.is_empty()).map(String::from).collect(),
            time,
            author: f[4].to_string(),
            subject: f[5].to_string(),
        });
    }
    v
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    pub subject: String,
    pub left: Option<String>,
    pub right: Option<String>,
}

/// Duplicates for display: a left and a right one with the same subject share a row; the rest
/// stay on their side. Counts come from git, not from this.
pub fn pair_duplicates(left: &[SideCommit], right: &[SideCommit]) -> Vec<Duplicate> {
    let mut rights: Vec<&SideCommit> = right.iter().collect();
    let mut out = Vec::new();
    for l in left {
        match rights.iter().position(|r| r.subject == l.subject) {
            Some(i) => {
                let r = rights.remove(i);
                out.push(Duplicate { subject: l.subject.clone(), left: Some(l.sha.clone()), right: Some(r.sha.clone()) });
            }
            None => out.push(Duplicate { subject: l.subject.clone(), left: Some(l.sha.clone()), right: None }),
        }
    }
    out.extend(rights.into_iter().map(|r| Duplicate { subject: r.subject.clone(), left: None, right: Some(r.sha.clone()) }));
    out
}

/// Every local and remote-tracking branch of a repository.
pub fn branch_refs_args() -> Vec<String> {
    strings(&["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"])
}

/// A branch for the picker: its name without `refs/heads/` or `refs/remotes/`, and which of the
/// two it came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchRef {
    pub name: String,
    pub remote: bool,
}

/// Each branch once, in git's order, classified by its refname: a local `origin/x` is local.
/// `refs/remotes/*/HEAD` is dropped, and so is any name git would not take as a branch.
pub fn parse_branch_refs(out: &str) -> Vec<BranchRef> {
    let mut v: Vec<BranchRef> = Vec::new();
    let mut seen: HashSet<BranchRef> = HashSet::new();
    for line in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let (name, remote) = if let Some(n) = line.strip_prefix("refs/heads/") {
            (n, false)
        } else if let Some(n) = line.strip_prefix("refs/remotes/") {
            if n.split_once('/').is_none_or(|(_, rest)| rest == "HEAD") {
                continue;
            }
            (n, true)
        } else {
            continue;
        };
        let b = BranchRef { name: name.to_string(), remote };
        if is_valid_ref(name) && seen.insert(b.clone()) {
            v.push(b);
        }
    }
    v
}
