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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use claudegate_core::gitlog::{
    ahead_behind, blame_args, count_records, discover, heads_for, history_args, history_paths_args, is_sha, is_valid_ref, log_args, par_map, parse_blame_porcelain, parse_changes, parse_history, parse_history_paths, parse_log,
    repo_searched, take_ready, web_url, BranchUse, ChangedFile, Commit, FileStatus, Filter, GitRun, HistoryFile, Repo, RepoProgress, DEFAULT_SCAN_DEPTH, PARALLEL,
};
use serde::{Deserialize, Serialize};

/// Records per repository per `git log` call, and the rows one page returns at most.
pub const PAGE_SIZE: usize = 200;
/// Refill rounds per page at most (Polylog's MAX_ROUNDS): a repository left empty holds the
/// horizon, so the loop needs a bound.
pub const MAX_ROUNDS: usize = 64;
/// Pool slots only untagged calls (changes, file, message, discovery) may use.
pub const RESERVED: usize = 2;
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

/// The cancellation channel of one feature of a window (`<window>#<name>`): its queries supersede
/// only each other, so a new Combined Changes selection never kills the log's paging.
/// `Runner::forget_window` drops every channel of the window.
pub fn channel(window: &str, name: &str) -> String {
    format!("{window}#{name}")
}

/// A query's identity: the window (or a channel of it, see [`channel`]) it belongs to and its id.
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
    /// The git binary; resolved on the first run, so nothing is probed at app start.
    git: OnceLock<PathBuf>,
    running: Mutex<usize>,
    freed: Condvar,
    reg: Mutex<Registry>,
}

/// One pool slot, given back on drop.
pub(crate) struct Permit<'a>(&'a Runner);

