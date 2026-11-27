//! Combined Changes: several selected commits of one repository, each file once. From the range
//! log, which files the selected commits changed, with each one's net status and whether its diff
//! can take the fast path; for the others, the left side: the file after the newest selected
//! commit with the selected commits' changes taken back out, newest first, by a three-way merge
//! in memory.
//!
//! Read-only: git is only read through [`GitRead`], nothing is written to the repository (no
//! objects, no index) and nothing is cached. Ported from Polylog's `combinedModel.ts` and
//! `combinedRead.ts`, with these deliberate differences:
//!
//! - **Which unselected commits a file includes.** Polylog only counted unselected commits that
//!   touched the file after the first selected one in the range log's order, so one that came
//!   earlier (a side branch merged in by a selected merge, or a commit below the oldest selected
//!   one on another branch) was passed off on the fast path or left unnamed by the fallback. Here
//!   every unselected commit in range that touched the file counts when the plain diff
//!   `before_ref ↔ last` carries its change: it is an ancestor of `last` and not of `before_ref`
//!   (brought in), or of `before_ref` and not of `last` (taken back out).
//! - **The selection is sorted** before git sees it, so the range log's order, and with it `last`
//!   on two branches, depends on the set of commits and never on the order they were passed in.
//! - **Submodules** (mode 160000) have no text: a step through one falls back as binary.
//! - A missing right side is an empty file only when the newest selected commit deleted it; a
//!   commit `git show` does not describe falls back instead of being skipped.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

use super::discover::par_map_limit;
use super::line_merge::{merge3, MergeResult};
use super::parse::is_sha;

/// The most commits one Combined Changes selection takes.
pub const COMBINED_LIMIT: usize = 100;
/// A side larger than this skips the merge and takes the fallback.
pub const MERGE_LIMIT_BYTES: u64 = 1_000_000;
/// A side with more lines than this skips the merge and takes the fallback.
pub const MERGE_LIMIT_LINES: usize = 20_000;
/// The files that may conflict are merged a few at a time: each is a handful of git reads.
pub const SLOW_FILES_AT_ONCE: usize = 4;

/// `%x1e` starts a record, `%x1f` ends the ids: a subject may contain either, so the header is
/// split at the FIRST unit separator and never on a record separator.
pub const RANGE_FORMAT: &str = "--format=%x1e%H %P%x1f%s";

/// One file a commit changed, as `--raw -z` prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChange {
    /// One of `A M D R C T`.
    pub status: char,
    /// Modes as git prints them (`100644`, `000000` when absent, `160000` for a submodule).
    pub old_mode: String,
    pub new_mode: String,
    pub path: String,
    /// Renames and copies: the path before.
    pub old_path: Option<String>,
    pub old_blob: String,
    pub new_blob: String,
}

/// One commit of the range. A range log lists them newest first, in topological order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub subject: Option<String>,
    pub changes: Vec<RawChange>,
}

/// A file's status over the whole selection (CC §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedFile {
    /// Its path after the newest selected commit that touched it.
    pub path: String,
    /// Renamed within the range: its path before.
    pub old_path: Option<String>,
    pub status: NetStatus,
    /// The selected commits that touched it, oldest first.
    pub shas: Vec<String>,
    pub last: String,
    /// The first parent of `shas[0]`; `None` for a root commit.
    pub before_ref: Option<String>,
    /// Its path in `before_ref`.
    pub before_path: String,
    /// Unselected commits in range that touched it before `last`, oldest first.
    pub included: Vec<String>,
    /// Nothing unselected touched it, and every selected commit is an ancestor of `last`: the
    /// diff is `before_ref` ↔ `last`.
    pub fast: bool,
}

/// The commit every selected commit descends from: the range starts there.
pub fn merge_base_args(selected: &[String]) -> Vec<String> {
    let mut a = vec!["merge-base".to_string(), "--octopus".to_string()];
    a.extend(selected.iter().cloned());
    a
}

/// What the range log excludes, given the merge base. No base (unrelated histories): nothing. A
/// base that is itself selected: its parents, as `<base>^@`, so the parents are git's and never
/// taken from the webview (a root commit then excludes nothing). Any other base: the base.
pub fn exclude_for(base: &str, selected: &[String]) -> Vec<String> {
    if base.is_empty() {
        Vec::new()
    } else if selected.iter().any(|s| s == base) {
        vec![format!("{base}^@")]
    } else {
        vec![base.to_string()]
    }
}

