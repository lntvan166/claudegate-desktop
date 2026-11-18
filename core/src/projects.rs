//! Add and remove projects: register a folder in `workspace-roots.json` so the hook captures
//! Claude's edits there (spec rule 6, "Roots: append, never rewrite").
//!
//! The app records which roots it added (`projects[root].addedByApp` in its own config, behind
//! `AppProjects`). Only those can be removed, and only those are re-added when the extension's
//! unlocked write drops one. Nothing here writes a file itself: the roots list goes through
//! `store`, the app's record through the caller's `AppProjects`.

use crate::paths::resolve_root;
use crate::roots::{read_roots, RootsError};
use crate::store::{excludes_key, excludes_remove, excludes_set, roots_add, roots_append, roots_remove, update_excludes, ROOTS_FILE};
use std::path::{Path, PathBuf};

/// The app's record of the roots it added.
pub trait AppProjects {
    /// Every root recorded as `addedByApp`, in resolved form.
    fn app_roots(&self) -> Vec<String>;
    /// Records `root` as added by the app.
    fn mark_added(&mut self, root: &str) -> Result<(), String>;
    /// Drops the app's whole record of `root`.
    fn forget(&mut self, root: &str) -> Result<(), String>;
    /// The app's record of `root` (`projects[root]`); the default when there is none.
    fn record(&self, root: &str) -> ProjectRecord;
    /// Records the exclude the app last wrote to `excludes.json[root]` (`excludeWritten`) and
    /// when it changed (`excludeWrittenAt`, epoch ms), moving the value it replaces to
    /// `excludeWrittenPrev`. Recording the same value again keeps the old stamp.
    fn set_exclude_written(&mut self, root: &str, globs: &[String], at_ms: i64) -> Result<(), String>;
    /// Records (`true`) that VS Code took `excludes.json[root]` over (`managedByVsCode`), or
    /// clears it (`false`, Take back).
    fn set_managed_by_vscode(&mut self, root: &str, on: bool) -> Result<(), String>;
}

/// `projects[root]` in the app's config, as far as exclude ownership needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectRecord {
    pub added_by_app: bool,
    /// The project's exclude as the user set it in the app; `None` means the defaults.
    pub exclude: Option<Vec<String>>,
    /// Exactly what the app last wrote to `excludes.json[root]`.
    pub exclude_written: Option<Vec<String>>,
    /// What the app wrote before that (`excludeWrittenPrev`): a stale write-back of it by the
    /// extension's unlocked rewrite is not a takeover.
    pub exclude_written_prev: Option<Vec<String>>,
    /// When `exclude_written` was written (`excludeWrittenAt`, epoch ms). `exclude_written_prev`
    /// counts as the app's only within `STALE_WRITE_BACK_MS` of it.
    pub exclude_written_at: Option<i64>,
    /// VS Code wrote a different `excludes.json[root]`; the app no longer writes that key.
    pub managed_by_vscode: bool,
}

impl ProjectRecord {
    /// The app writes this root's exclude: it added the root and VS Code has not taken it over.
    pub fn owned(&self) -> bool {
        self.added_by_app && !self.managed_by_vscode
    }

    /// The values that count as the app's own in `excludes.json[root]` at `now_ms`: the last
    /// write, and the one before it only within `STALE_WRITE_BACK_MS` of the last write (the
    /// window of the extension's unlocked read-then-write). After that the previous value is
    /// foreign: it is usually the defaults, which is also exactly what a default-configured VS
    /// Code writes when it opens the folder.
    pub fn written_values(&self, now_ms: i64) -> Vec<Vec<String>> {
        let mut v: Vec<Vec<String>> = self.exclude_written.iter().cloned().collect();
        let fresh = self.exclude_written_at.is_some_and(|at| now_ms.saturating_sub(at) <= STALE_WRITE_BACK_MS);
        if fresh {
            v.extend(self.exclude_written_prev.iter().cloned());
        }
        v
    }
}

/// What one sync did to a root.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RootChange {
    /// The app wrote `globs` to `excludes.json[root]`.
    Written { root: String, globs: Vec<String> },
    /// `excludes.json[root]` differs from what the app wrote: VS Code owns it from now on.
    ManagedByVsCode { root: String },
}

