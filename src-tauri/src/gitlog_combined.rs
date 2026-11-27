//! Combined Changes behind the Git log view: several selected commits, each file once.
//!
//! The model is `claudegate_core::gitlog::combined`; this module gives it real git through the
//! pooled [`Runner`], on the window's own `combined` channel ([`channel`]), so a new selection
//! supersedes only the previous selection and never the log's paging. Read-only, like the rest of
//! the Git log: every child is a `git` read with optional locks off.
//!
//! Nothing from the webview is trusted: every sha must be a full commit id, every path a plain
//! path in the repository, and a repository is only ever one of the backend's own list.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use claudegate_core::gitlog::{is_sha, left_side, par_map, read_combined, Blob, CombinedRead, Fallback, GitRead, LeftSide, Mode, NetStatus, ReadError, RepoRead, COMBINED_LIMIT};
use serde::{Deserialize, Serialize};

use crate::gitlog::{channel, tree_path_ok, GitLog, GitLogState, RunError, Runner, Tag, STALE};

/// The channel Combined Changes' queries run on.
pub const CHANNEL: &str = "combined";

/// One selected commit, as the webview names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRef {
    pub repo: String,
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedView {
    pub query_id: u64,
    /// One per repository, in the order each first appears in the request.
    pub repos: Vec<CombinedRepoView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedRepoView {
    pub repo: String,
    /// The selected commits found in the range, oldest first.
    pub commits: Vec<String>,
    pub files: Vec<CombinedFileView>,
    /// Files whose selected changes cancel out.
    pub no_net_change: Vec<String>,
    /// Subjects of the unselected commits in range, by sha.
    pub subjects: BTreeMap<String, String>,
    /// The repository could not be read: git's first stderr line. Everything else is then empty.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedFileView {
    pub path: String,
    /// Renamed within the range: its path before.
    pub from: Option<String>,
    /// `added`, `modified`, `deleted` or `renamed`.
    pub status: &'static str,
    /// The selected commits that touched it, oldest first.
    pub shas: Vec<String>,
    pub last: String,
    pub before_ref: Option<String>,
    pub before_path: String,
    /// Unselected commits the diff carries.
    pub included: Vec<String>,
    /// `fast`, `merge` or `fallback`.
    pub mode: &'static str,
    /// Set for `fallback`: `conflict`, `binary` or `tooLarge`.
    pub reason: Option<&'static str>,
}

/// A file's left side. Serialized as `{kind: "text", value}` or `{kind: "fallback", value: <reason>}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum CombinedLeftView {
    Text(String),
    Fallback(&'static str),
}

fn status_name(s: NetStatus) -> &'static str {
    match s {
        NetStatus::Added => "added",
        NetStatus::Modified => "modified",
        NetStatus::Deleted => "deleted",
        NetStatus::Renamed => "renamed",
    }
}

fn reason_name(f: Fallback) -> &'static str {
    match f {
        Fallback::Conflict => "conflict",
        Fallback::Binary => "binary",
        Fallback::TooLarge => "tooLarge",
    }
}

impl From<CombinedRead> for CombinedFileView {
    fn from(r: CombinedRead) -> Self {
        let f = r.file;
        let mode = match r.mode {
            Mode::Fast => "fast",
            Mode::Merge => "merge",
            Mode::Fallback => "fallback",
        };
        CombinedFileView {
            path: f.path,
            from: f.old_path,
            status: status_name(f.status),
            shas: f.shas,
            last: f.last,
            before_ref: f.before_ref,
            before_path: f.before_path,
            included: f.included,
            mode,
            reason: r.reason.map(reason_name),
        }
    }
}

fn repo_view(repo: String, r: RepoRead) -> CombinedRepoView {
    CombinedRepoView {
        repo,
        commits: r.commits,
        files: r.files.into_iter().map(CombinedFileView::from).collect(),
        no_net_change: r.no_net_change,
        subjects: r.subjects,
        error: None,
    }
}

fn failed_view(repo: String, why: &str) -> CombinedRepoView {
    let line = why.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("git failed").to_string();
    CombinedRepoView { repo, commits: Vec::new(), files: Vec::new(), no_net_change: Vec::new(), subjects: BTreeMap::new(), error: Some(line) }
}

/// [`GitRead`] over the pooled runner: every child belongs to `tag`, so a newer selection kills it.
pub struct TaggedGit<'a> {
    pub runner: &'a Runner,
    pub tag: &'a Tag,
    pub cwd: &'a Path,
}

fn read_error(e: RunError) -> ReadError {
    match e {
        RunError::Cancelled => ReadError::Cancelled,
        RunError::Spawn(s) | RunError::Failed(s) => ReadError::Failed(s),
    }
}

impl GitRead for TaggedGit<'_> {
    fn read(&self, args: &[&str]) -> Result<Vec<u8>, ReadError> {
        self.runner.run(Some(self.tag), self.cwd, args).map_err(read_error)
    }

    /// The size first (`cat-file -s`), so a blob over `max` is never read. A failed size: no such
    /// object.
    fn blob(&self, spec: &str, max: u64) -> Result<Blob, ReadError> {
        let size = match self.runner.text(Some(self.tag), self.cwd, &["cat-file", "-s", spec]) {
            Ok(s) => s,
            Err(RunError::Failed(_)) => return Ok(Blob::Missing),
            Err(e) => return Err(read_error(e)),
        };
        let size: u64 = size.trim().parse().map_err(|_| ReadError::Failed("Unexpected output from git cat-file".into()))?;
        if size > max {
            return Ok(Blob::TooLarge(size));
        }
        self.read(&["cat-file", "blob", spec]).map(Blob::Bytes)
    }
}