/// The flags both the range log and a single commit's read use. `--root` and
/// `--no-show-signature` override a user's `log.showRoot=false` (a root commit with no files) and
/// `log.showSignature=true` (gpg lines in the output).
const RAW_FLAGS: [&str; 8] = ["--raw", "-z", "-M", "--no-abbrev", "--diff-merges=first-parent", "--root", "--no-show-signature", RANGE_FORMAT];

/// Every commit reachable from the selected ones and not from `exclude`, with each one's files
/// (merges: against their first parent). The trailing `--` keeps a ref from reading as a path.
pub fn range_log_args(selected: &[String], exclude: &[String]) -> Vec<String> {
    let mut a: Vec<String> = ["log", "--topo-order"].iter().chain(RAW_FLAGS.iter()).map(|s| s.to_string()).collect();
    a.extend(selected.iter().cloned());
    if !exclude.is_empty() {
        a.push("--not".to_string());
        a.extend(exclude.iter().cloned());
    }
    a.push("--".to_string());
    a
}

/// One commit's changed files, with blob ids (merges: against their first parent; a root commit:
/// everything added). The trailing `--` keeps a work-tree file named like the sha from making the
/// argument ambiguous.
pub fn show_raw_args(sha: &str) -> Vec<String> {
    let mut a: Vec<String> = std::iter::once("show").chain(RAW_FLAGS.iter().copied()).map(|s| s.to_string()).collect();
    a.push(sha.to_string());
    a.push("--".to_string());
    a
}

/// The mode of a submodule entry (a gitlink): its "blob" is a commit, not text.
pub const GITLINK_MODE: &str = "160000";

fn raw_re() -> &'static Regex {
    static RAW: OnceLock<Regex> = OnceLock::new();
    RAW.get_or_init(|| Regex::new(r"^:\d{6} \d{6} [0-9a-f]+ [0-9a-f]+ [AMDRCT]\d*$").unwrap())
}

/// Parses `--raw -z` output with [`RANGE_FORMAT`] headers, positionally: a header token, then
/// raw lines each followed by its path token(s). Paths are taken verbatim and never matched
/// against anything, so a path holding `\x1e`, a newline or `:100644 …` is still just a path. A
/// change cut off before its path (a killed process) ends its commit.
pub fn parse_range_log(out: &str) -> Vec<RangeCommit> {
    let tokens: Vec<&str> = out.split('\0').collect();
    let mut commits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].trim_start_matches('\n');
        i += 1;
        let Some(header) = t.strip_prefix('\x1e') else { continue };
        let (ids, subject) = match header.split_once('\x1f') {
            Some((ids, subject)) => (ids, Some(subject.to_string())),
            None => (header, None),
        };
        let mut words = ids.split(' ').filter(|w| !w.is_empty());
        let sha = words.next().unwrap_or("").to_string();
        let parents: Vec<String> = words.map(str::to_string).collect();
        // Its changes are consumed even when the record is dropped, so none of its path tokens
        // is ever read as a header.
        let mut changes = Vec::new();
        while i < tokens.len() {
            let r = tokens[i].trim_start_matches('\n');
            if !raw_re().is_match(r) {
                break;
            }
            let fields: Vec<&str> = r.split(' ').collect();
            let status = fields[4].chars().next().unwrap_or('M');
            let two = status == 'R' || status == 'C';
            let need = if two { 2 } else { 1 };
            let paths: Vec<&str> = tokens[i + 1..].iter().take(need).copied().collect();
            i += 1 + need;
            if paths.len() < need || paths.iter().any(|p| p.is_empty()) {
                break;
            }
            let (path, old_path) = if two { (paths[1].to_string(), Some(paths[0].to_string())) } else { (paths[0].to_string(), None) };
            let old_mode = fields[0].trim_start_matches(':').to_string();
            changes.push(RawChange { status, path, old_path, old_mode, new_mode: fields[1].to_string(), old_blob: fields[2].to_string(), new_blob: fields[3].to_string() });
        }
        if is_sha(&sha) {
            commits.push(RangeCommit { sha, parents, subject, changes });
        }
    }
    commits
}

