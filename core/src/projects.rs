//! Add and remove projects: register a folder in `workspace-roots.json` so the hook captures
//! Claude's edits there (spec rule 6, "Roots: append, never rewrite").
//!
//! The app records which roots it added (`projects[root].addedByApp` in its own config, behind
//! `AppProjects`). Only those can be removed, and only those are re-added when the extension's
//! unlocked write drops one. Nothing here writes a file itself: the roots list goes through
//! `store`, the app's record through the caller's `AppProjects`.

use crate::paths::resolve_root;
use crate::roots::{check_new_root, read_roots, RootsError};
use crate::store::{roots_add, roots_append, roots_remove, ROOTS_FILE};
use std::path::{Path, PathBuf};

/// The app's record of the roots it added.
pub trait AppProjects {
    /// Every root recorded as `addedByApp`, in resolved form.
    fn app_roots(&self) -> Vec<String>;
    /// Records `root` as added by the app.
    fn mark_added(&mut self, root: &str) -> Result<(), String>;
    /// Drops the app's whole record of `root`.
    fn forget(&mut self, root: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    /// The root as it is now listed (resolved).
    pub root: String,
    /// It was already a project; nothing was written and ownership was not taken.
    pub already: bool,
}

fn root_string(p: &Path) -> Result<String, RootsError> {
    p.to_str().map(String::from).ok_or_else(|| RootsError::Io("The folder's path is not valid UTF-8.".into()))
}

/// Strict view of the roots file for the guard: a malformed file is an error, never "no roots".
fn current_roots(home: &Path) -> Result<Vec<PathBuf>, RootsError> {
    match std::fs::read_to_string(home.join(ROOTS_FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(RootsError::Io(format!("Couldn't read {ROOTS_FILE}: {e}"))),
        Ok(t) => match serde_json::from_str::<serde_json::Value>(&t) {
            Ok(serde_json::Value::Array(_)) => Ok(read_roots(home)),
            _ => Err(RootsError::Io(format!("{ROOTS_FILE} is not a JSON list; fix or delete it, then try again"))),
        },
    }
}

/// Registers `path` as a project:
/// 1. resolves it; 2. checks it is an absolute directory; 3. applies the nesting guard (a git
/// worktree under a root is refused with a hint); 4. appends it to `workspace-roots.json`;
/// 5. records it as added by the app. If step 5 fails the root is taken back out.
/// An existing root is a no-op (`already`), and a VS Code root never becomes the app's.
pub fn add_project(home: &Path, cfg: &mut impl AppProjects, path: &str) -> Result<Added, RootsError> {
    let raw = Path::new(path.trim());
    if !raw.is_absolute() {
        return Err(RootsError::NotAbsolute);
    }
    let root = resolve_root(raw);
    if !root.is_dir() {
        return Err(RootsError::NotADirectory);
    }
    let name = root_string(&root)?;
    let existing = current_roots(home)?;
    match check_new_root(&existing, &root) {
        Err(RootsError::Nested { existing, .. }) => {
            let worktree = crate::worktrees::is_worktree_root(&root);
            return Err(RootsError::Nested { existing, worktree });
        }
        Err(e) => return Err(e),
        Ok(()) => {}
    }
    if existing.iter().any(|e| resolve_root(e) == root) {
        return Ok(Added { root: name, already: true });
    }
    roots_add(home, &root)?;
    if let Err(e) = cfg.mark_added(&name) {
        let _ = roots_remove(home, &root);
        return Err(RootsError::Io(format!("Couldn't record the project in the app's settings: {e}")));
    }
    Ok(Added { root: name, already: false })
}

/// Unregisters a root the app added. Its session file (the review history) is kept, so adding
/// it again restores it. A root the app did not add is `NotAppAdded`.
pub fn remove_project(home: &Path, cfg: &mut impl AppProjects, root: &str) -> Result<(), RootsError> {
    let root = resolve_root(Path::new(root.trim()));
    let name = root_string(&root)?;
    if !cfg.app_roots().iter().any(|r| resolve_root(Path::new(r)) == root) {
        return Err(RootsError::NotAppAdded);
    }
    roots_remove(home, &root)?;
    if let Err(e) = cfg.forget(&name) {
        // Still recorded as the app's, so put it back rather than leave the two disagreeing.
        let _ = roots_add(home, &root);
        return Err(RootsError::Io(format!("Couldn't update the app's settings: {e}")));
    }
    Ok(())
}

/// Re-adds the app's roots that are missing from `workspace-roots.json` (the extension rewrites
/// it without a lock and can drop one). Only folders that still exist are re-added, and none
/// that would now nest under or around another root. Writes nothing when nothing is missing, so
/// running it on every change of the file cannot loop on its own write. Returns what it re-added.
pub fn ensure_app_roots_present(home: &Path, cfg: &impl AppProjects) -> Result<Vec<String>, RootsError> {
    let mine = cfg.app_roots();
    if mine.is_empty() {
        return Ok(Vec::new());
    }
    let mut listed = current_roots(home)?;
    let mut missing: Vec<PathBuf> = Vec::new();
    for r in mine {
        let r = resolve_root(Path::new(&r));
        if listed.iter().any(|l| resolve_root(l) == r) || !r.is_dir() {
            continue;
        }
        if check_new_root(&listed, &r).is_err() {
            continue;
        }
        listed.push(r.clone());
        missing.push(r);
    }
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    roots_append(home, &missing)?;
    Ok(missing.iter().map(|p| p.to_string_lossy().into_owned()).collect())
}