/// Which part of the pool a call waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Share {
    /// Background reads (paging, a Combined Changes tree): `PARALLEL - RESERVED` slots at most.
    Paging,
    /// A click (a changed file, a message, a Combined Changes left side): the whole pool, so it
    /// never waits behind paging.
    Reserved,
}

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
        Runner { git: OnceLock::from(git), running: Mutex::new(0), freed: Condvar::new(), reg: Mutex::new(Registry::default()) }
    }

    /// A runner that looks for git (`resolve_git`) the first time it runs one.
    pub fn lazy() -> Self {
        Runner { git: OnceLock::new(), running: Mutex::new(0), freed: Condvar::new(), reg: Mutex::new(Registry::default()) }
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

    /// A closed window: no query of it or of any of its channels (`<window>#…`) is current any
    /// more, and their children are killed.
    pub fn forget_window(&self, window: &str) {
        let prefix = format!("{window}#");
        let mine = |w: &str| w == window || w.starts_with(&prefix);
        let doomed: Vec<Slot> = {
            let mut reg = lock(&self.reg);
            reg.current.retain(|w, _| !mine(w));
            let old: Vec<Tag> = reg.children.keys().filter(|t| mine(&t.window)).cloned().collect();
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

    pub(crate) fn acquire(&self, tag: Option<&Tag>, share: Share) -> Result<Permit<'_>, RunError> {
        let mut n = lock(&self.running);
        loop {
            if tag.is_some_and(|t| !self.is_current(t)) {
                return Err(RunError::Cancelled);
            }
            // Paging (tagged) leaves `RESERVED` slots free, so a click on a changed file or a
            // message never waits behind a page of dozens of repositories.
            let limit = match share {
                Share::Paging => PARALLEL - RESERVED,
                Share::Reserved => PARALLEL,
            };
            if *n < limit {
                *n += 1;
                return Ok(Permit(self));
            }
            n = self.freed.wait_timeout(n, Duration::from_millis(50)).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    /// `git <args>` in `cwd`: its stdout bytes. With a `tag`, the child belongs to that query
    /// and is killed when the query is superseded. Tagged calls take the paging share of the
    /// pool, untagged ones the whole pool.
    pub fn run(&self, tag: Option<&Tag>, cwd: &Path, args: &[&str]) -> Result<Vec<u8>, RunError> {
        let share = if tag.is_some() { Share::Paging } else { Share::Reserved };
        self.run_in(tag, share, cwd, args)
    }

    /// [`Runner::run`] waiting for the given `share` of the pool: a click that still belongs to
    /// a query (killed with it) takes `Share::Reserved`.
    pub fn run_in(&self, tag: Option<&Tag>, share: Share, cwd: &Path, args: &[&str]) -> Result<Vec<u8>, RunError> {
        let (code, out, err) = self.exec(tag, share, cwd, args)?;
        if code == 0 {
            return Ok(out);
        }
        let msg = String::from_utf8_lossy(&err);
        let line = msg.lines().map(str::trim).find(|l| !l.is_empty()).map(String::from);
        Err(RunError::Failed(line.unwrap_or_else(|| format!("git exited with exit status: {code}"))))
    }

    /// `git <args>` in `cwd`: its exit code and stdout bytes, for a command whose exit code is an
    /// answer (`merge-base` exits 1 for no common history). A non-zero exit is not an error here;
    /// a child killed by a newer query is `Cancelled`, and one killed by anything else `Failed`.
    /// The pool share is [`Runner::run`]'s.
    pub fn run_code(&self, tag: Option<&Tag>, cwd: &Path, args: &[&str]) -> Result<(i32, Vec<u8>), RunError> {
        let share = if tag.is_some() { Share::Paging } else { Share::Reserved };
        self.exec(tag, share, cwd, args).map(|(code, out, _)| (code, out))
    }

    /// The one place a child is spawned: exit code, stdout, stderr.
    fn exec(&self, tag: Option<&Tag>, share: Share, cwd: &Path, args: &[&str]) -> Result<(i32, Vec<u8>, Vec<u8>), RunError> {
        if !cwd.is_dir() {
            return Err(RunError::Spawn(format!("The folder {} is missing", cwd.display())));
        }
        let _permit = self.acquire(tag, share)?;
        let mut cmd = Command::new(self.git.get_or_init(resolve_git));
        cmd.args(["--no-pager", "-c", "color.ui=never", "-c", "core.quotepath=off", "-c", "i18n.logOutputEncoding=UTF-8"]).args(args).current_dir(cwd);
        claudegate_core::gitlog::sanitize_git_env(&mut cmd);
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
            Some(s) => match s.code() {
                Some(code) => Ok((code, out, err)),
                // Killed by a signal nobody here sent.
                None => Err(RunError::Failed(format!("git exited with {s}"))),
            },
        }
    }

    pub(crate) fn text(&self, tag: Option<&Tag>, cwd: &Path, args: &[&str]) -> Result<String, RunError> {
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
    /// Where the commit lives: `HEAD` (the main checkout's history), `refs/heads/<b>` (only on
    /// that worktree's branch), a sha (only on that detached worktree), or, with the branch
    /// filter on, the branch as typed.
    pub source: String,
    /// File History only: the file as this commit saw it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<HistoryFileView>,
}

impl RowView {
    fn of(c: Commit, source: String, file: Option<HistoryFileView>) -> Self {
        RowView { repo: c.repo, sha: c.sha, time: c.time, author: c.author, email: c.email, subject: c.subject, parents: c.parents, source, file }
    }
}

/// One file's history in one repository, as the UI asks for it. The repository must be one of the
/// backend's own, and the path a plain path in it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryReq {
    pub repo: String,
    pub path: String,
}

/// The file a File History row is about: its name after the commit, the name it was renamed or
/// copied from, and the status words of `ChangedFileView` (`None` when git listed no change, as for
/// a merge).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFileView {
    pub path: String,
    pub from: Option<String>,
    pub status: Option<&'static str>,
}

impl From<HistoryFile> for HistoryFileView {
    fn from(h: HistoryFile) -> Self {
        HistoryFileView { path: h.path, from: h.from, status: h.status.as_ref().map(status_word) }
    }
}

/// The UI's word for a status.
fn status_word(s: &FileStatus) -> &'static str {
    match s {
        FileStatus::Added => "added",
        FileStatus::Modified => "modified",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed { .. } => "renamed",
        FileStatus::Copied { .. } => "copied",
        FileStatus::TypeChanged => "typeChanged",
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
        let status = status_word(&c.status);
        let from = match c.status {
            FileStatus::Renamed { from } | FileStatus::Copied { from } => Some(from),
            _ => None,
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
    /// For each head after the first: the commits only it has (`rev-list <first>..<head>`).
    only: Vec<(String, HashSet<String>)>,
}

impl RepoQuery {
    /// The head a row is labelled with: the first head (`HEAD`, or the branch filter's), unless
    /// only another head has the commit. Not git's `--source`, which labels shared history with
    /// whichever head reached it first in date order.
    fn source_of(&self, sha: &str) -> String {
        for (h, set) in &self.only {
            if set.contains(sha) {
                return h.clone();
            }
        }
        self.heads.first().cloned().unwrap_or_default()
    }
}

/// `rev-list` reads at most this many commits per worktree for its "only here" set; a branch
/// further ahead than that labels its older commits with the first head.
const ONLY_MAX: usize = 10_000;

struct QueryState {
    query: u64,
    /// `within` already resolved against the query's "now", fixed at its first page.
    filter: Filter,
    repos: Vec<RepoQuery>,
    progress: BTreeMap<String, RepoProgress>,
    failures: Vec<Failure>,
    branch_use: Option<BranchUse>,
    /// File History only.
    history: Option<HistoryState>,
}

/// A File History query: every name the file has had (read once, on the first page), the name
/// it had at the oldest commit read so far (where a status-less record on the next page starts),
/// and each pending commit's file.
struct HistoryState {
    /// What the query was started for; a Load more for another file is stale.
    req: HistoryReq,
    paths: Vec<String>,
    last: String,
    files: HashMap<String, HistoryFile>,
}

/// The cancellation channel of File History, so it never supersedes the log's paging.
pub const HISTORY_CHANNEL: &str = "history";

/// The cancellation channel of Blame: the cursor moving on supersedes only the previous blame.
pub const BLAME_CHANNEL: &str = "blame";
/// The largest line number Blame asks git about; anything above is not a line of a file we show.
pub const MAX_BLAME_LINE: u32 = 100_000_000;

/// Who last changed one line, as the UI reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameLineView {
    pub sha: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub summary: String,
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
pub(crate) fn tree_path_ok(p: &str) -> bool {
    // Every component must be a plain name: a Windows `C:\x` or `\\server\share` would otherwise
    // replace the root when joined, and a backslash is never a separator in a git path.
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\0')
        && !p.contains('\\')
        && !p.split('/').any(|s| s == "." || s == "..")
        && Path::new(p).components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

impl Default for GitLog {
    fn default() -> Self {
        GitLog::new(Runner::lazy())
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

    /// The cached repository ids, in discovery order; `None` before the first discovery.
    pub fn known_ids(&self) -> Option<Vec<String>> {
        lock(&self.repos).as_ref().map(|(_, l)| l.iter().map(|r| r.id.clone()).collect())
    }

    /// Whether a repository list was ever loaded (an empty one counts).
    pub fn loaded(&self) -> bool {
        lock(&self.repos).is_some()
    }

    /// A window closed: drop its queries (the log's and File History's) and kill their children.
    pub fn forget_window(&self, window: &str) {
        self.runner.forget_window(window);
        let prefix = format!("{window}#");
        lock(&self.queries).retain(|k, _| k != window && !k.starts_with(&prefix));
    }

    pub(crate) fn cached(&self) -> Arc<Vec<Repo>> {
        lock(&self.repos).as_ref().map(|(_, l)| l.clone()).unwrap_or_default()
    }

    pub(crate) fn cwd_of(&self, repo: &str) -> Result<PathBuf, String> {
        self.cached().iter().find(|r| r.id == repo).map(main_path).ok_or_else(|| "Unknown repository; refresh the list".to_string())
    }

    /// One page of the merged log. `more: false` starts query `query_id` (superseding the
    /// window's previous one, whose children are killed); `more: true` continues it.
    ///
    /// With `history`: one file's history in one repository, followed across renames, on its own
    /// channel (`<window>#history`) so it never supersedes the log. Only `history.repo` is
    /// searched, whatever `repos` says, and of the filter only the branch applies.
    pub fn page(&self, window: &str, query_id: u64, repos: &[String], filter: &FilterView, more: bool, history: Option<&HistoryReq>) -> Result<PageView, String> {
        if let Some(h) = history {
            if !tree_path_ok(&h.path) {
                return Err("Not a path in the repository".into());
            }
            // Only a repository from the backend's own list.
            self.cwd_of(&h.repo)?;
        }
        let key = match history {
            Some(_) => channel(window, HISTORY_CHANNEL),
            None => window.to_string(),
        };
        let tag = Tag { window: key.clone(), query: query_id };
        let state = if more {
            let existing = lock(&self.queries).get(&key).cloned();
            match existing {
                // The same query, still current, and (for File History) the same file.
                Some(s) if self.runner.is_current(&tag) && {
                    let st = lock(&s);
                    st.query == query_id && st.history.as_ref().map(|h| &h.req) == history
                } => s,
                _ => return Err(STALE.into()),
            }
        } else {
            if !self.runner.set_current(&key, query_id) {
                return Err(STALE.into());
            }
            let st = match history {
                None => self.start(&tag, repos, filter)?,
                Some(h) => self.start_history(&tag, h, filter)?,
            };
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
            q.insert(key, state);
        }
        Ok(view)
    }

    /// A new File History query: the repository's heads under the branch filter (nothing else of
    /// the filter), then one unfiltered `--follow` walk for every name the file has had. That
    /// walk failing makes the repository a failure row, like a log that fails.
    fn start_history(&self, tag: &Tag, h: &HistoryReq, fv: &FilterView) -> Result<QueryState, String> {
        let only_branch = FilterView { branch: fv.branch.clone(), ..Default::default() };
        let mut st = self.start(tag, std::slice::from_ref(&h.repo), &only_branch)?;
        let mut paths = vec![h.path.clone()];
        if let Some(q) = st.repos.first() {
            // A folder has no history of its own here: its pathspec would match every file in it.
            // A path missing at the head (a deleted file) still has one, so only "tree" refuses.
            if let Some(head) = q.heads.first() {
                match self.runner.text(Some(tag), &q.cwd, &["cat-file", "-t", &format!("{head}:{}", h.path)]) {
                    Ok(kind) if kind.trim() == "tree" => return Err("Not a file".into()),
                    Err(RunError::Cancelled) => return Err(STALE.into()),
                    _ => {}
                }
            }
            let args = history_paths_args(&h.path, &q.heads);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            match self.runner.text(Some(tag), &q.cwd, &args) {
                Ok(out) => paths = parse_history_paths(&out, &h.path),
                Err(RunError::Cancelled) => return Err(STALE.into()),
                Err(e) => {
                    let id = q.id.clone();
                    st.progress.remove(&id);
                    st.failures.push(Failure { repo: id, reason: e.reason() });
                }
            }
        }
        st.history = Some(HistoryState { req: h.clone(), paths, last: h.path.clone(), files: HashMap::new() });
        Ok(st)
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
            // One spawn per extra head; a repository with one head skips this.
            let mut only = Vec::new();
            for h in heads.iter().skip(1) {
                let range = format!("{}..{h}", heads[0]);
                let max = format!("--max-count={ONLY_MAX}");
                match self.runner.text(Some(tag), &cwd, &["rev-list", &max, "--end-of-options", &range, "--"]) {
                    Ok(out) => only.push((h.clone(), out.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())),
                    Err(RunError::Cancelled) => return Err(RunError::Cancelled),
                    // The log of that head fails too, and says why.
                    Err(_) => {}
                }
            }
            Ok(Some((RepoQuery { id: r.id.clone(), cwd, heads, me, only }, has)))
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
        Ok(QueryState { query: tag.query, filter, repos: rq, progress, failures: Vec::new(), branch_use, history: None })
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
                let history = st.history.as_ref();
                let got = par_map(&need, |q| {
                    let skip = progress.get(&q.id).map_or(0, |p| p.fetched);
                    let args = match history {
                        Some(h) => history_args(&q.heads, page, skip, &h.paths),
                        None => log_args(filter, &q.heads, q.me.as_deref(), page, skip),
                    };
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
                            let text = String::from_utf8_lossy(&out);
                            let (n, commits) = match st.history.as_mut() {
                                Some(h) => {
                                    let (rows, n) = parse_history(&text, &h.last);
                                    if let Some((_, f)) = rows.last() {
                                        h.last = f.from.clone().unwrap_or_else(|| f.path.clone());
                                    }
                                    let mut commits = Vec::with_capacity(rows.len());
                                    for (c, f) in rows {
                                        h.files.insert(c.sha.clone(), f);
                                        commits.push(c);
                                    }
                                    (n, commits)
                                }
                                None => (count_records(&out), parse_log(&text)),
                            };
                            let Some(p) = st.progress.get_mut(&id) else { continue };
                            p.fetched += n;
                            p.exhausted = n < page;
                            p.pending.extend(commits.into_iter().map(|mut c| {
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
            rows: rows
                .into_iter()
                .map(|c| {
                    let src = st.repos.iter().find(|q| q.id == c.repo).map(|q| q.source_of(&c.sha)).unwrap_or_default();
                    let file = st.history.as_mut().and_then(|h| h.files.remove(&c.sha)).map(HistoryFileView::from);
                    RowView::of(c, src, file)
                })
                .collect(),
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
    pub fn file(&self, repo: &str, sha: &str, path: &str, side: Side, from: Option<&str>) -> Result<FileView, String> {
        if !is_sha(sha) {
            return Err("Not a commit id".into());
        }
        if !tree_path_ok(path) || from.is_some_and(|f| !tree_path_ok(f)) {
            return Err("Not a path in the repository".into());
        }
        let cwd = self.cwd_of(repo)?;
        let spec = match side {
            Side::After => format!("{sha}:{path}"),
            Side::Before => {
                let line = self.runner.text(None, &cwd, &["rev-list", "--parents", "-n", "1", sha]).map_err(|e| e.reason())?;
                let Some(p0) = line.split_whitespace().nth(1).map(String::from) else { return Ok(FileView::Missing) };
                let mut name = path.to_string();
                if let Some(f) = from {
                    // The caller already knows the rename source; no lookup needed.
                    name = f.to_string();
                } else {
                    let ns = self.runner.text(None, &cwd, &Self::show_args("--name-status", sha)).map_err(|e| e.reason())?;
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
                }
                if !tree_path_ok(&name) {
                    return Ok(FileView::Missing);
                }
                format!("{p0}:{name}")
            }
        };
        self.blob_view(&cwd, &spec)
    }

    /// The file `path` at commit `rev` (`<rev>:<path>`): Combined Changes' right side.
    pub fn rev_file(&self, repo: &str, rev: &str, path: &str) -> Result<FileView, String> {
        if !is_sha(rev) {
            return Err("Not a commit id".into());
        }
        if !tree_path_ok(path) {
            return Err("Not a path in the repository".into());
        }
        let cwd = self.cwd_of(repo)?;
        self.blob_view(&cwd, &format!("{rev}:{path}"))
    }

    /// Who last changed the 1-based `line` of `path` as of commit `rev`, or `None` when git has
    /// no answer (the line is past the end, the file is not in that commit). On its own channel
    /// (`<window>#blame`): the next call supersedes this one and kills its git. Takes a reserved
    /// slot, so it never waits behind paging.
    pub fn blame(&self, window: &str, query_id: u64, repo: &str, rev: &str, path: &str, line: u32) -> Result<Option<BlameLineView>, String> {
        if !is_sha(rev) {
            return Err("Not a commit id".into());
        }
        if !tree_path_ok(path) {
            return Err("Not a path in the repository".into());
        }
        if line < 1 || line > MAX_BLAME_LINE {
            return Err("Not a line number".into());
        }
        let cwd = self.cwd_of(repo)?;
        let tag = Tag { window: channel(window, BLAME_CHANNEL), query: query_id };
        if !self.runner.set_current(&tag.window, query_id) {
            return Err(STALE.into());
        }
        let args = blame_args(rev, path, line);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.runner.run_in(Some(&tag), Share::Reserved, &cwd, &args) {
            Ok(out) => Ok(parse_blame_porcelain(&String::from_utf8_lossy(&out)).map(|b| BlameLineView {
                sha: b.sha,
                author: b.author,
                email: b.email,
                time: b.time,
                summary: b.summary,
            })),
            Err(RunError::Cancelled) => Err(STALE.into()),
            Err(RunError::Failed(_)) => Ok(None),
            Err(e) => Err(e.reason()),
        }
    }

    /// Drops the window's in-flight blame: its git is killed, and a blame older than `query_id`
    /// arriving late is stale.
    pub fn blame_cancel(&self, window: &str, query_id: u64) {
        self.runner.set_current(&channel(window, BLAME_CHANNEL), query_id);
    }

    /// The object `spec` names. No such object: missing. The size first, so a huge blob is never read.
    fn blob_view(&self, cwd: &Path, spec: &str) -> Result<FileView, String> {
        let size = match self.runner.text(None, cwd, &["cat-file", "-s", spec]) {
            Ok(s) => s,
            Err(RunError::Failed(_)) => return Ok(FileView::Missing),
            Err(e) => return Err(e.reason()),
        };
        let size: u64 = size.trim().parse().map_err(|_| "Unexpected output from git cat-file".to_string())?;
        if size > MAX_FILE {
            return Ok(FileView::TooLarge(size));
        }
        let bytes = self.runner.run(None, cwd, &["cat-file", "blob", spec]).map_err(|e| e.reason())?;
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
    history: Option<HistoryReq>,
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
        gl.page(&label, query_id, &repos, &filter, more, history.as_ref())
    })
    .await?
}

#[tauri::command]
pub async fn gitlog_changes(repo: String, sha: String, state: tauri::State<'_, GitLogState>) -> Result<Vec<ChangedFileView>, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.changes(&repo, &sha)).await?
}

#[tauri::command]
pub async fn gitlog_file(
    repo: String,
    sha: String,
    path: String,
    side: Side,
    from: Option<String>,
    state: tauri::State<'_, GitLogState>,
) -> Result<FileView, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.file(&repo, &sha, &path, side, from.as_deref())).await?
}

/// The working-tree file for a changed path, for "Open in editor": `path` is a path in the
/// repository (as `gitlog_changes` reports it) and the file must still be there. A commit's file
/// may be gone by now (deleted or renamed later), which is an error, never a new empty buffer.
/// Read-only: nothing is created.
#[tauri::command]
pub async fn gitlog_worktree_file(repo: String, path: String, source: Option<String>, state: tauri::State<'_, GitLogState>) -> Result<String, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || {
        let root = gl.cached().iter().find(|r| r.id == repo).map(|r| checkout_path(r, source.as_deref())).ok_or_else(|| "Unknown repository; refresh the list".to_string())?;
        worktree_file(&root, &path)
    })
    .await?
}

/// The checkout a commit's `source` (`RowView.source`: `HEAD`, `refs/heads/<b>`, a branch name or a
/// sha) lives in: one of this repository's own checkouts, never a path from the caller. Anything
/// that matches none falls back to the main checkout.
pub fn checkout_path(r: &Repo, source: Option<&str>) -> PathBuf {
    let want = source.map(str::trim).filter(|s| !s.is_empty() && *s != "HEAD");
    let hit = want.and_then(|w| {
        let b = w.strip_prefix("refs/heads/").unwrap_or(w);
        r.checkouts.iter().find(|c| c.branch.as_deref() == Some(b)).or_else(|| r.checkouts.iter().find(|c| c.head == w))
    });
    match hit {
        Some(c) => PathBuf::from(&c.path),
        None => main_path(r),
    }
}

/// Symlinks are followed by design: the same as `open_in_editor`, which opens whatever the path names.
pub fn worktree_file(root: &std::path::Path, path: &str) -> Result<String, String> {
    if !tree_path_ok(path) {
        return Err("Not a path in the repository".into());
    }
    let full = root.join(path);
    if !full.is_file() {
        return Err(format!("{path} is no longer in the working tree"));
    }
    Ok(full.to_string_lossy().into_owned())
}

/// The file `path` at commit `rev`.
#[tauri::command]
pub async fn gitlog_rev_file(repo: String, rev: String, path: String, state: tauri::State<'_, GitLogState>) -> Result<FileView, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.rev_file(&repo, &rev, &path)).await?
}

/// Who last changed one line of `path` as of commit `rev`.
#[tauri::command]
pub async fn gitlog_blame(
    query_id: u64,
    repo: String,
    rev: String,
    path: String,
    line: u32,
    window: tauri::Window,
    state: tauri::State<'_, GitLogState>,
) -> Result<Option<BlameLineView>, String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || gl.blame(&label, query_id, &repo, &rev, &path, line)).await?
}

/// Cancels the window's blame in flight (the cursor left the line, blame was switched off).
#[tauri::command]
pub async fn gitlog_blame_cancel(query_id: u64, window: tauri::Window, state: tauri::State<'_, GitLogState>) -> Result<(), String> {
    let gl = state.inner().0.clone();
    let label = window.label().to_string();
    crate::commands::blocking(move || gl.blame_cancel(&label, query_id)).await
}

#[tauri::command]
pub async fn gitlog_message(repo: String, sha: String, state: tauri::State<'_, GitLogState>) -> Result<String, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || gl.message(&repo, &sha)).await?
}

/// A link the app may hand to the system: http(s) only, a plain host (`[A-Za-z0-9.-]`, optional
/// `:port`), and none of the characters a Windows command-line tail or a shell could read as
/// syntax. `%` is only allowed as a valid percent-escape. `rundll32 url.dll,FileProtocolHandler`
/// takes the raw tail, so this is checked even though the link is built here from `origin`.
pub fn check_open_url(url: &str) -> Result<(), String> {
    let bad = || Err::<(), String>("That link can't be opened.".into());
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).ok_or("Only http and https links can be opened.")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return bad();
    }
    if port.is_some_and(|p| p.is_empty() || p.len() > 5 || !p.chars().all(|c| c.is_ascii_digit())) {
        return bad();
    }
    let b = url.as_bytes();
    for (i, c) in url.char_indices() {
        let hex2 = |at: usize| b.get(at + 1).is_some_and(u8::is_ascii_hexdigit) && b.get(at + 2).is_some_and(u8::is_ascii_hexdigit);
        let ok = !(c.is_control() || c.is_whitespace() || !c.is_ascii() || matches!(c, '"' | '<' | '>' | '^' | '|' | '`' | '\\' | '\'' | '{' | '}'))
            && (c != '%' || hex2(i));
        if !ok {
            return bad();
        }
    }
    Ok(())
}

/// Opens the commit's page on its remote in the system browser. The webview names the commit,
/// never the URL: the link is built here, from the repository's own `origin`.
#[tauri::command]
pub async fn gitlog_open_remote(repo: String, sha: String, state: tauri::State<'_, GitLogState>) -> Result<bool, String> {
    let gl = state.inner().0.clone();
    crate::commands::blocking(move || {
        let Some(url) = gl.web_url(&repo, &sha)? else { return Ok(false) };
        check_open_url(&url)?;
        #[cfg(target_os = "macos")]
        let mut cmd = Command::new("open");
        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = Command::new("rundll32");
            c.arg("url.dll,FileProtocolHandler");
            c
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let mut cmd = Command::new("xdg-open");
        let mut child = cmd
            .arg(&url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't open the browser: {e}"))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(true)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hostile_origin_never_reaches_the_opener() {
        let sha = "a".repeat(40);
        for remote in ["https://github.x\"&y.com/acme/r.git", "https://github.com/acme/r\"&calc.git", "git@github.com:acme/r^|x.git"] {
            if let Some(url) = web_url(remote, &sha) {
                assert!(check_open_url(&url).is_err(), "{remote} -> {url}");
            }
        }
        // This one does produce a link, so the assertion above can never be skipped for it.
        {
            let url = web_url("https://github.x\"&y.com/acme/r.git", &sha).expect("a link is built for this origin");
            assert!(check_open_url(&url).is_err(), "{url}");
        }
        let ok = web_url("git@github.com:acme/acme-api.git", &sha).expect("github link");
        assert!(check_open_url(&ok).is_ok(), "{ok}");
    }

    #[test]
    fn only_plain_web_links_are_opened() {
        assert!(check_open_url("https://github.com/acme/acme-api/commit/abc").is_ok());
        assert!(check_open_url("http://git.acme.test/x").is_ok());
        assert!(check_open_url("https://git.acme.test:8443/a/b/commit/abc?x=1#y%20z").is_ok());
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "-flag", "https://", "https://u:p@github.com/x", "https://a.test/x y", "https://a.test/\nx", "ssh://git@a.test/x", "https://github.x\"&y.com/a", "https://a.test/x\"y", "https://a.test/x^y", "https://a.test/x|y", "https://a.test/x`y", "https://a.test/x\\y", "https://a.test/x<y>", "https://a.test/100%", "https://a.test/%zz", "https://a.test:80a/x", "https://a_b.test/x", "https://a.test%2f.evil/x"] {
            assert!(check_open_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn untagged_calls_keep_reserved_slots() {
        let r = Runner::lazy();
        assert!(r.set_current("w", 1));
        let tag = Tag { window: "w".into(), query: 1 };
        let held: Vec<_> = (0..PARALLEL - RESERVED).map(|_| r.acquire(Some(&tag), Share::Paging).ok().expect("a paging slot")).collect();
        // Paging is out of slots, but an untagged call still gets one.
        let extra: Vec<_> = (0..RESERVED).map(|_| r.acquire(None, Share::Reserved).ok().expect("a reserved slot")).collect();
        assert_eq!(*lock(&r.running), PARALLEL);
        drop(extra);
        drop(held);
        assert_eq!(*lock(&r.running), 0);
    }

    #[test]
    fn forget_window_kills_every_channel() {
        let r = Runner::lazy();
        let w = Tag { window: "w".into(), query: 1 };
        let wc = Tag { window: channel("w", "combined"), query: 2 };
        let other = Tag { window: channel("wx", "combined"), query: 3 };
        for t in [&w, &wc, &other] {
            assert!(r.set_current(&t.window, t.query));
            assert!(r.is_current(t));
        }
        // A query on one channel leaves the window's other channels current.
        assert!(r.set_current(&wc.window, 4));
        assert!(r.is_current(&w));
        assert!(!r.set_current(&wc.window, 2), "an older id on the channel is late");
        let wc = Tag { query: 4, ..wc };
        r.forget_window("w");
        assert!(!r.is_current(&w));
        assert!(!r.is_current(&wc));
        assert!(r.is_current(&other), "\"wx\" is another window");
    }

    /// Every child runs with the sanitized environment, lazy fetch off included.
    #[cfg(unix)]
    #[test]
    fn children_never_lazy_fetch() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("envgit");
        std::fs::write(&fake, "#!/bin/sh\nexec env\n").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = Runner::new(fake);
        let env = r.text(None, dir.path(), &["status"]).unwrap();
        for want in ["GIT_NO_LAZY_FETCH=1", "GIT_OPTIONAL_LOCKS=0", "GIT_TERMINAL_PROMPT=0"] {
            assert!(env.lines().any(|l| l == want), "{want} missing from:\n{env}");
        }
        // The same helper builds core's `RealGit` command.
        let mut cmd = Command::new("git");
        claudegate_core::gitlog::sanitize_git_env(&mut cmd);
        let set: Vec<_> = cmd.get_envs().filter_map(|(k, v)| Some((k.to_str()?.to_string(), v?.to_str()?.to_string()))).collect();
        assert!(set.contains(&("GIT_NO_LAZY_FETCH".to_string(), "1".to_string())), "{set:?}");
    }

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
    fn worktree_file_is_read_only_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "x").unwrap();
        let got = worktree_file(dir.path(), "src/a.rs").unwrap();
        assert!(got.ends_with("a.rs"));
        assert!(worktree_file(dir.path(), "src/gone.rs").unwrap_err().contains("no longer"));
        assert!(worktree_file(dir.path(), "src").is_err(), "a folder is not a file");
        assert!(worktree_file(dir.path(), "../x").is_err());
        assert!(!dir.path().join("src/gone.rs").exists(), "nothing was created");
    }

    #[test]
    fn a_worktree_only_commit_opens_in_its_own_checkout() {
        use claudegate_core::gitlog::Checkout;
        let co = |path: &str, head: &str, branch: Option<&str>, is_main| Checkout { path: path.into(), head: head.into(), branch: branch.map(String::from), is_main };
        let r = Repo {
            id: "/w/acme-api/.git".into(),
            name: "acme-api".into(),
            color: 0,
            checkouts: vec![co("/w/acme-api", "aaa", Some("main"), true), co("/w/acme-api-wt", "bbb", Some("feat"), false), co("/w/acme-api-det", "ccc", None, false)],
        };
        assert_eq!(checkout_path(&r, None), PathBuf::from("/w/acme-api"));
        assert_eq!(checkout_path(&r, Some("HEAD")), PathBuf::from("/w/acme-api"));
        assert_eq!(checkout_path(&r, Some("refs/heads/feat")), PathBuf::from("/w/acme-api-wt"));
        assert_eq!(checkout_path(&r, Some("feat")), PathBuf::from("/w/acme-api-wt"));
        assert_eq!(checkout_path(&r, Some("ccc")), PathBuf::from("/w/acme-api-det"));
        assert_eq!(checkout_path(&r, Some("/etc")), PathBuf::from("/w/acme-api"), "an unknown source never becomes a root");
        // The file is looked up in that checkout, not the main one.
        let main = tempfile::tempdir().unwrap();
        let wt = tempfile::tempdir().unwrap();
        std::fs::write(wt.path().join("only.rs"), "x").unwrap();
        let r2 = Repo { checkouts: vec![co(main.path().to_str().unwrap(), "a", Some("main"), true), co(wt.path().to_str().unwrap(), "b", Some("feat"), false)], ..r };
        assert!(worktree_file(&checkout_path(&r2, Some("feat")), "only.rs").is_ok());
        assert!(worktree_file(&checkout_path(&r2, None), "only.rs").unwrap_err().contains("no longer"));
    }

    #[test]
    fn tree_paths_reject_roots_and_backslashes() {
        assert!(!tree_path_ok("C:\\x"));
        assert!(!tree_path_ok("\\\\server\\share\\f"));
        assert!(!tree_path_ok("a\\b"));
        assert!(tree_path_ok("a/b c/d.txt"));
    }

    #[test]
    fn tree_paths() {
        assert!(tree_path_ok("a/b.txt"));
        assert!(!tree_path_ok("./a"));
        assert!(!tree_path_ok("a/../b"));
        assert!(!tree_path_ok("/etc/passwd"));
    }
}