/// The exclude an owned root gets: the app's own list if the user set one, else the extension's
/// defaults. Never read back from `excludes.json`, which the app itself writes.
pub fn owned_exclude(rec: &ProjectRecord) -> Vec<String> {
    rec.exclude
        .clone()
        .unwrap_or_else(|| crate::settings::DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect())
}

/// Trimmed, without repeats, in order; refused if a pattern is blank or does not compile with
/// the extension's matcher (the hook would silently ignore it).
pub fn validate_globs(globs: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for g in globs {
        let g = g.trim();
        if g.is_empty() {
            return Err("A pattern is empty.".into());
        }
        if crate::excludes::glob_to_regex(g).is_none() {
            return Err(format!("{g} is not a pattern the matcher understands."));
        }
        if !out.iter().any(|o| o == g) {
            out.push(g.to_string());
        }
    }
    Ok(out)
}

fn store_err(e: crate::store::StoreError) -> String {
    e.to_string()
}

/// The values the app itself wrote to `excludes.json[root]`: the last one and the one before
/// it. The extension rewrites the file without a lock, so a stale copy of the previous value can
/// land just after the app's write; that is still the app's, not a takeover.
fn ours(rec: &ProjectRecord, now_ms: i64) -> Vec<serde_json::Value> {
    rec.written_values(now_ms).iter().map(|w| serde_json::json!(w)).collect()
}

/// How long after its own write the app still reads its previous value as a stale write-back by
/// the extension rather than a takeover.
pub const STALE_WRITE_BACK_MS: i64 = 30_000;

/// One root's step of `sync_excludes`. `None` when nothing changed or the root is not owned.
/// Every key that resolves to the root counts (the extension writes `path.resolve` form, but a
/// differently spelled key reaches the same root in hook.py).
fn sync_root(home: &Path, cfg: &mut impl AppProjects, root: &str, now_ms: i64) -> Result<Option<RootChange>, String> {
    let rec = cfg.record(root);
    if !rec.owned() {
        return Ok(None);
    }
    let resolved = resolve_root(Path::new(root));
    let key = excludes_key(&resolved);
    let want = owned_exclude(&rec);
    let want_v = serde_json::json!(want);
    let mine = ours(&rec, now_ms);
    let mut taken = false;
    let wrote = update_excludes(home, |map| {
        taken = false;
        let keys: Vec<String> = map.keys().filter(|k| resolve_root(Path::new(k)) == resolved).cloned().collect();
        // A value that is neither what we want nor one we wrote: VS Code's.
        if keys.iter().any(|k| map[k] != want_v && !mine.contains(&map[k])) {
            taken = true;
            return Ok(false);
        }
        // Already exactly what we want, under the resolved key only: adopt it (also covers a
        // write whose record was lost).
        if keys.len() == 1 && keys[0] == key && map[&key] == want_v {
            return Ok(false);
        }
        // Absent, or still ours (any spelling): write `want` under the resolved key.
        for k in &keys {
            map.remove(k);
        }
        map.insert(key.clone(), want_v.clone());
        Ok(true)
    })
    .map_err(store_err)?;
    if taken {
        cfg.set_managed_by_vscode(root, true)?;
        return Ok(Some(RootChange::ManagedByVsCode { root: root.to_string() }));
    }
    if wrote || rec.exclude_written.as_ref() != Some(&want) {
        cfg.set_exclude_written(root, &want, now_ms)?;
    }
    Ok(wrote.then(|| RootChange::Written { root: root.to_string(), globs: want }))
}

