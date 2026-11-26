//! The Git log view's backend: a pooled, cancellable `git` runner and the `gitlog_*` commands.
//!
//! Read-only. Every child is a `git` read (`log`, `show`, `cat-file`, `rev-parse`, `config
//! --get`, `remote get-url`, `worktree list`, `rev-list`) with optional locks off, so not even
//! the index is refreshed. Nothing here writes a file.
//!
//! - **Pool.** At most `PARALLEL` (16) children run at once, across every command.
//! - **Cancellation.** A child started for a query is tagged with its window and `query_id`.
//!   A new `query_id` for that window kills (and reaps) every child of the previous one; a call
//!   whose query was superseded returns `Err(STALE)`, never its rows.
//! - **Paging.** Each window's latest query keeps its `RepoProgress` per repository and its
//!   "now". A page refills every repository that is not exhausted and has fewer than a page
//!   pending, then takes the rows at or above the horizon (`take_ready`), for at most
//!   `MAX_ROUNDS` rounds.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use claudegate_core::gitlog::{
    ahead_behind, discover, heads_for, is_sha, is_valid_ref, log_args, par_map, parse_changes, parse_log, repo_searched, take_ready, web_url,
    BranchUse, ChangedFile, Commit, FileStatus, Filter, GitRun, Repo, RepoProgress, DEFAULT_SCAN_DEPTH, PARALLEL,
};
use serde::{Deserialize, Serialize};

/// Records per repository per `git log` call, and the rows one page returns at most.
pub const PAGE_SIZE: usize = 200;
/// Refill rounds per page at most (Polylog's MAX_ROUNDS): a repository left empty holds the
/// horizon, so the loop needs a bound.
pub const MAX_ROUNDS: usize = 64;
/// A file side larger than this is not loaded: `FileView::TooLarge`.
pub const MAX_FILE: u64 = 2 * 1024 * 1024;
/// The error a superseded query's call returns. The UI also gates on `query_id`.
pub const STALE: &str = "stale query";

// ---------------------------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// The query was superseded; the child (if any) was killed and reaped.
    Cancelled,
    /// git could not be started (or its folder is gone).
    Spawn(String),
    /// git exited non-zero: its first stderr line.
    Failed(String),
}

impl RunError {
    pub fn reason(&self) -> String {
        match self {
            RunError::Cancelled => STALE.into(),
            RunError::Spawn(s) | RunError::Failed(s) => s.clone(),
        }
    }
}

/// A query's identity: the window it belongs to and its id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    pub window: String,
    pub query: u64,
}

type Slot = Arc<Mutex<Option<Child>>>;

#[derive(Default)]
struct Registry {
    /// The latest query per window.
    current: HashMap<String, u64>,
    /// The live children of each query, by pid.
    children: HashMap<Tag, Vec<(u32, Slot)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Kills and reaps the child in `slot`, if it is still there.
fn kill(slot: &Slot) {
    if let Some(mut c) = lock(slot).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// Runs `git` with at most `PARALLEL` children alive, each tracked by its query.
pub struct Runner {
    git: PathBuf,
    running: Mutex<usize>,
    freed: Condvar,
    reg: Mutex<Registry>,
}

/// One pool slot, given back on drop.
struct Permit<'a>(&'a Runner);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *lock(&self.0.running) -= 1;
        self.0.freed.notify_one();
    }
}

/// git as the app finds python3 and editors: each PATH entry, then the usual install folders
/// (a GUI app's PATH is often bare). Bare `git` when none is found.
pub fn resolve_git() -> PathBuf {
    let home = dirs::home_dir();
    let mut extra = crate::editor::fallback_dirs(home.as_deref());
    if cfg!(windows) {
        extra.push(PathBuf::from(r"C:\Program Files\Git\cmd"));
    }
    let path = std::env::var_os("PATH");
    crate::editor::resolve_program("git", path.as_deref(), &extra, &crate::editor::is_executable).unwrap_or_else(|| PathBuf::from("git"))
}

impl Runner {
    pub fn new(git: PathBuf) -> Self {
        Runner { git, running: Mutex::new(0), freed: Condvar::new(), reg: Mutex::new(Registry::default()) }
    }