struct Acc {
    /// Its path after the newest selected commit that touched it: the path shown.
    shown_path: String,
    before_path: String,
    before_ref: Option<String>,
    first: RawChange,
    last: RawChange,
    shas: Vec<String>,
    /// Every unselected commit in range that touched it, oldest first, including those before
    /// its first selected commit; [`combine_repo`] keeps the ones the plain diff carries.
    touched: Vec<String>,
}

fn net_status(a: &Acc) -> Option<NetStatus> {
    let (first, last) = (a.first.status, a.last.status);
    if first == 'A' && last == 'D' {
        return None;
    }
    Some(if first == 'A' {
        NetStatus::Added
    } else if last == 'D' {
        NetStatus::Deleted
    } else if first == 'D' {
        NetStatus::Modified
    } else if a.shown_path != a.before_path {
        NetStatus::Renamed
    } else {
        NetStatus::Modified
    })
}

/// The commits in `log` that `sha` descends from, itself included.
fn ancestors(sha: &str, log: &[RangeCommit]) -> HashSet<String> {
    let by_sha: HashMap<&str, &RangeCommit> = log.iter().map(|c| (c.sha.as_str(), c)).collect();
    let mut seen = HashSet::new();
    let mut stack = vec![sha.to_string()];
    while let Some(s) = stack.pop() {
        if !seen.insert(s.clone()) {
            continue;
        }
        if let Some(c) = by_sha.get(s.as_str()) {
            stack.extend(c.parents.iter().filter(|p| by_sha.contains_key(p.as_str())).cloned());
        }
    }
    seen
}

/// Each file the selected commits changed, once, with its net status and path; and the shown
/// paths of the files whose selected changes cancel out (added then deleted, or back to the same
/// blob on the fast path).
pub fn combine_repo(selected: &HashSet<String>, log: &[RangeCommit]) -> (Vec<CombinedFile>, Vec<String>) {
    let mut by_path: HashMap<String, usize> = HashMap::new();
    let mut all: Vec<Acc> = Vec::new();
    // Unselected commits that touched a path no selected commit has touched yet, followed through
    // renames the same way: they seed the entry a selected commit opens later.
    let mut before_any: HashMap<String, Vec<String>> = HashMap::new();
    for c in log.iter().rev() {
        let mine = selected.contains(&c.sha);
        for change in &c.changes {
            let was = change.old_path.as_ref().unwrap_or(&change.path);
            let renamed = change.old_path.as_ref().filter(|old| **old != change.path);
            let at = match by_path.get(was) {
                Some(&at) => at,
                None if !mine => {
                    let mut seen = match renamed {
                        Some(old) => before_any.remove(old).unwrap_or_default(),
                        None => before_any.remove(&change.path).unwrap_or_default(),
                    };
                    seen.push(c.sha.clone());
                    before_any.insert(change.path.clone(), seen);
                    continue;
                }
                None => {
                    let touched = before_any.remove(was).unwrap_or_default();
                    if renamed.is_some() {
                        before_any.remove(&change.path);
                    }
                    all.push(Acc {
                        shown_path: change.path.clone(),
                        before_path: was.clone(),
                        before_ref: c.parents.first().cloned(),
                        first: change.clone(),
                        last: change.clone(),
                        shas: Vec::new(),
                        touched,
                    });
                    all.len() - 1
                }
            };
            if let Some(old) = renamed {
                by_path.remove(old);
            }
            by_path.insert(change.path.clone(), at);
            let acc = &mut all[at];
            if mine {
                acc.shown_path = change.path.clone();
                acc.shas.push(c.sha.clone());
                acc.last = change.clone();
            } else {
                acc.touched.push(c.sha.clone());
            }
        }
    }
    let mut files = Vec::new();
    let mut no_net_change = Vec::new();
    for a in all {
        let status = net_status(&a);
        let last = a.shas.last().cloned().expect("an entry starts with a selected commit");
        let reach = ancestors(&last, log);
        let below = a.before_ref.as_deref().map(|b| ancestors(b, log)).unwrap_or_default();
        // The plain diff `before_ref ↔ last` carries the change of every commit that reaches
        // `last` but not `before_ref`, and takes back out every one that reaches `before_ref` but
        // not `last`. An unselected one among them that touched the file is included.
        let included: Vec<String> = a.touched.iter().filter(|s| reach.contains(*s) != below.contains(*s)).cloned().collect();
        // Fast: the diff carries nothing unselected, and every selected change (each reaches `last`).
        let fast = included.is_empty() && a.shas.iter().all(|s| reach.contains(s));
        let Some(status) = status else {
            no_net_change.push(a.shown_path);
            continue;
        };
        if fast && a.first.old_blob == a.last.new_blob && a.before_path == a.shown_path {
            no_net_change.push(a.shown_path);
            continue;
        }
        let old_path = (a.before_path != a.shown_path).then(|| a.before_path.clone());
        files.push(CombinedFile { path: a.shown_path, old_path, status, shas: a.shas, last, before_ref: a.before_ref, before_path: a.before_path, included, fast });
    }
    (files, no_net_change)
}

