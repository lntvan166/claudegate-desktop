//! Compare Branches behind the Git log view: two branches across the ticked repositories.
//!
//! The model is `claudegate_core::gitlog::compare`; this module gives it real git through the
//! pooled [`Runner`], on the window's own `compare` channel ([`CHANNEL`]): a new pair supersedes
//! (and kills) the previous pair's reads, never the log's paging. One call answers every
//! repository; nothing is cached and no ref is watched, so each call reads git again.
//! Read-only, like the rest of the Git log.
//!
//! Nothing from the webview is trusted: branch names must be names git accepts as a branch,
//! commit ids full ids, and a repository only ever one of the backend's own list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use claudegate_core::gitlog::compare::{
    both_paths, branch_refs_args, files_args, merge_base_args, parse_branch_refs, parse_diff_z, parse_resolve, parse_side_count, parse_side_log, resolve_args, side_count_args, side_log_args, valid_pair, BranchRef, Pair,
    Side, SideCommit,
};
use claudegate_core::gitlog::{is_sha, par_map, ChangedFile};
use serde::Serialize;

use crate::gitlog::{channel, ChangedFileView, GitLog, GitLogState, RunError, Share, Tag, STALE};

/// The channel Compare Branches' queries run on.
pub const CHANNEL: &str = "compare";
/// A side's commits per call at most: 20 pages of 500, plus one to tell whether more exist.
pub const MAX_COMMITS: usize = 10_001;