/// "Take back" for a root VS Code took over: the app owns its exclude again. Clears
/// `managedByVsCode`, writes the app's effective exclude over whatever is there (unconditionally:
/// the key is VS Code's value, which a plain sync would read as a takeover again) and records it.
/// Only for a root the app added. If a VS Code window still has the folder open, the extension
/// overwrites the key on its next start and the next sync marks it managed again.
pub fn take_back_exclude(home: &Path, cfg: &mut impl AppProjects, root: &str) -> Result<RootChange, String> {
    let rec = cfg.record(root);
    if !rec.added_by_app {
        return Err("Only a project added in this app can be taken back.".into());
    }
    let want = owned_exclude(&rec);
    // Recorded before the key is overwritten: if the write then fails, the key still holds VS
    // Code's value, which the next sync reads as a takeover again. Never the reverse (our key
    // on disk while the config still says VS Code's).
    cfg.set_exclude_written(root, &want, crate::time::now_ms())?;
    cfg.set_managed_by_vscode(root, false)?;
    // Every spelling of the root goes, so a value VS Code wrote under another spelling cannot
    // mark it managed again at once.
    excludes_set(home, Path::new(root), &want).map_err(store_err)?;
    Ok(RootChange::Written { root: root.to_string(), globs: want })
}

/// For every root the app added and still owns: when `excludes.json[root]` is absent or still
/// what the app last wrote, write the effective exclude (`owned_exclude`) and record it as
/// `excludeWritten`; when it differs, VS Code has taken the root over: record
/// `managedByVsCode` and never write that key again. Compares before writing, so running it on
/// its own write's watch event writes nothing. Returns what changed.
pub fn sync_excludes(home: &Path, cfg: &mut impl AppProjects) -> Result<Vec<RootChange>, String> {
    sync_excludes_at(home, cfg, crate::time::now_ms())
}

/// `sync_excludes` with the clock injected.
pub fn sync_excludes_at(home: &Path, cfg: &mut impl AppProjects, now_ms: i64) -> Result<Vec<RootChange>, String> {
    let mut out = Vec::new();
    let mut first_err = None;
    for root in cfg.app_roots() {
        match sync_root(home, cfg, &root, now_ms) {
            Ok(Some(c)) => out.push(c),
            Ok(None) => {}
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if out.is_empty() => Err(e),
        _ => Ok(out),
    }
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
/// roots file holds it once): an `excludes.json` key for the root whose value is none of
/// `written`, the values the app itself wrote there (empty: the app wrote nothing, so any key is
/// VS Code's), or the folder open in either editor's `storage.json`. Read only; anything
/// unreadable is no evidence.
pub fn vscode_uses_root(home: &Path, root: &Path, written: &[Vec<String>], storage_files: &[PathBuf]) -> bool {
    let root = resolve_root(root);
    let excludes = std::fs::read_to_string(home.join("excludes.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let written: Vec<serde_json::Value> = written.iter().map(|w| serde_json::json!(w)).collect();
    if let Some(m) = excludes.as_ref().and_then(|v| v.as_object()) {
        if m.iter().any(|(k, v)| resolve_root(Path::new(k)) == root && !written.contains(v)) {
            return true;
        }
    }
    open_in_editor(&root, storage_files)
}

/// The storage half of `vscode_uses_root`: VS Code's or Cursor's `storage.json` lists `root` as
/// an open (or recently open) folder. Read only; anything unreadable is no evidence.
pub fn open_in_editor(root: &Path, storage_files: &[PathBuf]) -> bool {
    let root = resolve_root(root);
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
    // 6. The app took ownership: hand the hook its exclude (the defaults, or VS Code's key if a
    // window already wrote one, which makes the root VS Code's). A failure here keeps the add;
    // the next sync (on any change of excludes.json or the config) retries.
    let _ = sync_root(home, cfg, &name, crate::time::now_ms());
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
    let rec = cfg.record(&name);
    if rec.managed_by_vscode || vscode_uses_root(home, &root, &rec.written_values(crate::time::now_ms()), storage_files) {
        cfg.forget(&name).map_err(settings_err)?;
        return Ok(Removed { root: name, unregistered: false });
    }
    roots_remove(home, &root)?;
    if let Err(e) = cfg.forget(&name) {
        // Still recorded as the app's, so put it back rather than leave the two disagreeing.
        let _ = roots_add(home, &root);
        return Err(settings_err(e));
    }
    // No evidence means the key is absent or still the app's own: take it out with the root. A
    // failure leaves a key the hook never consults (no root routes to it).
    let _ = excludes_remove(home, &root);
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
