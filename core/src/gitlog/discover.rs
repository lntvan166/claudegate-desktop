//! Finding the repositories behind a workspace. A bounded walk of the roots collects every
//! directory with a `.git` entry; one `git rev-parse` per candidate resolves it to its git
//! common dir, so a main checkout and all of its worktrees are ONE `Repo`.
//! Read-only: only `git` reads are run, nothing is written.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use md5::{Digest, Md5};

use crate::excludes::{glob_to_regex, ExcludeMatcher};

/// Number of colours in the UI palette; `Repo::color` is an index below this.
pub const PALETTE_LEN: u8 = 12;
/// The default `scan_depth`.
pub const DEFAULT_SCAN_DEPTH: usize = 2;
const SKIP: &[&str] = &[".git", "node_modules", "vendor", "target"];

/// Runs `git` with the given arguments in `cwd`; `None` on a spawn failure or non-zero exit.
/// A trait so tests (and the app) can substitute the runner.
pub trait GitRun {
    fn run(&self, cwd: &Path, args: &[&str]) -> Option<String>;
}

/// The real `git` binary: no pager, no colour, unquoted paths, never prompts.
pub struct RealGit;

impl GitRun for RealGit {
    fn run(&self, cwd: &Path, args: &[&str]) -> Option<String> {
        let out = Command::new("git")
            .args(["--no-pager", "-c", "color.ui=never", "-c", "core.quotepath=off"])
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    pub path: String,
    /// What to log for this checkout: its HEAD commit.
    pub head: String,
    /// The checked-out branch (short name); `None` when detached.
    pub branch: Option<String>,
    pub is_main: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    /// The resolved git common dir.
    pub id: String,
    pub name: String,
    /// Index into the UI palette, stable for a given `id`.
    pub color: u8,
    /// The main checkout first, then its worktrees.
    pub checkouts: Vec<Checkout>,
}

/// Candidate repository directories under `roots`, sorted. A directory with a `.git` entry (a
/// directory, or a file for worktrees and submodules) is a candidate; descent continues past it
/// so a nested repository within `scan_depth` is still found. Symlinks are never followed.
fn walk(roots: &[PathBuf], scan_depth: usize, skip_excluded: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let mut frontier = vec![root.clone()];
        for level in 0..=scan_depth {
            let mut next = Vec::new();
            for dir in &frontier {
                if skip_excluded(dir) {
                    continue;
                }
                let Ok(rd) = fs::read_dir(dir) else { continue };
                let mut has_git = false;
                for entry in rd.flatten() {
                    let name = entry.file_name();
                    if name == ".git" {
                        has_git = true;
                        continue;
                    }
                    if level == scan_depth {
                        continue;
                    }
                    let Ok(ft) = entry.file_type() else { continue };
                    if !ft.is_dir() || SKIP.iter().any(|s| name == *s) {
                        continue;
                    }
                    next.push(entry.path());
                }
                if has_git {
                    found.push(dir.clone());
                }
            }
            frontier = next;
        }
    }
    found.sort();
    found.dedup();
    found
}

