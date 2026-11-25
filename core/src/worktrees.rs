//! Port of `isWorktreeRoot` / `nestedWorktreesUnder` from the extension's `src/worktrees.ts`
//! (v1.16.1). Filesystem only, read-only; symlinks are never followed.
use std::fs;
use std::path::{Path, PathBuf};

const SKIP: &[&str] = &[
    "node_modules", ".git", "dist", "build", "out", "target", "vendor", "__pycache__", ".next",
    ".nuxt", ".venv", "venv",
];
const MAX_DEPTH: usize = 6;
/// The extension's `claudegate.worktrees.maxAttached` default.
pub const MAX_WORKTREES: usize = 256;

fn gitdir_target(dot_git: &Path) -> Option<String> {
    let raw = fs::read_to_string(dot_git).ok()?;
    let raw = raw.trim();
    let rest = raw.strip_prefix("gitdir:")?;
    // `^gitdir:\s*(.+)$` : `.` excludes newlines, so the match must be a single line.
    let t = rest.trim_start().trim();
    if t.is_empty() || t.contains('\n') {
        return None;
    }
    Some(t.to_string())
}

/// True if `dir` is a git worktree working directory (not a submodule): `.git` is a file whose
/// `gitdir:` target is structurally `<main>/.git/worktrees/<name>`.
pub fn is_worktree_root(dir: &Path) -> bool {
    let dot_git = dir.join(".git");
    match fs::symlink_metadata(&dot_git) {
        Ok(m) if m.is_file() => {}
        _ => return false,
    }
    let Some(target) = gitdir_target(&dot_git) else { return false };
    let worktrees_dir = match Path::new(&target).parent() {
        Some(p) => p,
        None => return false,
    };
    let Some(git_dir) = worktrees_dir.parent() else { return false };
    worktrees_dir.file_name().is_some_and(|n| n == "worktrees")
        && git_dir.file_name().is_some_and(|n| n == ".git")
}

/// Worktree working dirs nested under `root`, sorted, at most 256.
pub fn nested_worktrees_under(root: &Path) -> Vec<PathBuf> {
    nested_worktrees_under_max(root, MAX_WORKTREES)
}

/// `nested_worktrees_under` with the cap (`worktrees.maxAttached`) given.
pub fn nested_worktrees_under_max(root: &Path, max: usize) -> Vec<PathBuf> {
    nested_worktrees_under_skip(root, max, &[])
}

/// `nested_worktrees_under_max` that never enters a directory in `skip` (compared in resolved
/// form): the registered roots nested inside `root`, which own the worktrees under them.
pub fn nested_worktrees_under_skip(root: &Path, max: usize, skip: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut frontier = vec![crate::paths::resolve_root(root)];
    let mut depth = 1;
    while depth <= MAX_DEPTH && !frontier.is_empty() {
        let mut next = Vec::new();
        for dir in &frontier {
            let Ok(rd) = fs::read_dir(dir) else { continue };
            for entry in rd.flatten() {
                // file_type() does not follow symlinks, so symlinked dirs are skipped.
                let Ok(ft) = entry.file_type() else { continue };
                if !ft.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                if SKIP.iter().any(|s| name == *s) {
                    continue;
                }
                let child = entry.path();
                if skip.iter().any(|s| *s == child) {
                    continue;
                }
                if is_worktree_root(&child) {
                    found.push(child);
                } else {
                    next.push(child);
                }
            }
        }
        frontier = next;
        depth += 1;
    }
    found.sort();
    found.truncate(max);
    found
}