fn bad_sha() -> String {
    "Not a commit id".into()
}

fn bad_path() -> String {
    "Not a path in the repository".into()
}

impl GitLog {
    /// Combined Changes of `items`, grouped by repository, each repository read in parallel.
    /// Starts query `query_id` on the window's `combined` channel (superseding, and killing, the
    /// previous selection's reads; never the log's). One repository failing fills its `error`;
    /// a superseded query returns `Err(STALE)`.
    pub fn combined(&self, window: &str, query_id: u64, items: &[CommitRef]) -> Result<CombinedView, String> {
        // Validation first: a request refused here supersedes nothing.
        let mut seen: HashSet<&CommitRef> = HashSet::new();
        let distinct: Vec<&CommitRef> = items.iter().filter(|i| seen.insert(*i)).collect();
        if distinct.len() > COMBINED_LIMIT {
            return Err(format!("Combined Changes shows up to {COMBINED_LIMIT} commits"));
        }
        if distinct.iter().any(|i| !is_sha(&i.sha)) {
            return Err(bad_sha());
        }
        // Group by repository, in order of first appearance; each resolved from the backend's list.
        let mut groups: Vec<(String, PathBuf, Vec<String>)> = Vec::new();
        for i in distinct {
            match groups.iter_mut().find(|g| g.0 == i.repo) {
                Some(g) => g.2.push(i.sha.clone()),
                None => {
                    let cwd = self.cwd_of(&i.repo)?;
                    groups.push((i.repo.clone(), cwd, vec![i.sha.clone()]));
                }
            }
        }
        let tag = Tag { window: channel(window, CHANNEL), query: query_id };
        if !self.runner.set_current(&tag.window, query_id) {
            return Err(STALE.into());
        }
        let reads = par_map(&groups, |(_, cwd, shas)| read_combined(&TaggedGit { runner: &self.runner, tag: &tag, cwd }, shas));
        let mut repos = Vec::new();
        for ((repo, _, _), r) in groups.into_iter().zip(reads) {
            match r {
                Ok(r) => repos.push(repo_view(repo, r)),
                Err(ReadError::Failed(why)) => repos.push(failed_view(repo, &why)),
                Err(ReadError::Cancelled) => return Err(STALE.into()),
            }
        }
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        Ok(CombinedView { query_id, repos })
    }

    /// The left side of one file of the current selection: `path` at `last` with the commits
    /// `without` (newest first) taken back out. It runs under the selection's query and never
    /// starts one, so opening a file never supersedes the tree; a superseded selection returns
    /// `Err(STALE)`.
    pub fn combined_left(&self, window: &str, query_id: u64, repo: &str, last: &str, path: &str, without: &[String]) -> Result<CombinedLeftView, String> {
        if !is_sha(last) || without.iter().any(|s| !is_sha(s)) {
            return Err(bad_sha());
        }
        if !tree_path_ok(path) {
            return Err(bad_path());
        }
        let cwd = self.cwd_of(repo)?;
        let tag = Tag { window: channel(window, CHANNEL), query: query_id };
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        let git = TaggedGit { runner: &self.runner, tag: &tag, cwd: &cwd };
        let left = match left_side(&git, last, path, without) {
            Ok(l) => l,
            Err(ReadError::Cancelled) => return Err(STALE.into()),
            Err(ReadError::Failed(why)) => return Err(why),
        };
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        Ok(match left {
            LeftSide::Text(t) => CombinedLeftView::Text(t),
            LeftSide::Fallback(f) => CombinedLeftView::Fallback(reason_name(f)),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------------------------

#[tauri::command]
pub async fn gitlog_combined(query_id: u64, items: Vec<CommitRef>, window: tauri::Window, state: tauri::State<'_, GitLogState>) -> Result<CombinedView, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || gl.combined(&label, query_id, &items)).await?
}

#[tauri::command]
pub async fn gitlog_combined_left(
    query_id: u64,
    repo: String,
    last: String,
    path: String,
    without: Vec<String>,
    window: tauri::Window,
    state: tauri::State<'_, GitLogState>,
) -> Result<CombinedLeftView, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || gl.combined_left(&label, query_id, &repo, &last, &path, &without)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_serialize_camel_case() {
        assert_eq!(serde_json::to_value(CombinedLeftView::Fallback("tooLarge")).unwrap(), serde_json::json!({"kind": "fallback", "value": "tooLarge"}));
        assert_eq!(serde_json::to_value(CombinedLeftView::Text("x".into())).unwrap(), serde_json::json!({"kind": "text", "value": "x"}));
        let r: CommitRef = serde_json::from_value(serde_json::json!({"repo": "/r/.git", "sha": "a"})).unwrap();
        assert_eq!(r, CommitRef { repo: "/r/.git".into(), sha: "a".into() });
        let v = serde_json::to_value(failed_view("/r/.git".into(), "\nfatal: bad object\nmore")).unwrap();
        assert_eq!(v["error"], "fatal: bad object");
        assert!(v.get("noNetChange").is_some());
    }
}