/// One repository's comparison of the pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RepoCompareView {
    /// A branch is not in this repository.
    Missing,
    /// Both tips hold the same tree: nothing to review, whatever the commits.
    Identical,
    /// No common history.
    Nobase,
    Error {
        reason: String,
    },
    Differs {
        left_sha: String,
        right_sha: String,
        base: String,
        /// Commits only on the left (merges not counted).
        left: u32,
        right: u32,
        /// Left commits whose change the right has too.
        same_left: u32,
        same_right: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoCompareRow {
    pub repo: String,
    #[serde(flatten)]
    pub result: RepoCompareView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareView {
    pub query_id: u64,
    /// One per requested repository, in request order.
    pub rows: Vec<RepoCompareRow>,
}

/// Files mode for one repository: each side's changes since the split.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareFiles {
    pub left: Vec<ChangedFileView>,
    pub right: Vec<ChangedFileView>,
    /// Paths both sides changed (on each side's new path), sorted.
    pub both: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideCommitView {
    /// `+`: only on this side; `=`: the other side has the same change.
    pub mark: String,
    pub sha: String,
    pub parents: Vec<String>,
    pub time: i64,
    pub author: String,
    pub subject: String,
}

impl From<SideCommit> for SideCommitView {
    fn from(c: SideCommit) -> Self {
        SideCommitView { mark: c.mark.to_string(), sha: c.sha, parents: c.parents, time: c.time, author: c.author, subject: c.subject }
    }
}

/// A branch name for the picker, with how many of the asked repositories have it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchNameView {
    pub name: String,
    pub count: usize,
    /// A remote-tracking name (`origin/main`).
    pub remote: bool,
}

fn bad_sha() -> String {
    "Not a commit id".into()
}

fn bad_side() -> String {
    "Not a side".into()
}

/// The names cross the webview boundary: only branch names reach git. Each side carries its
/// kind (`left_remote`/`right_remote`) and is resolved as exactly that ref.
pub const BAD_PAIR: &str = "Not a branch name";

fn files_of(r: Result<Vec<u8>, RunError>) -> Result<Vec<ChangedFile>, RunError> {
    r.map(|b| parse_diff_z(&String::from_utf8_lossy(&b)))
}

fn join<T>(h: std::thread::ScopedJoinHandle<'_, Result<T, RunError>>) -> Result<T, RunError> {
    h.join().unwrap_or(Err(RunError::Spawn("git reader panicked".into())))
}

fn arg_refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

impl GitLog {
    /// One repository's answer, or `Err(())` when the query was superseded.
    fn compare_one(&self, tag: &Tag, cwd: &Path, pair: &Pair) -> Result<RepoCompareView, ()> {
        let error = |e: RunError| match e {
            RunError::Cancelled => Err(()),
            e => Ok(RepoCompareView::Error { reason: e.reason() }),
        };
        // A branch the repository lacks is no output, not a failure: a failure is an error.
        let ids = match self.runner.run(Some(tag), cwd, &arg_refs(&resolve_args(pair))) {
            Ok(out) => parse_resolve(&String::from_utf8_lossy(&out), pair),
            Err(e) => return error(e),
        };
        let Some((l, r, same_tree)) = ids else { return Ok(RepoCompareView::Missing) };
        if same_tree {
            return Ok(RepoCompareView::Identical);
        }
        // The merge base first: with no common history, --cherry-mark would compute a patch id
        // for every commit of both histories, for an answer that shows no counts.
        let base = match self.runner.run_code_err(Some(tag), cwd, &arg_refs(&merge_base_args(&l, &r))) {
            Ok((0, out, _)) => {
                let b = String::from_utf8_lossy(&out).trim().to_string();
                if !is_sha(&b) {
                    return Ok(RepoCompareView::Error { reason: "Unexpected output from git merge-base".into() });
                }
                b
            }
            Ok((1, _, _)) => return Ok(RepoCompareView::Nobase),
            Ok((code, _, err)) => return Ok(RepoCompareView::Error { reason: err.unwrap_or_else(|| format!("git merge-base exited with {code}")) }),
            Err(e) => return error(e),
        };
        // Both sides' counts at once.
        let (la, ra) = (side_count_args(Side::Left, &l, &r), side_count_args(Side::Right, &l, &r));
        let (lc, rc) = std::thread::scope(|s| {
            let lc = s.spawn(|| self.runner.run(Some(tag), cwd, &arg_refs(&la)));
            let rc = self.runner.run(Some(tag), cwd, &arg_refs(&ra));
            (join(lc), rc)
        });
        let (lc, rc) = match (lc, rc) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(RunError::Cancelled), _) | (_, Err(RunError::Cancelled)) => return Err(()),
            (Err(e), _) | (_, Err(e)) => return error(e),
        };
        let (left, same_left) = parse_side_count(&String::from_utf8_lossy(&lc));
        let (right, same_right) = parse_side_count(&String::from_utf8_lossy(&rc));
        Ok(RepoCompareView::Differs { left_sha: l, right_sha: r, base, left, right, same_left, same_right })
    }

    /// Branches `left` and `right` compared in each of `repos`, in parallel, answered in one
    /// response in request order. Starts query `query_id` on the window's `compare` channel,
    /// superseding (and killing) the previous pair's reads; a superseded call returns
    /// `Err(STALE)`.
    pub fn compare(&self, window: &str, query_id: u64, repos: &[String], pair: &Pair) -> Result<CompareView, String> {
        // Validation first: a request refused here supersedes nothing.
        if !valid_pair(pair) {
            return Err(BAD_PAIR.into());
        }
        let cwds: Vec<(String, PathBuf)> = repos.iter().map(|r| self.cwd_of(r).map(|c| (r.clone(), c))).collect::<Result<_, _>>()?;
        let tag = Tag { window: channel(window, CHANNEL), query: query_id };
        if !self.runner.set_current(&tag.window, query_id) {
            return Err(STALE.into());
        }
        let results = par_map(&cwds, |(_, cwd)| self.compare_one(&tag, cwd, pair));
        let mut rows = Vec::with_capacity(cwds.len());
        for ((repo, _), r) in cwds.into_iter().zip(results) {
            let Ok(result) = r else { return Err(STALE.into()) };
            rows.push(RepoCompareRow { repo, result });
        }
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        Ok(CompareView { query_id, rows })
    }

    /// Files mode for one repository: what each tip changed since `base`. Runs under the pair's
    /// query and never starts one; a superseded pair returns `Err(STALE)`.
    pub fn compare_files(&self, window: &str, query_id: u64, repo: &str, left_sha: &str, right_sha: &str, base: &str) -> Result<CompareFiles, String> {
        if ![left_sha, right_sha, base].iter().all(|s| is_sha(s)) {
            return Err(bad_sha());
        }
        let cwd = self.cwd_of(repo)?;
        let tag = Tag { window: channel(window, CHANNEL), query: query_id };
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        let (la, ra) = (files_args(base, left_sha), files_args(base, right_sha));
        let (left, right) = std::thread::scope(|s| {
            let l = s.spawn(|| files_of(self.runner.run_in(Some(&tag), Share::Reserved, &cwd, &arg_refs(&la))));
            let r = files_of(self.runner.run_in(Some(&tag), Share::Reserved, &cwd, &arg_refs(&ra)));
            (join(l), r)
        });
        let (left, right) = match (left, right) {
            (Ok(l), Ok(r)) => (l, r),
            (Err(RunError::Cancelled), _) | (_, Err(RunError::Cancelled)) => return Err(STALE.into()),
            (Err(e), _) | (_, Err(e)) => return Err(e.reason()),
        };
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        let both = both_paths(&left, &right);
        Ok(CompareFiles { left: left.into_iter().map(ChangedFileView::from).collect(), right: right.into_iter().map(ChangedFileView::from).collect(), both })
    }

    /// Commits mode for one side (`"left"` or `"right"`) of one repository, newest first, `max`
    /// at most (clamped to `1..=MAX_COMMITS`), duplicates (`=`) included. Runs under the pair's
    /// query and never starts one.
    #[allow(clippy::too_many_arguments)]
    pub fn compare_commits(&self, window: &str, query_id: u64, repo: &str, side: &str, left_sha: &str, right_sha: &str, max: usize) -> Result<Vec<SideCommitView>, String> {
        let side = match side {
            "left" => Side::Left,
            "right" => Side::Right,
            _ => return Err(bad_side()),
        };
        if !is_sha(left_sha) || !is_sha(right_sha) {
            return Err(bad_sha());
        }
        let cwd = self.cwd_of(repo)?;
        let tag = Tag { window: channel(window, CHANNEL), query: query_id };
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        let args = side_log_args(side, left_sha, right_sha, max.clamp(1, MAX_COMMITS));
        let out = match self.runner.run_in(Some(&tag), Share::Reserved, &cwd, &arg_refs(&args)) {
            Ok(o) => o,
            Err(RunError::Cancelled) => return Err(STALE.into()),
            Err(e) => return Err(e.reason()),
        };
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        Ok(parse_side_log(&String::from_utf8_lossy(&out)).into_iter().map(SideCommitView::from).collect())
    }

    /// Branch names across `repos` for the picker: how many have each, most first, then by
    /// name. One `for-each-ref` per repository, untagged; a repository git cannot read adds
    /// nothing. A name is remote when its ref is under `refs/remotes/`; a local branch and a
    /// remote-tracking one of the same name are two entries.
    pub fn branches(&self, repos: &[String]) -> Result<Vec<BranchNameView>, String> {
        let cwds: Vec<PathBuf> = repos.iter().map(|r| self.cwd_of(r)).collect::<Result<_, _>>()?;
        let args = branch_refs_args();
        let lists = par_map(&cwds, |cwd| self.runner.run(None, cwd, &arg_refs(&args)).map(|b| parse_branch_refs(&String::from_utf8_lossy(&b))).unwrap_or_default());
        // Local and remote-tracking names are kept apart: a local `origin/x` is not the remote one.
        let mut counts: HashMap<BranchRef, usize> = HashMap::new();
        for b in lists.into_iter().flatten() {
            *counts.entry(b).or_default() += 1;
        }
        let mut v: Vec<BranchNameView> = counts.into_iter().map(|(b, count)| BranchNameView { name: b.name, count, remote: b.remote }).collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)).then_with(|| a.remote.cmp(&b.remote)));
        Ok(v)
    }
}