// ---------------------------------------------------------------------------------------------
// The left side.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The selection changed: the read was killed.
    Cancelled,
    Failed(String),
}

/// A blob as [`GitRead::blob`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blob {
    Bytes(Vec<u8>),
    /// No such object (a `<sha>:<path>` the commit does not have).
    Missing,
    /// Larger than the `max` asked for; its size. Not read.
    TooLarge(u64),
}

/// Reads git for Combined Changes. A trait so tests can use a fake; the app runs real git, killed
/// on a selection change (`ReadError::Cancelled`).
pub trait GitRead: Sync {
    /// Runs `git <args>` and returns its stdout.
    fn read(&self, args: &[&str]) -> Result<Vec<u8>, ReadError>;
    /// A blob by id or `<sha>:<path>`. Never reads more than `max` bytes.
    fn blob(&self, spec: &str, max: u64) -> Result<Blob, ReadError>;
}

/// Why a side can't go through the merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    Conflict,
    Binary,
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftSide {
    Text(String),
    Fallback(Fallback),
}

fn read_str(git: &dyn GitRead, args: &[String]) -> Result<String, ReadError> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    git.read(&args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// git's binary test (a NUL in the first 8,000 bytes), text that is not UTF-8, or the limits.
fn unmergeable_bytes(bytes: &[u8]) -> Option<Fallback> {
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Some(Fallback::Binary);
    }
    if std::str::from_utf8(bytes).is_err() {
        return Some(Fallback::Binary);
    }
    unmergeable_text(bytes)
}

fn unmergeable_text(bytes: &[u8]) -> Option<Fallback> {
    if bytes.len() as u64 > MERGE_LIMIT_BYTES || bytes.iter().filter(|&&b| b == b'\n').count() > MERGE_LIMIT_LINES {
        return Some(Fallback::TooLarge);
    }
    None
}

/// A side as read for the merge.
enum Side {
    Text(String),
    Unmergeable(Fallback),
    Missing,
}

fn side(git: &dyn GitRead, spec: &str) -> Result<Side, ReadError> {
    if is_no_blob(spec) {
        return Ok(Side::Text(String::new()));
    }
    Ok(match git.blob(spec, MERGE_LIMIT_BYTES)? {
        Blob::Bytes(b) => match unmergeable_bytes(&b) {
            Some(why) => Side::Unmergeable(why),
            None => Side::Text(String::from_utf8(b).expect("checked as UTF-8")),
        },
        Blob::TooLarge(_) => Side::Unmergeable(Fallback::TooLarge),
        Blob::Missing => Side::Missing,
    })
}

/// A blob id the range log named: it must exist.
fn step_side(git: &dyn GitRead, id: &str) -> Result<Result<String, Fallback>, ReadError> {
    match side(git, id)? {
        Side::Text(t) => Ok(Ok(t)),
        Side::Unmergeable(why) => Ok(Err(why)),
        Side::Missing => Err(ReadError::Failed(format!("missing blob {id}"))),
    }
}

fn is_no_blob(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b == b'0')
}

fn is_gitlink(c: &RawChange) -> bool {
    c.old_mode == GITLINK_MODE || c.new_mode == GITLINK_MODE
}

/// The file `path` at `last` with the commits `without` (newest first) taken back out, each by a
/// three-way merge: base = the commit's after, ours = the text so far, theirs = its before. A
/// rename by an unselected commit in between is followed back. Whatever the line alignment, the
/// selected changes put back onto the result must give `last:path` byte for byte; otherwise, and
/// on any conflict, the file falls back.
pub fn left_side(git: &dyn GitRead, last: &str, path: &str, without: &[String]) -> Result<LeftSide, ReadError> {
    left_and_right(git, last, path, without).map(|(left, _)| left)
}