fn canon(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// `(common_dir, toplevel)` of the checkout at `dir`, both resolved.
fn identify(dir: &Path, git: &dyn GitRun) -> Option<(PathBuf, PathBuf)> {
    let out = git.run(dir, &["rev-parse", "--git-common-dir", "--show-toplevel"])?;
    let mut lines = out.lines();
    let common = lines.next()?.trim();
    let top = lines.next()?.trim();
    if common.is_empty() || top.is_empty() {
        return None;
    }
    let common = Path::new(common);
    let common = if common.is_absolute() { common.to_path_buf() } else { dir.join(common) };
    Some((canon(&common), canon(Path::new(top))))
}

/// Parses `git worktree list --porcelain`: the first entry is the main checkout. Bare,
/// prunable and unborn (all-zero HEAD) entries are dropped.
fn parse_worktrees(out: &str) -> Vec<Checkout> {
    let mut res = Vec::new();
    for (i, block) in out.split("\n\n").enumerate() {
        let (mut path, mut head, mut branch) = (None, None, None);
        let (mut bare, mut prunable) = (false, false);
        for line in block.lines() {
            if let Some(v) = line.strip_prefix("worktree ") {
                path = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("HEAD ") {
                head = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("branch ") {
                branch = Some(v.strip_prefix("refs/heads/").unwrap_or(v).to_string());
            } else if line == "bare" {
                bare = true;
            } else if line == "prunable" || line.starts_with("prunable ") {
                prunable = true;
            }
        }
        let (Some(path), Some(head)) = (path, head) else { continue };
        if bare || prunable || head.is_empty() || head.chars().all(|c| c == '0') {
            continue;
        }
        res.push(Checkout { path, head, branch, is_main: i == 0 });
    }
    res
}

fn color_of(id: &str) -> u8 {
    Md5::digest(id.as_bytes())[0] % PALETTE_LEN
}

fn base(path: &str) -> &str {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path)
}

/// Folder-name labels; repos sharing a name get their parent folder (`name (parent)`), and
/// any still colliding get the full path.
fn label(repos: &mut [Repo]) {
    let main_path = |r: &Repo| r.checkouts.first().map(|c| c.path.clone()).unwrap_or_default();
    let mut count: HashMap<String, usize> = HashMap::new();
    for r in repos.iter() {
        *count.entry(base(&main_path(r)).to_string()).or_default() += 1;
    }
    for r in repos.iter_mut() {
        let p = main_path(r);
        let name = base(&p).to_string();
        r.name = if count[&name] > 1 {
            let parent = Path::new(&p).parent().and_then(|d| d.file_name()).and_then(|n| n.to_str()).unwrap_or("").to_string();
            format!("{name} ({parent})")
        } else {
            name
        };
    }
    let mut again: HashMap<String, usize> = HashMap::new();
    for r in repos.iter() {
        *again.entry(r.name.clone()).or_default() += 1;
    }
    for r in repos.iter_mut() {
        if again[&r.name] > 1 {
            r.name = main_path(r);
        }
    }
}

/// The repositories under the workspace `roots`, grouped by git common dir and sorted by name.
/// `exclude` globs (the `claudegate.exclude` syntax, plus a bare folder name) drop directories
/// from the walk and checkouts from the result.
pub fn discover(roots: &[PathBuf], scan_depth: usize, exclude: &[String], git: &dyn GitRun) -> Vec<Repo> {
    let matchers: Vec<ExcludeMatcher> = roots.iter().map(|r| ExcludeMatcher::new(r, exclude)).collect();
    let name_res: Vec<regex::Regex> = exclude.iter().filter_map(|g| glob_to_regex(g)).collect();
    let excluded = |p: &Path| {
        let s = p.to_string_lossy();
        let b = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        matchers.iter().any(|m| m.is_excluded(&s)) || name_res.iter().any(|re| re.is_match(&b))
    };

    let mut by_id: HashMap<PathBuf, Vec<Checkout>> = HashMap::new();
    for dir in walk(roots, scan_depth, &excluded) {
        let Some((common, _top)) = identify(&dir, git) else { continue };
        if by_id.contains_key(&common) {
            continue;
        }
        let list = git
            .run(&dir, &["worktree", "list", "--porcelain"])
            .map(|o| parse_worktrees(&o))
            .unwrap_or_default();
        let list: Vec<Checkout> = list.into_iter().filter(|c| !excluded(Path::new(&c.path))).collect();
        by_id.insert(common, list);
    }
    let mut repos: Vec<Repo> = by_id
        .into_iter()
        .filter(|(_, c)| c.iter().any(|c| c.is_main))
        .map(|(common, checkouts)| {
            let id = common.to_string_lossy().into_owned();
            Repo { color: color_of(&id), id, name: String::new(), checkouts }
        })
        .collect();
    repos.sort_by(|a, b| a.checkouts[0].path.cmp(&b.checkouts[0].path));
    label(&mut repos);
    repos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    repos
}

/// `(ahead, behind)` of the main checkout's HEAD against its upstream, or `None` when it has
/// no upstream (or no main checkout).
pub fn ahead_behind(repo: &Repo, git: &dyn GitRun) -> Option<(u32, u32)> {
    let main = repo.checkouts.iter().find(|c| c.is_main)?;
    let out = git.run(Path::new(&main.path), &["rev-list", "--left-right", "--count", "@{u}...HEAD"])?;
    let mut it = out.split_whitespace();
    let behind: u32 = it.next()?.parse().ok()?;
    let ahead: u32 = it.next()?.parse().ok()?;
    Some((ahead, behind))
}