    /// Makes `query` the window's latest. Every child of the window's other queries is killed
    /// and reaped before this returns. Query ids only rise within a window (the UI seeds them
    /// from `Date.now()`, so a reloaded webview keeps rising): a query older than the current
    /// one arriving late returns false and kills nothing.
    pub fn set_current(&self, window: &str, query: u64) -> bool {
        let doomed: Vec<Slot> = {
            let mut reg = lock(&self.reg);
            if reg.current.get(window).is_some_and(|c| *c > query) {
                return false;
            }
            reg.current.insert(window.to_string(), query);
            let old: Vec<Tag> = reg.children.keys().filter(|t| t.window == window && t.query != query).cloned().collect();
            old.iter().flat_map(|t| reg.children.remove(t).unwrap_or_default()).map(|(_, s)| s).collect()
        };
        for s in &doomed {
            kill(s);
        }
        // Waiters for a pool slot re-check whether they were superseded.
        self.freed.notify_all();
        true
    }

    /// A closed window: no query of it is current any more, and its children are killed.
    pub fn forget_window(&self, window: &str) {
        let doomed: Vec<Slot> = {
            let mut reg = lock(&self.reg);
            reg.current.remove(window);
            let old: Vec<Tag> = reg.children.keys().filter(|t| t.window == window).cloned().collect();
            old.iter().flat_map(|t| reg.children.remove(t).unwrap_or_default()).map(|(_, s)| s).collect()
        };
        for s in &doomed {
            kill(s);
        }
        self.freed.notify_all();
    }

    pub fn is_current(&self, tag: &Tag) -> bool {
        lock(&self.reg).current.get(&tag.window) == Some(&tag.query)
    }

    /// The pids of a query's live children.
    pub fn pids(&self, tag: &Tag) -> Vec<u32> {
        lock(&self.reg).children.get(tag).map(|v| v.iter().map(|(p, _)| *p).collect()).unwrap_or_default()
    }

    fn acquire(&self, tag: Option<&Tag>) -> Result<Permit<'_>, RunError> {
        let mut n = lock(&self.running);
        loop {
            if tag.is_some_and(|t| !self.is_current(t)) {
                return Err(RunError::Cancelled);
            }
            if *n < PARALLEL {
                *n += 1;
                return Ok(Permit(self));
            }
            n = self.freed.wait_timeout(n, Duration::from_millis(50)).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    /// `git <args>` in `cwd`: its stdout bytes. With a `tag`, the child belongs to that query
    /// and is killed when the query is superseded.
    pub fn run(&self, tag: Option<&Tag>, cwd: &Path, args: &[&str]) -> Result<Vec<u8>, RunError> {
        if !cwd.is_dir() {
            return Err(RunError::Spawn(format!("The folder {} is missing", cwd.display())));
        }
        let _permit = self.acquire(tag)?;
        let mut cmd = Command::new(&self.git);
        cmd.args(["--no-pager", "-c", "color.ui=never", "-c", "core.quotepath=off"]).args(args).current_dir(cwd);
        // An inherited repository override (GIT_DIR and friends) would point every call at one
        // repository; none of git's own variables is passed on.
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("GIT_") {
                cmd.env_remove(&k);
            }
        }
        cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0");
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| RunError::Spawn(format!("Couldn't run git: {e}")))?;
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let pid = child.id();
        let slot: Slot = Arc::new(Mutex::new(Some(child)));
        if let Some(t) = tag {
            let mut reg = lock(&self.reg);
            if reg.current.get(&t.window) != Some(&t.query) {
                drop(reg);
                kill(&slot);
                return Err(RunError::Cancelled);
            }
            reg.children.entry(t.clone()).or_default().push((pid, slot.clone()));
        }
        // stderr on its own thread, so a chatty git can never block on a full pipe.
        let err_reader = std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(mut e) = stderr {
                let _ = e.read_to_end(&mut b);
            }
            b
        });
        let mut out = Vec::new();
        if let Some(mut o) = stdout {
            let _ = o.read_to_end(&mut out);
        }
        let err = err_reader.join().unwrap_or_default();
        let status = lock(&slot).take().and_then(|mut c| c.wait().ok());
        if let Some(t) = tag {
            let mut reg = lock(&self.reg);
            if let Some(v) = reg.children.get_mut(t) {
                v.retain(|(_, s)| !Arc::ptr_eq(s, &slot));
                if v.is_empty() {
                    reg.children.remove(t);
                }
            }
        }
        match status {
            // Killed by `set_current`, which reaped it.
            None => Err(RunError::Cancelled),
            Some(s) if s.success() => Ok(out),
            Some(s) => {
                let msg = String::from_utf8_lossy(&err);
                let line = msg.lines().map(str::trim).find(|l| !l.is_empty()).map(String::from);
                Err(RunError::Failed(line.unwrap_or_else(|| format!("git exited with {s}"))))
            }
        }
    }

    fn text(&self, tag: Option<&Tag>, cwd: &Path, args: &[&str]) -> Result<String, RunError> {
        self.run(tag, cwd, args).map(|b| String::from_utf8_lossy(&b).into_owned())
    }
}