/// [`left_side`], and the right side it was computed from (`last:path`; empty when `last`
/// deleted the file). The right side is `None` when the file fell back before it was read.
pub fn left_and_right(git: &dyn GitRead, last: &str, path: &str, without: &[String]) -> Result<(LeftSide, Option<String>), ReadError> {
    let fallback = |why| Ok((LeftSide::Fallback(why), None));
    let mut records: HashMap<String, Option<RangeCommit>> = HashMap::new();
    let mut record = |sha: &str| -> Result<Option<RangeCommit>, ReadError> {
        if let Some(r) = records.get(sha) {
            return Ok(r.clone());
        }
        let r = parse_range_log(&read_str(git, &show_raw_args(sha))?).into_iter().next();
        records.insert(sha.to_string(), r.clone());
        Ok(r)
    };
    // The right side. Missing reads as empty only when `last` deleted the file; otherwise the
    // path did not survive the read (a path that is not UTF-8), and the file falls back.
    let last_change = record(last)?.and_then(|r| r.changes.into_iter().find(|c| c.path == path));
    if last_change.as_ref().is_some_and(is_gitlink) {
        return fallback(Fallback::Binary);
    }
    let right = match side(git, &format!("{last}:{path}"))? {
        Side::Text(t) => t,
        Side::Unmergeable(why) => return fallback(why),
        Side::Missing if last_change.as_ref().is_some_and(|c| c.status == 'D') => String::new(),
        Side::Missing => return fallback(Fallback::Conflict),
    };
    let done = |left: LeftSide| Ok((left, Some(right.clone())));
    let mut text = right.clone();
    // Each step's file before and after its commit, newest first: checked again at the end.
    let mut steps: Vec<(String, String)> = Vec::new();
    let mut current = path.to_string();
    // The revision `text` stands for: renames between it and the next commit are followed back.
    let mut state = last.to_string();
    for sha in without {
        // A commit `git show` did not describe can't be taken out: leaving its change in would
        // make the left side wrong, so the file falls back (Polylog skipped it).
        let Some(commit) = record(sha)? else { return done(LeftSide::Fallback(Fallback::Conflict)) };
        let mut change = commit.changes.iter().find(|c| c.path == current);
        if change.is_none() && state != *sha {
            // Renamed by a commit in between (one not selected): its path after this commit.
            let out = read_str(git, &["diff", "--raw", "-z", "-M", "--no-abbrev", sha, &state, "--"].map(str::to_string))?;
            let between = parse_range_log(&format!("\x1e{sha}\0{out}")).into_iter().next();
            let renamed = between.and_then(|b| b.changes.into_iter().find(|c| c.path == current && c.old_path.is_some()));
            if let Some(r) = renamed {
                current = r.old_path.expect("checked");
                change = commit.changes.iter().find(|c| c.path == current);
            }
        }
        if let Some(p) = commit.parents.first() {
            state = p.clone();
        }
        // The range log says this commit touched the file: not finding it means the path was lost.
        let Some(change) = change else { return done(LeftSide::Fallback(Fallback::Conflict)) };
        // A submodule has no text to merge.
        if is_gitlink(change) {
            return done(LeftSide::Fallback(Fallback::Binary));
        }
        let after = step_side(git, &change.new_blob)?;
        let before = step_side(git, &change.old_blob)?;
        if let Some(why) = unmergeable_text(text.as_bytes()) {
            return done(LeftSide::Fallback(why));
        }
        let (after, before) = match (after, before) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(why), _) | (_, Err(why)) => return done(LeftSide::Fallback(why)),
        };
        text = match merge3(&after, &text, &before) {
            MergeResult::Clean(t) => t,
            MergeResult::Conflict => return done(LeftSide::Fallback(Fallback::Conflict)),
            MergeResult::TooLarge => return done(LeftSide::Fallback(Fallback::TooLarge)),
        };
        steps.push((before, after));
        if let Some(old) = &change.old_path {
            current = old.clone();
        }
    }
    // The guard: the selected changes put back onto the left side must give the right side
    // exactly. Where the line alignment made them come apart, show the plain diff instead.
    let mut check = text.clone();
    for (before, after) in steps.iter().rev() {
        match merge3(before, &check, after) {
            MergeResult::Clean(t) => check = t,
            _ => return done(LeftSide::Fallback(Fallback::Conflict)),
        }
    }
    if check != right {
        return done(LeftSide::Fallback(Fallback::Conflict));
    }
    done(LeftSide::Text(text))
}

