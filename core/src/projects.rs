//! Add and remove projects: register a folder in `workspace-roots.json` so the hook captures
//! Claude's edits there (spec rule 6, "Roots: append, never rewrite").
//!
//! The app records which roots it added (`projects[root].addedByApp` in its own config, behind
//! `AppProjects`). Only those can be removed, and only those are re-added when the extension's
//! unlocked write drops one. Nothing here writes a file itself: the roots list goes through
//! `store`, the app's record through the caller's `AppProjects`.

use crate::paths::resolve_root;
use crate::roots::{read_roots, RootsError};
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removed {
    pub root: String,
    /// Taken out of `workspace-roots.json`. False when VS Code also uses the folder: only the
    /// app's record was dropped and capture stays on.
    pub unregistered: bool,
}

fn root_string(p: &Path) -> Result<String, RootsError> {
    p.to_str().map(String::from).ok_or_else(|| RootsError::Io("The folder's path is not valid UTF-8.".into()))
}

/// Where VS Code and Cursor record their open windows (`User/globalStorage/storage.json`, beside
/// each one's user `settings.json`). Read only.
pub fn storage_files(sources: &crate::settings::ExtSources) -> Vec<PathBuf> {
    [&sources.vscode_user, &sources.cursor_user]
        .into_iter()
        .flatten()
        .filter_map(|settings| settings.parent().map(|user| user.join("globalStorage").join("storage.json")))
        .collect()
}

/// `file:///a/b%20c` -> `/a/b c`. Anything but a local file URI (a remote window) is `None`.
fn file_uri_path(uri: &str) -> Option<PathBuf> {
    let bytes = uri.strip_prefix("file://")?.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(out).ok()?;
    #[cfg(windows)]
    let s = match s.strip_prefix('/') {
        Some(t) if t.as_bytes().get(1) == Some(&b':') => t.to_string(),
        _ => s,
    };
    Some(PathBuf::from(s))
}

/// The folders a `storage.json` lists as open: `windowsState.lastActiveWindow.folder`,
/// `windowsState.openedWindows[].folder` and `backupWorkspaces.folders[].folderUri`. A missing
/// or unparseable file lists none.
fn open_folders(storage: &Path) -> Vec<PathBuf> {
    let Some(v) = std::fs::read_to_string(storage).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return Vec::new();
    };
    let mut uris: Vec<&str> = Vec::new();
    let ws = &v["windowsState"];
    uris.extend(ws["lastActiveWindow"]["folder"].as_str());
    if let Some(a) = ws["openedWindows"].as_array() {
        uris.extend(a.iter().filter_map(|w| w["folder"].as_str()));
    }
    if let Some(a) = v["backupWorkspaces"]["folders"].as_array() {
        uris.extend(a.iter().filter_map(|f| f["folderUri"].as_str()));
    }
    uris.into_iter().filter_map(file_uri_path).collect()
}

/// Evidence that VS Code or Cursor uses `root` (its ClaudeGate registers the root too, and the
/// roots file holds it once): an `excludes.json[root]` key (the app writes none yet; once it
/// does, only a key it did not write should count), or the folder open in either editor's
/// `storage.json`. Read only; anything unreadable is no evidence.
pub fn vscode_uses_root(home: &Path, root: &Path, storage_files: &[PathBuf]) -> bool {
    let root = resolve_root(root);
    let excludes = std::fs::read_to_string(home.join("excludes.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    if let Some(m) = excludes.as_ref().and_then(|v| v.as_object()) {
        if m.keys().any(|k| resolve_root(Path::new(k)) == root) {
            return true;
        }
    }
    storage_files.iter().any(|f| open_folders(f).iter().any(|p| resolve_root(p) == root))
}

/// Strict view of the roots file: a malformed file is an error, never "no roots".
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
/// 1. resolves it; 2. checks it is an absolute directory; 3+4. appends it to
/// `workspace-roots.json`, applying the nesting guard to the list as read right before the write
/// (a git worktree under a root is refused with a hint); 5. records it as added by the app. If
/// step 5 fails the root is taken back out. An existing root is a no-op (`already`), and a VS
/// Code root never becomes the app's.
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
    let appended = match roots_add(home, &root) {
        Ok(a) => a,
        Err(RootsError::Nested { existing, .. }) => {
            let worktree = crate::worktrees::is_worktree_root(&root);
            return Err(RootsError::Nested { existing, worktree });
        }
        Err(e) => return Err(e),
    };
    if !appended {
        return Ok(Added { root: name, already: true });
    }
    if let Err(e) = cfg.mark_added(&name) {
        let _ = roots_remove(home, &root);
        return Err(RootsError::Io(format!("Couldn't record the project in the app's settings: {e}")));
    }
    Ok(Added { root: name, already: false })
}

/// Removes a project the app added. If VS Code or Cursor also uses the folder
/// (`vscode_uses_root`), only the app's record is dropped and `workspace-roots.json` is left
/// alone, so capture there keeps working; otherwise the root is unregistered too. The session
/// file (the review history) is always kept. A root the app did not add is `NotAppAdded`.
pub fn remove_project(
    home: &Path,
    cfg: &mut impl AppProjects,
    root: &str,
    storage_files: &[PathBuf],
) -> Result<Removed, RootsError> {
    let root = resolve_root(Path::new(root.trim()));
    let name = root_string(&root)?;
    if !cfg.app_roots().iter().any(|r| resolve_root(Path::new(r)) == root) {
        return Err(RootsError::NotAppAdded);
    }
    let settings_err = |e: String| RootsError::Io(format!("Couldn't update the app's settings: {e}"));
    if vscode_uses_root(home, &root, storage_files) {
        cfg.forget(&name).map_err(settings_err)?;
        return Ok(Removed { root: name, unregistered: false });
    }
    roots_remove(home, &root)?;
    if let Err(e) = cfg.forget(&name) {
        // Still recorded as the app's, so put it back rather than leave the two disagreeing.
        let _ = roots_add(home, &root);
        return Err(settings_err(e));
    }
    Ok(Removed { root: name, unregistered: true })
}

/// Re-adds the app's roots that are missing from `workspace-roots.json` (the extension rewrites
/// it without a lock and can drop one). Only folders that still exist are re-added, and none
/// that would now nest under or around another root (checked inside the write). Writes nothing
/// when nothing is missing, so running it on every change of the file cannot loop on its own
/// write. Returns what it re-added.
pub fn ensure_app_roots_present(home: &Path, cfg: &impl AppProjects) -> Result<Vec<String>, RootsError> {
    let mine = cfg.app_roots();
    if mine.is_empty() {
        return Ok(Vec::new());
    }
    let listed = current_roots(home)?;
    let missing: Vec<PathBuf> = mine
        .iter()
        .map(|r| resolve_root(Path::new(r)))
        .filter(|r| r.is_dir() && !listed.iter().any(|l| resolve_root(l) == *r))
        .collect();
    if missing.is_empty() {
        return Ok(Vec::new());
    }
    let added = roots_append(home, &missing)?;
    Ok(added.iter().map(|p| p.to_string_lossy().into_owned()).collect())
}