/// Discovery's calls go through the same pool, untagged: a refresh is never superseded.
impl GitRun for Runner {
    fn run(&self, cwd: &Path, args: &[&str]) -> Option<String> {
        self.text(None, cwd, args).ok()
    }
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutView {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoView {
    /// The git common dir: the key for ticks, profiles and every other `gitlog_*` call.
    pub id: String,
    pub name: String,
    /// Index into the UI's 12-colour palette.
    pub color: u8,
    pub checkouts: Vec<CheckoutView>,
    /// Commits the main checkout's HEAD is ahead of / behind its upstream; `None` without one.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}

/// The filter as the UI sends it. `within` is "the last N seconds", resolved against the
/// query's "now" once, at its first page, so paging never drifts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterView {
    pub search: Option<String>,
    pub authors: Vec<String>,
    pub mine: bool,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub within: Option<i64>,
}

impl FilterView {
    pub fn resolve(&self, now: i64) -> Filter {
        let blank = |s: &Option<String>| s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        Filter {
            search: blank(&self.search),
            authors: self.authors.clone(),
            mine: self.mine,
            path: blank(&self.path),
            branch: blank(&self.branch),
            since: self.since.or(self.within.map(|w| now - w.max(0))),
            until: self.until,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowView {
    pub repo: String,
    pub sha: String,
    /// Committer time, epoch seconds.
    pub time: i64,
    pub author: String,
    pub email: String,
    pub subject: String,
    pub parents: Vec<String>,
    /// The head the commit was reached from: `HEAD` (the main checkout), `refs/heads/<b>`
    /// (a worktree's branch), the branch filter as typed, or a sha (a detached worktree).
    pub source: String,
}

impl From<Commit> for RowView {
    fn from(c: Commit) -> Self {
        RowView { repo: c.repo, sha: c.sha, time: c.time, author: c.author, email: c.email, subject: c.subject, parents: c.parents, source: c.source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub repo: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchUseView {
    pub branch: String,
    /// Repositories that have the branch.
    pub found: usize,
    /// Repositories that fell back to their current HEADs.
    pub fallback: usize,
}

impl From<BranchUse> for BranchUseView {
    fn from(b: BranchUse) -> Self {
        BranchUseView { branch: b.branch, found: b.found, fallback: b.fallback }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageView {
    pub query_id: u64,
    /// This page's rows only, newest first; Load more appends the next call's.
    pub rows: Vec<RowView>,
    /// Every repository that failed in this query so far (cumulative: replace, don't append).
    pub failures: Vec<Failure>,
    /// With a branch filter: how many searched repositories have it.
    pub branch_use: Option<BranchUseView>,
    /// Nothing more to load.
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFileView {
    pub path: String,
    /// `added`, `modified`, `deleted`, `renamed`, `copied` or `typeChanged`.
    pub status: &'static str,
    /// The old path of a rename or copy.
    pub from: Option<String>,
    /// `None` for a binary file.
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

impl From<ChangedFile> for ChangedFileView {
    fn from(c: ChangedFile) -> Self {
        let (status, from) = match c.status {
            FileStatus::Added => ("added", None),
            FileStatus::Modified => ("modified", None),
            FileStatus::Deleted => ("deleted", None),
            FileStatus::Renamed { from } => ("renamed", Some(from)),
            FileStatus::Copied { from } => ("copied", Some(from)),
            FileStatus::TypeChanged => ("typeChanged", None),
        };
        ChangedFileView { path: c.path, status, from, added: c.added, removed: c.removed }
    }
}

/// One side of a changed file. Serialized as `{kind: "text", value}`, `{kind: "binary"}`,
/// `{kind: "tooLarge", value: <bytes>}` or `{kind: "missing"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum FileView {
    Text(String),
    Binary,
    TooLarge(u64),
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Before,
    After,
}

/// Binary: a NUL anywhere, or not UTF-8.
pub fn file_view_of(bytes: Vec<u8>) -> FileView {
    if bytes.contains(&0) {
        return FileView::Binary;
    }
    match String::from_utf8(bytes) {
        Ok(s) => FileView::Text(s),
        Err(_) => FileView::Binary,
    }
}

// ---------------------------------------------------------------------------------------------
// Query state
// ---------------------------------------------------------------------------------------------

/// One searched repository of a query.
struct RepoQuery {
    id: String,
    cwd: PathBuf,
    heads: Vec<String>,
    me: Option<String>,
}

struct QueryState {
    query: u64,
    /// `within` already resolved against the query's "now", fixed at its first page.
    filter: Filter,
    repos: Vec<RepoQuery>,
    progress: BTreeMap<String, RepoProgress>,
    failures: Vec<Failure>,
    branch_use: Option<BranchUse>,
}

/// What discovery ran over; a change means the cached list is stale.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RepoKey {
    roots: Vec<PathBuf>,
    scan_depth: usize,
    exclude: Vec<String>,
}

/// Everything behind the `gitlog_*` commands, without Tauri.
pub struct GitLog {
    pub runner: Runner,
    page_size: usize,
    repos: Mutex<Option<(Option<RepoKey>, Arc<Vec<Repo>>)>>,
    discovering: Mutex<()>,
    queries: Mutex<HashMap<String, Arc<Mutex<QueryState>>>>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `git log` records in `out`: one RS each. Counted on the raw output, not on parsed commits,
/// so a record the parser drops still advances `--skip`.
fn count_records(out: &[u8]) -> usize {
    out.iter().filter(|b| **b == 0x1e).count()
}

/// The refs that log one repository's current checkouts: `HEAD` for the main one (the logs run
/// there), each worktree's branch by name, and a detached worktree's commit. Names resolve
/// when the query runs, so a commit made since discovery still shows.
fn current_heads(r: &Repo) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for c in &r.checkouts {
        let h = if c.is_main {
            "HEAD".to_string()
        } else {
            match &c.branch {
                Some(b) => format!("refs/heads/{b}"),
                None => c.head.clone(),
            }
        };
        if !v.contains(&h) {
            v.push(h);
        }
    }
    v
}

fn main_path(r: &Repo) -> PathBuf {
    let c = r.checkouts.iter().find(|c| c.is_main).or(r.checkouts.first());
    PathBuf::from(c.map(|c| c.path.as_str()).unwrap_or(&r.id))
}

/// A path as git reports it, relative to the repository root. Refused when `<rev>:<path>`
/// would read it relative to the working directory instead (`./`, `../`), or it leaves the
/// repository.
fn tree_path_ok(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.contains('\0') && !p.split('/').any(|s| s == "." || s == "..")
}

impl Default for GitLog {
    fn default() -> Self {
        GitLog::new(Runner::new(resolve_git()))
    }
}

impl GitLog {
    pub fn new(runner: Runner) -> Self {
        GitLog { runner, page_size: PAGE_SIZE, repos: Mutex::new(None), discovering: Mutex::new(()), queries: Mutex::new(HashMap::new()) }
    }

    /// A smaller page, for tests.
    pub fn with_page_size(mut self, n: usize) -> Self {
        self.page_size = n.max(1);
        self
    }

    /// Replaces the repository list without discovery (tests, and a launch that already knows).
    pub fn set_repos(&self, repos: Vec<Repo>) {
        *lock(&self.repos) = Some((None, Arc::new(repos)));
    }

    /// The repositories under `roots`, discovered once and cached until the roots, depth or
    /// exclude change, or `refresh`.
    pub fn repos_for(&self, roots: &[PathBuf], scan_depth: usize, exclude: &[String], refresh: bool) -> Arc<Vec<Repo>> {
        let key = RepoKey { roots: roots.to_vec(), scan_depth, exclude: exclude.to_vec() };
        let _one_at_a_time = lock(&self.discovering);
        if !refresh {
            if let Some((Some(k), list)) = lock(&self.repos).as_ref() {
                if *k == key {
                    return list.clone();
                }
            }
        }
        let list = Arc::new(discover(roots, scan_depth, exclude, &self.runner));
        *lock(&self.repos) = Some((Some(key), list.clone()));
        list
    }

    /// `repos_for` with each repository's ahead/behind (fresh on every call).
    pub fn repo_views(&self, roots: &[PathBuf], scan_depth: usize, exclude: &[String], refresh: bool) -> Vec<RepoView> {
        let list = self.repos_for(roots, scan_depth, exclude, refresh);
        let ab = par_map(&list, |r| ahead_behind(r, &self.runner));
        list.iter()
            .zip(ab)
            .map(|(r, ab)| RepoView {
                id: r.id.clone(),
                name: r.name.clone(),
                color: r.color,
                checkouts: r
                    .checkouts
                    .iter()
                    .map(|c| CheckoutView { path: c.path.clone(), head: c.head.clone(), branch: c.branch.clone(), is_main: c.is_main })
                    .collect(),
                ahead: ab.map(|x| x.0),
                behind: ab.map(|x| x.1),
            })
            .collect()
    }

    /// Whether a repository list was ever loaded (an empty one counts).
    pub fn loaded(&self) -> bool {
        lock(&self.repos).is_some()
    }

    /// A window closed: drop its query and kill its children.
    pub fn forget_window(&self, window: &str) {
        self.runner.forget_window(window);
        lock(&self.queries).remove(window);
    }

    fn cached(&self) -> Arc<Vec<Repo>> {
        lock(&self.repos).as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
    }

    fn cwd_of(&self, repo: &str) -> Result<PathBuf, String> {
        self.cached().iter().find(|r| r.id == repo).map(main_path).ok_or_else(|| "Unknown repository; refresh the list".to_string())
    }

    /// One page of the merged log. `more: false` starts query `query_id` (superseding the
    /// window's previous one, whose children are killed); `more: true` continues it.
    pub fn page(&self, window: &str, query_id: u64, repos: &[String], filter: &FilterView, more: bool) -> Result<PageView, String> {
        let tag = Tag { window: window.to_string(), query: query_id };
        let state = if more {
            let existing = lock(&self.queries).get(window).cloned();
            match existing {
                Some(s) if lock(&s).query == query_id && self.runner.is_current(&tag) => s,
                _ => return Err(STALE.into()),
            }
        } else {
            if !self.runner.set_current(window, query_id) {
                return Err(STALE.into());
            }
            let st = self.start(&tag, repos, filter)?;
            Arc::new(Mutex::new(st))
        };
        let view = {
            let mut st = lock(&state);
            self.fill(&tag, &mut st)?
        };
        // Checked and stored under one lock, so a newer query's state is never overwritten.
        let mut q = lock(&self.queries);
        if !self.runner.is_current(&tag) {
            return Err(STALE.into());
        }
        if !more {
            q.insert(window.to_string(), state);
        }
        Ok(view)
    }

    /// A new query: each selected repository's `user.email` (for Me), whether it has the
    /// branch, and the heads to log.
    fn start(&self, tag: &Tag, ids: &[String], fv: &FilterView) -> Result<QueryState, String> {
        let now = now_secs();
        let filter = fv.resolve(now);
        let all = self.cached();
        let selected: Vec<&Repo> = all.iter().filter(|r| ids.contains(&r.id)).collect();
        let branch = filter.branch.clone().filter(|b| is_valid_ref(b));
        type Setup = Result<Option<(RepoQuery, bool)>, RunError>;
        let setups: Vec<Setup> = par_map(&selected, |r| {
            let cwd = main_path(r);
            let me = if filter.mine {
                match self.runner.text(Some(tag), &cwd, &["config", "--get", "user.email"]) {
                    Ok(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
                    Err(RunError::Cancelled) => return Err(RunError::Cancelled),
                    Err(_) => None,
                }
            } else {
                None
            };
            if !repo_searched(&filter, me.as_deref()) {
                return Ok(None);
            }
            let has = match &branch {
                Some(b) => match self.runner.run(Some(tag), &cwd, &["rev-parse", "--verify", "--quiet", &format!("{b}^{{commit}}")]) {
                    Ok(_) => true,
                    Err(RunError::Cancelled) => return Err(RunError::Cancelled),
                    Err(_) => false,
                },
                None => false,
            };
            let heads = heads_for(branch.as_deref(), has, &current_heads(r));
            Ok(Some((RepoQuery { id: r.id.clone(), cwd, heads, me }, has)))
        });
        let mut rq = Vec::new();
        let mut has = Vec::new();
        for s in setups {
            match s {
                Err(_) => return Err(STALE.into()),
                Ok(None) => {}
                Ok(Some((q, h))) => {
                    rq.push(q);
                    has.push(h);
                }
            }
        }
        let branch_use = branch.as_deref().map(|b| BranchUse::tally(b, has));
        let progress = rq.iter().map(|q| (q.id.clone(), RepoProgress::default())).collect();
        Ok(QueryState { query: tag.query, filter, repos: rq, progress, failures: Vec::new(), branch_use })
    }

    /// Refill and take ready rows until a page is full, everything is drained, or
    /// `MAX_ROUNDS`. Rows past the page go back to their repository's pending, in order: they
    /// all sit at or above the horizon, so the next page returns them first.
    fn fill(&self, tag: &Tag, st: &mut QueryState) -> Result<PageView, String> {
        let page = self.page_size;
        let mut rows: Vec<Commit> = Vec::new();
        for _ in 0..MAX_ROUNDS {
            let need: Vec<&RepoQuery> = st
                .repos
                .iter()
                .filter(|q| st.progress.get(&q.id).is_some_and(|p| !p.exhausted && p.pending.len() < page))
                .collect();
            if !need.is_empty() {
                let filter = &st.filter;
                let progress = &st.progress;
                let got = par_map(&need, |q| {
                    let skip = progress.get(&q.id).map_or(0, |p| p.fetched);
                    let args = log_args(filter, &q.heads, q.me.as_deref(), page, skip);
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    self.runner.run(Some(tag), &q.cwd, &args)
                });
                let ids: Vec<String> = need.iter().map(|q| q.id.clone()).collect();
                for (id, res) in ids.into_iter().zip(got) {
                    match res {
                        Err(RunError::Cancelled) => return Err(STALE.into()),
                        Err(e) => {
                            st.progress.remove(&id);
                            st.failures.push(Failure { repo: id, reason: e.reason() });
                        }
                        Ok(out) => {
                            let n = count_records(&out);
                            let Some(p) = st.progress.get_mut(&id) else { continue };
                            p.fetched += n;
                            p.exhausted = n < page;
                            p.pending.extend(parse_log(&String::from_utf8_lossy(&out)).into_iter().map(|mut c| {
                                c.repo = id.clone();
                                c
                            }));
                        }
                    }
                }
            }
            rows.extend(take_ready(&mut st.progress));
            let drained = st.progress.values().all(|p| p.exhausted && p.pending.is_empty());
            if rows.len() >= page || drained {
                break;
            }
        }
        if rows.len() > page {
            for c in rows.split_off(page) {
                if let Some(p) = st.progress.get_mut(&c.repo) {
                    p.pending.push(c);
                }
            }
        }
        let done = st.progress.values().all(|p| p.exhausted && p.pending.is_empty());
        Ok(PageView {
            query_id: st.query,
            rows: rows.into_iter().map(RowView::from).collect(),
            failures: st.failures.clone(),
            branch_use: st.branch_use.clone().map(BranchUseView::from),
            done,
        })
    }

    fn show_args<'a>(kind: &'a str, sha: &'a str) -> [&'a str; 6] {
        ["show", "-M", "--diff-merges=first-parent", "--format=", kind, sha]
    }

    /// The files `sha` changed against its first parent. Two calls: git drops `--numstat`
    /// when it is combined with `--name-status`.
    pub fn changes(&self, repo: &str, sha: &str) -> Result<Vec<ChangedFileView>, String> {
        if !is_sha(sha) {
            return Err("Not a commit id".into());
        }
        let cwd = self.cwd_of(repo)?;
        let (ns, num) = std::thread::scope(|s| {
            let ns = s.spawn(|| self.runner.text(None, &cwd, &Self::show_args("--name-status", sha)));
            let num = self.runner.text(None, &cwd, &Self::show_args("--numstat", sha));
            (ns.join().unwrap_or(Err(RunError::Spawn("git reader panicked".into()))), num)
        });
        let (ns, num) = (ns.map_err(|e| e.reason())?, num.map_err(|e| e.reason())?);
        Ok(parse_changes(&ns, &num).into_iter().map(ChangedFileView::from).collect())
    }

    /// One side of a changed file. Before is `parents[0]:<renamed-from or path>` (Missing for a
    /// root commit or an added file); after is `sha:<path>` (Missing for a deleted file).
    pub fn file(&self, repo: &str, sha: &str, path: &str, side: Side) -> Result<FileView, String> {
        if !is_sha(sha) {
            return Err("Not a commit id".into());
        }
        if !tree_path_ok(path) {
            return Err("Not a path in the repository".into());
        }
        let cwd = self.cwd_of(repo)?;
        let spec = match side {
            Side::After => format!("{sha}:{path}"),
            Side::Before => {
                let line = self.runner.text(None, &cwd, &["rev-list", "--parents", "-n", "1", sha]).map_err(|e| e.reason())?;
                let Some(p0) = line.split_whitespace().nth(1).map(String::from) else { return Ok(FileView::Missing) };
                let ns = self.runner.text(None, &cwd, &Self::show_args("--name-status", sha)).map_err(|e| e.reason())?;
                let mut name = path.to_string();
                for c in parse_changes(&ns, "") {
                    if c.path != path {
                        continue;
                    }
                    match c.status {
                        FileStatus::Added => return Ok(FileView::Missing),
                        FileStatus::Renamed { from } | FileStatus::Copied { from } => name = from,
                        _ => {}
                    }
                    break;
                }
                if !tree_path_ok(&name) {
                    return Ok(FileView::Missing);
                }
                format!("{p0}:{name}")
            }
        };
        // No such object at that side: missing. The size first, so a huge blob is never read.
        let size = match self.runner.text(None, &cwd, &["cat-file", "-s", &spec]) {
            Ok(s) => s,
            Err(RunError::Failed(_)) => return Ok(FileView::Missing),
            Err(e) => return Err(e.reason()),
        };
        let size: u64 = size.trim().parse().map_err(|_| "Unexpected output from git cat-file".to_string())?;
        if size > MAX_FILE {
            return Ok(FileView::TooLarge(size));
        }
        let bytes = self.runner.run(None, &cwd, &["cat-file", "blob", &spec]).map_err(|e| e.reason())?;
        Ok(file_view_of(bytes))
    }

    /// The commit's page on its remote host (`origin`, else the first remote), if known.
    pub fn web_url(&self, repo: &str, sha: &str) -> Result<Option<String>, String> {
        let cwd = self.cwd_of(repo)?;
        let remote = match self.runner.text(None, &cwd, &["remote", "get-url", "origin"]) {
            Ok(u) => Some(u),
            Err(_) => {
                let names = self.runner.text(None, &cwd, &["remote"]).unwrap_or_default();
                match names.lines().map(str::trim).find(|n| !n.is_empty()) {
                    Some(n) => self.runner.text(None, &cwd, &["remote", "get-url", n]).ok(),
                    None => None,
                }
            }
        };
        Ok(remote.and_then(|u| web_url(u.trim(), sha)))
    }

    /// The full commit message.
    pub fn message(&self, repo: &str, sha: &str) -> Result<String, String> {
        if !is_sha(sha) {
            return Err("Not a commit id".into());
        }
        let cwd = self.cwd_of(repo)?;
        let m = self.runner.text(None, &cwd, &["show", "-s", "--format=%B", sha]).map_err(|e| e.reason())?;
        Ok(m.trim_end().to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------------------------

/// What discovery runs over: the workspace roots, each registered root nested inside one of
/// them, the walk depth and the repository excludes (`gitLog.scanDepth`, `gitLog.excludeRepos`).
/// Read without creating or migrating anything: with no `workspace` key yet, the rail's listed
/// roots stand in for it.
pub fn workspace_scan(config_path: &Path, cg_home: &Path) -> (Vec<PathBuf>, usize, Vec<String>) {
    let cfg: serde_json::Value = std::fs::read_to_string(config_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let ws: Vec<String> = match cfg.get("workspace").and_then(|w| w.as_array()) {
        Some(a) => a.iter().filter_map(|r| r.as_str()).map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect(),
        None => crate::config::listed_root_strings(cg_home).unwrap_or_default(),
    };
    let mut roots: Vec<PathBuf> = Vec::new();
    for r in ws {
        let r = claudegate_core::paths::resolve_root(Path::new(&r));
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    let ws_roots = roots.clone();
    for r in claudegate_core::roots::read_roots(cg_home) {
        let r = claudegate_core::paths::resolve_root(&r);
        if ws_roots.iter().any(|w| r.starts_with(w)) && !roots.contains(&r) {
            roots.push(r);
        }
    }
    let gl = cfg.get("gitLog");
    let depth = gl.and_then(|g| g.get("scanDepth")).and_then(|d| d.as_u64()).map(|d| d.min(8) as usize).unwrap_or(DEFAULT_SCAN_DEPTH);
    let exclude = gl
        .and_then(|g| g.get("excludeRepos"))
        .and_then(|e| e.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).map(String::from).collect())
        .unwrap_or_default();
    (roots, depth, exclude)
}

// ---------------------------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------------------------

/// The app's one `GitLog`, shared by every window.
#[derive(Clone, Default)]
pub struct GitLogState(pub Arc<GitLog>);

/// The repositories behind the workspace, with ahead/behind. Discovery is cached until the
/// workspace changes or `refresh`.
#[tauri::command]
pub async fn gitlog_repos(refresh: Option<bool>, state: tauri::State<'_, GitLogState>) -> Result<Vec<RepoView>, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || {
        let (roots, depth, exclude) = workspace_scan(&crate::config::config_path(), &claudegate_core::paths::claudegate_home());
        gl.repo_views(&roots, depth, &exclude, refresh.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn gitlog_page(
    query_id: u64,
    repos: Vec<String>,
    filter: FilterView,
    more: bool,
    window: tauri::Window,
    state: tauri::State<'_, GitLogState>,
) -> Result<PageView, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || {
        // A page asked for before the list was ever loaded: discover first.
        if !gl.loaded() {
            let (roots, depth, exclude) = workspace_scan(&crate::config::config_path(), &claudegate_core::paths::claudegate_home());
            gl.repos_for(&roots, depth, &exclude, false);
        }
        gl.page(&label, query_id, &repos, &filter, more)
    })
    .await?
}

#[tauri::command]
pub async fn gitlog_changes(repo: String, sha: String, state: tauri::State<'_, GitLogState>) -> Result<Vec<ChangedFileView>, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.changes(&repo, &sha)).await?
}

#[tauri::command]
pub async fn gitlog_file(repo: String, sha: String, path: String, side: Side, state: tauri::State<'_, GitLogState>) -> Result<FileView, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.file(&repo, &sha, &path, side)).await?
}

#[tauri::command]
pub async fn gitlog_web_url(repo: String, sha: String, state: tauri::State<'_, GitLogState>) -> Result<Option<String>, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.web_url(&repo, &sha)).await?
}

#[tauri::command]
pub async fn gitlog_message(repo: String, sha: String, state: tauri::State<'_, GitLogState>) -> Result<String, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.message(&repo, &sha)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_sniff() {
        assert_eq!(file_view_of(b"plain\n".to_vec()), FileView::Text("plain\n".into()));
        assert_eq!(file_view_of(vec![b'a', 0, b'b']), FileView::Binary);
        assert_eq!(file_view_of(vec![0xff, 0xfe, b'a']), FileView::Binary);
        let mut late_nul = vec![b'a'; 64 * 1024];
        late_nul[60 * 1024] = 0;
        assert_eq!(file_view_of(late_nul), FileView::Binary, "a NUL anywhere is binary");
    }

    #[test]
    fn file_view_serializes_tagged() {
        assert_eq!(serde_json::to_value(FileView::TooLarge(5)).unwrap(), serde_json::json!({"kind": "tooLarge", "value": 5}));
        assert_eq!(serde_json::to_value(FileView::Missing).unwrap(), serde_json::json!({"kind": "missing"}));
    }

    #[test]
    fn within_resolves_against_now() {
        let f = FilterView { within: Some(86_400), path: Some("  ".into()), ..Default::default() }.resolve(1_000_000);
        assert_eq!(f.since, Some(1_000_000 - 86_400));
        assert_eq!(f.path, None);
    }

    #[test]
    fn tree_paths() {
        assert!(tree_path_ok("a/b.txt"));
        assert!(!tree_path_ok("./a"));
        assert!(!tree_path_ok("a/../b"));
        assert!(!tree_path_ok("/etc/passwd"));
    }
}