// ---------------------------------------------------------------------------------------------
// One repository's read.

/// How a file's diff opens: before ↔ last, the merged left side, or the plain range diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Fast,
    Merge,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CombinedRead {
    pub file: CombinedFile,
    pub mode: Mode,
    /// Set for `Mode::Fallback`.
    pub reason: Option<Fallback>,
}

/// One repository's Combined Changes: its files, how each diff opens, and what cancelled out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRead {
    /// The selected commits found in the range, oldest first (topological).
    pub commits: Vec<String>,
    pub files: Vec<CombinedRead>,
    pub no_net_change: Vec<String>,
    /// Subjects of the unselected commits in range (a fallback names them).
    pub subjects: BTreeMap<String, String>,
}

/// A slow file's outcome.
enum Slow {
    Same,
    Merge,
    Fallback(Fallback),
}

/// One repository's Combined Changes for the `selected` commits: the merge base, the range log,
/// each file once, and the left side of every file that can't take the fast path, a few at a
/// time. A failed range log fails the repository; any cancelled read cancels it.
pub fn read_combined(git: &dyn GitRead, selected: &[String]) -> Result<RepoRead, ReadError> {
    // The range starts below the commit every selected one descends from (none: unrelated
    // histories).
    // Only commit ids reach git, and in one order: the range log's order (and with it `last` on
    // two branches) then depends on the set, never on the order it was passed in.
    if let Some(bad) = selected.iter().find(|s| !is_sha(s)) {
        return Err(ReadError::Failed(format!("not a commit id: {bad:?}")));
    }
    let mut sorted = selected.to_vec();
    sorted.sort();
    sorted.dedup();
    let selected = &sorted[..];
    let base = match read_str(git, &merge_base_args(selected)) {
        Ok(out) => out.trim().to_string(),
        Err(ReadError::Cancelled) => return Err(ReadError::Cancelled),
        Err(ReadError::Failed(_)) => String::new(),
    };
    let base = if is_sha(&base) { base } else { String::new() };
    let exclude = exclude_for(&base, selected);
    let log = parse_range_log(&read_str(git, &range_log_args(selected, &exclude))?);
    let chosen: HashSet<String> = selected.iter().cloned().collect();
    let (files, mut no_net_change) = combine_repo(&chosen, &log);
    let subjects: BTreeMap<String, String> = log.iter().filter(|c| !chosen.contains(&c.sha)).filter_map(|c| c.subject.clone().map(|s| (c.sha.clone(), s))).collect();
    let commits: Vec<String> = log.iter().rev().filter(|c| chosen.contains(&c.sha)).map(|c| c.sha.clone()).collect();

    let slow: Vec<usize> = (0..files.len()).filter(|&i| !files[i].fast).collect();
    let settled = par_map_limit(&slow, SLOW_FILES_AT_ONCE, |&i| -> Result<Slow, ReadError> {
        let f = &files[i];
        let without: Vec<String> = f.shas.iter().rev().cloned().collect();
        Ok(match left_and_right(git, &f.last, &f.path, &without)? {
            (LeftSide::Fallback(why), _) => Slow::Fallback(why),
            (LeftSide::Text(left), right) if right.as_deref() == Some(left.as_str()) => Slow::Same,
            (LeftSide::Text(_), _) => Slow::Merge,
        })
    });
    if settled.iter().any(|r| matches!(r, Err(ReadError::Cancelled))) {
        return Err(ReadError::Cancelled);
    }
    let mut how: HashMap<usize, Slow> = HashMap::new();
    for (i, r) in slow.into_iter().zip(settled) {
        how.insert(i, r?);
    }
    let mut out = Vec::new();
    for (i, f) in files.into_iter().enumerate() {
        match how.get(&i) {
            None => out.push(CombinedRead { file: f, mode: Mode::Fast, reason: None }),
            // The left side equals the right side: the selected changes cancel out.
            Some(Slow::Same) => no_net_change.push(f.path),
            Some(Slow::Merge) => out.push(CombinedRead { file: f, mode: Mode::Merge, reason: None }),
            Some(&Slow::Fallback(why)) => out.push(CombinedRead { file: f, mode: Mode::Fallback, reason: Some(why) }),
        }
    }
    Ok(RepoRead { commits, files: out, no_net_change, subjects })
}