// ---------------------------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------------------------

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn gitlog_compare(
    query_id: u64,
    repos: Vec<String>,
    left: String,
    left_remote: bool,
    right: String,
    right_remote: bool,
    window: tauri::Window,
    state: tauri::State<'_, GitLogState>,
) -> Result<CompareView, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    let pair = Pair { left, right, left_remote, right_remote };
    crate::commands::blocking(move || gl.compare(&label, query_id, &repos, &pair)).await?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn gitlog_compare_files(
    query_id: u64,
    repo: String,
    left_sha: String,
    right_sha: String,
    base: String,
    window: tauri::Window,
    state: tauri::State<'_, GitLogState>,
) -> Result<CompareFiles, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || gl.compare_files(&label, query_id, &repo, &left_sha, &right_sha, &base)).await?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn gitlog_compare_commits(
    query_id: u64,
    repo: String,
    side: String,
    left_sha: String,
    right_sha: String,
    max: usize,
    window: tauri::Window,
    state: tauri::State<'_, GitLogState>,
) -> Result<Vec<SideCommitView>, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || gl.compare_commits(&label, query_id, &repo, &side, &left_sha, &right_sha, max)).await?
}

#[tauri::command]
pub async fn gitlog_branches(repos: Vec<String>, state: tauri::State<'_, GitLogState>) -> Result<Vec<BranchNameView>, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.branches(&repos)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_serialize_camel_case() {
        let row = RepoCompareRow {
            repo: "/r/.git".into(),
            result: RepoCompareView::Differs { left_sha: "a".into(), right_sha: "b".into(), base: "c".into(), left: 2, right: 1, same_left: 1, same_right: 1 },
        };
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            serde_json::json!({"repo": "/r/.git", "kind": "differs", "leftSha": "a", "rightSha": "b", "base": "c", "left": 2, "right": 1, "sameLeft": 1, "sameRight": 1})
        );
        let row = RepoCompareRow { repo: "/r/.git".into(), result: RepoCompareView::Error { reason: "bad".into() } };
        assert_eq!(serde_json::to_value(&row).unwrap(), serde_json::json!({"repo": "/r/.git", "kind": "error", "reason": "bad"}));
        let row = RepoCompareRow { repo: "/r/.git".into(), result: RepoCompareView::Nobase };
        assert_eq!(serde_json::to_value(&row).unwrap(), serde_json::json!({"repo": "/r/.git", "kind": "nobase"}));
        let v = serde_json::to_value(CompareView { query_id: 3, rows: vec![] }).unwrap();
        assert_eq!(v, serde_json::json!({"queryId": 3, "rows": []}));
    }
}
