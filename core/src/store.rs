//! Session store: a port of the extension's write protocol (`sessionManager.ts`, v1.16.1):
//! `acquireLock`, `releaseLock`, `persist`, `persistLocked` and `atomicWrite`. hook.py and the
//! extension write the same session files concurrently, so the lock path, the timings, the
//! merge-on-write and the temp-file naming all match theirs.
//!
//! Every write first copies the on-disk session to `<desktop_home>/backups/<hash>/`; a failed
//! backup refuses the write. `persist` refuses any path whose parent is not `<home>/sessions`.
//! `decide` takes that backup up front with `preflight`, before it touches any workspace file,
//! and saves each workspace file it is about to overwrite or delete with `FileSnapshots`
//! (`<desktop_home>/backups/<hash>/files/<stamp>/<path>`), so a failed session write can never
//! leave Claude's version (or the user's) only in memory.

use crate::merge::{merge_fresh_captures, MergeGuards};
use crate::model::{parse_session, Session, Shape, Status};
use crate::roots::{check_new_root, RootsError};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A lock older than this (by mtime) is presumed abandoned by a crashed writer and stolen.
const LOCK_STALE_MS: u128 = 3000;
/// How long we wait for the lock before proceeding unlocked (fail-open).
const LOCK_TIMEOUT_MS: u64 = 200;
const LOCK_SLEEP_MS: u64 = 5;
/// Backups kept per session hash.
const BACKUPS_KEPT: usize = 50;

#[derive(Debug, Clone)]
pub struct StorePaths {
    /// The ClaudeGate home (`~/.claudegate`).
    pub home: PathBuf,
    /// The app's own home (`~/.claudegate-desktop`); backups go under it.
    pub desktop_home: PathBuf,
    pub session_file: PathBuf,
}

#[derive(Debug)]
pub enum StoreError {
    Backup(String),
    Write(String),
    /// The session file is not directly under `<home>/sessions`.
    OutsideHome,
    /// The on-disk session has fields this version does not know; a newer writer owns it.
    UnknownShape,
    /// A shared file other than a session (`excludes.json`) could not be updated.
    Shared(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Backup(e) => write!(f, "could not back up the session file: {e}"),
            StoreError::Write(e) => write!(f, "could not write the session file: {e}"),
            StoreError::OutsideHome => {
                write!(f, "refusing to write a session file outside the ClaudeGate sessions dir")
            }
            StoreError::UnknownShape => write!(f, "the session file was written by a newer ClaudeGate"),
            StoreError::Shared(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s: OsString = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// The advisory lock `<session file>.lock`, shared with hook.py and the extension.
/// Released (unlinked) on drop.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    file: Option<File>,
}

impl Drop for Lock {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// Take the lock, stealing a stale one. `None` means proceed without it: the wait timed out,
/// or the lock could not be created for a reason other than it existing (fail-open, as the
/// extension does).
pub(crate) fn acquire_lock(session_file: &Path) -> Option<Lock> {
    let path = with_suffix(session_file, ".lock");
    let deadline = Instant::now() + Duration::from_millis(LOCK_TIMEOUT_MS);
    loop {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let _ = file.write_all(std::process::id().to_string().as_bytes()); // advisory only
                return Some(Lock { path, file: Some(file) });
            }
            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return None,
            Err(_) => {}
        }
        match fs::metadata(&path).and_then(|m| m.modified()) {
            Ok(mtime) => {
                // A future mtime reads as age 0, like the TS's negative age.
                let age = SystemTime::now().duration_since(mtime).map(|d| d.as_millis()).unwrap_or(0);
                if age > LOCK_STALE_MS {
                    // Retry at once only when the stale lock is gone (our unlink, or someone
                    // else's). An unlink that fails (a read-only dir) falls through to the
                    // deadline, or the loop would spin forever.
                    match fs::remove_file(&path) {
                        Ok(()) => continue,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(_) => {}
                    }
                }
            }
            Err(_) => continue, // vanished between open and stat: try to grab it
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(LOCK_SLEEP_MS));
    }
}

fn tmp_name(target: &Path) -> PathBuf {
    let bytes: [u8; 6] = rand::random();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    with_suffix(target, &format!(".{hex}.tmp"))
}

/// Write via `<target>.<12 hex>.tmp` + rename so an interrupted write can't corrupt the target.
/// With `preserve_existing` (a user's working file), a symlink is followed and written through,
/// and the existing file's mode is kept; a new file gets the default mode. Parent dirs are
/// created. On error the tmp is removed.
///
/// Crate-private: the only callers are `persist` (session files) and `workspace_write` (which
/// checks the path is a key of the session being decided). `no_writes` forbids it elsewhere.
pub(crate) fn atomic_write(target: &Path, content: &str, preserve_existing: bool) -> io::Result<()> {
    let mut target = target.to_path_buf();
    let mut perms = None;
    if preserve_existing {
        if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            if let Ok(real) = fs::canonicalize(&target) {
                target = real;
            }
        }
        perms = fs::metadata(&target).ok().map(|m| m.permissions());
    }
    let tmp = tmp_name(&target);
    let result = (|| {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(content.as_bytes())?;
        drop(f);
        if let Some(p) = perms {
            fs::set_permissions(&tmp, p)?;
        }
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// `yyyymmddThhmmss.mmmZ` in UTC.
fn backup_stamp(t: SystemTime) -> String {
    let ms = t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s, milli) = (rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}.{milli:03}Z")
}

/// Sort key of a backup name `<stamp>[-N].json`: `(stamp, N)`, with no suffix as 0. Plain name
/// order would put `…Z-1.json` before `…Z.json` (`-` < `.`) and `-10` before `-9`.
fn backup_key(name: &str) -> Option<(String, u64)> {
    stamp_key(name.strip_suffix(".json")?)
}

/// `(stamp, N)` of a name `<stamp>[-N]`: a backup's stem, or a file-snapshot stamp dir.
fn stamp_key(stem: &str) -> Option<(String, u64)> {
    if stem.len() < 20 || !stem.is_char_boundary(20) {
        return None;
    }
    let (stamp, rest) = stem.split_at(20);
    if !stamp.ends_with('Z') || stamp.as_bytes()[8] != b'T' {
        return None;
    }
    let n = if rest.is_empty() { 0 } else { rest.strip_prefix('-')?.parse().ok()? };
    Some((stamp.to_owned(), n))
}

fn backup_dir(desktop_home: &Path, session_file: &Path) -> Result<PathBuf, StoreError> {
    let hash = session_file
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && !s.starts_with('.'))
        .ok_or_else(|| StoreError::Backup("session file has no name".into()))?;
    Ok(desktop_home.join("backups").join(hash))
}

/// Copy the current session file to `<desktop_home>/backups/<hash>/<stamp>[-N].json` and keep
/// the newest 50. A missing session file needs no backup. Never overwrites an existing backup.
pub(crate) fn backup(desktop_home: &Path, session_file: &Path) -> Result<(), StoreError> {
    let err = |e: io::Error| StoreError::Backup(e.to_string());
    let bytes = match fs::read(session_file) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(err(e)),
    };
    let dir = backup_dir(desktop_home, session_file)?;
    fs::create_dir_all(&dir).map_err(err)?;
    let stamp = backup_stamp(SystemTime::now());
    let mut n = 0u64;
    loop {
        let name = if n == 0 { format!("{stamp}.json") } else { format!("{stamp}-{n}.json") };
        let dest = dir.join(&name);
        match OpenOptions::new().write(true).create_new(true).open(&dest) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(&bytes) {
                    drop(f);
                    let _ = fs::remove_file(&dest);
                    return Err(err(e));
                }
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 10_000 => n += 1,
            Err(e) => return Err(err(e)),
        }
    }
    prune_backups(&dir);
    Ok(())
}

/// Keep the newest `BACKUPS_KEPT`. Best-effort: the backup itself already succeeded.
fn prune_backups(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut all: Vec<((String, u64), PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            Some((backup_key(&name)?, e.path()))
        })
        .collect();
    if all.len() <= BACKUPS_KEPT {
        return;
    }
    all.sort();
    let excess = all.len() - BACKUPS_KEPT;
    for (_, p) in all.into_iter().take(excess) {
        let _ = fs::remove_file(p);
    }
}

fn check_session_path(paths: &StorePaths) -> Result<(), StoreError> {
    let sessions = paths.home.join("sessions");
    if paths.session_file.parent() != Some(sessions.as_path()) || paths.session_file.file_name().is_none() {
        return Err(StoreError::OutsideHome);
    }
    Ok(())
}

/// What `decide` runs once per scope before its first write of any kind: the checks `persist`
/// would make (the session path, a disk copy of known shape), then the session backup. A
/// failure here refuses the decision for that scope with nothing changed. The `persist` that
/// follows is told the backup was taken, so one decision makes one backup.
pub(crate) fn preflight(paths: &StorePaths) -> Result<(), StoreError> {
    check_session_path(paths)?;
    if let Ok(text) = fs::read_to_string(&paths.session_file) {
        if let Ok((_, Shape::Unknown(_))) = parse_session(&text) {
            return Err(StoreError::UnknownShape);
        }
    }
    backup(&paths.desktop_home, &paths.session_file)
}

/// Read-merge-write `mine` under the lock, as the extension's `persist`: re-read the disk copy
/// and merge in what other writers added, set `status`, back up (unless `take_backup` is false
/// because `preflight` already did), then write atomically. Returns the session as written.
/// Refuses a disk copy of unknown shape without writing.
pub(crate) fn persist(
    paths: &StorePaths,
    mine: Session,
    guards: &MergeGuards,
    take_backup: bool,
) -> Result<Session, StoreError> {
    check_session_path(paths)?;
    let _lock = acquire_lock(&paths.session_file);

    // No readable/parseable disk copy: write our own state (never lose it).
    let disk = fs::read_to_string(&paths.session_file).ok().and_then(|t| parse_session(&t).ok());
    let mut session = match disk {
        Some((_, Shape::Unknown(_))) => return Err(StoreError::UnknownShape),
        Some((disk, Shape::Known)) => merge_fresh_captures(mine, disk, guards),
        None => mine,
    };

    session.status = if session.files.is_empty() { Status::Reviewed } else { Status::Active };

    if take_backup {
        backup(&paths.desktop_home, &paths.session_file)?;
    }

    let text = serde_json::to_string_pretty(&session).map_err(|e| StoreError::Write(e.to_string()))?;
    atomic_write(&paths.session_file, &text, false).map_err(|e| StoreError::Write(e.to_string()))?;
    Ok(session)
}

/// Raw-byte copies of the workspace files one decision is about to overwrite or delete, in
/// `<desktop_home>/backups/<hash>/files/<stamp>[-N]/<path relative to the scope root>`. The stamp
/// dir is created on the first save; the newest `BACKUPS_KEPT` stamp dirs are kept per hash.
#[derive(Debug)]
pub(crate) struct FileSnapshots {
    root: PathBuf,
    stamp: String,
    dir: Option<PathBuf>,
}

impl FileSnapshots {
    pub(crate) fn new(desktop_home: &Path, session_file: &Path) -> Result<FileSnapshots, StoreError> {
        let root = backup_dir(desktop_home, session_file)?.join("files");
        Ok(FileSnapshots { root, stamp: backup_stamp(SystemTime::now()), dir: None })
    }

    /// The stamp dir, once something was saved.
    pub(crate) fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    fn ensure_dir(&mut self) -> io::Result<PathBuf> {
        if let Some(d) = &self.dir {
            return Ok(d.clone());
        }
        fs::create_dir_all(&self.root)?;
        let mut n = 0u64;
        let dir = loop {
            let name = if n == 0 { self.stamp.clone() } else { format!("{}-{n}", self.stamp) };
            let d = self.root.join(name);
            match fs::create_dir(&d) {
                Ok(()) => break d,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 10_000 => n += 1,
                Err(e) => return Err(e),
            }
        };
        prune_snapshot_dirs(&self.root, &dir);
        self.dir = Some(dir.clone());
        Ok(dir)
    }

    /// Copy `src`'s current bytes (through a symlink) to `<stamp dir>/<rel>`. `Ok(false)` when
    /// `src` does not exist: there is nothing to lose. `rel` must be relative and made only of
    /// plain names, so it cannot leave the stamp dir.
    pub(crate) fn save(&mut self, rel: &Path, src: &Path) -> io::Result<bool> {
        use std::path::Component;
        if rel.as_os_str().is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unsafe snapshot path {}", rel.display())));
        }
        let bytes = match fs::read(src) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        let dest = self.ensure_dir()?.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = OpenOptions::new().write(true).create_new(true).open(&dest)?;
        if let Err(e) = f.write_all(&bytes).and_then(|()| f.sync_all()) {
            drop(f);
            let _ = fs::remove_file(&dest);
            return Err(e);
        }
        Ok(true)
    }
}

/// Keep the newest `BACKUPS_KEPT` stamp dirs under `root`, never `current`. Best-effort.
fn prune_snapshot_dirs(root: &Path, current: &Path) {
    let Ok(rd) = fs::read_dir(root) else { return };
    let mut all: Vec<((String, u64), PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((stamp_key(&e.file_name().into_string().ok()?)?, e.path())))
        .collect();
    if all.len() <= BACKUPS_KEPT {
        return;
    }
    all.sort();
    let excess = all.len() - BACKUPS_KEPT;
    for (_, p) in all.into_iter().take(excess) {
        if p != current {
            let _ = fs::remove_dir_all(p);
        }
    }
}

/// The hook-log switch the extension also reads: the sentinel's presence is the setting.
pub const HOOKLOG_SENTINEL: &str = "hooklog.enabled";

/// Creates (`on`) or deletes the hook-log sentinel. Guarded to exactly
/// `<claudegate home>/hooklog.enabled`: any other path is refused. Creating is `create_new` (an
/// existing file is fine) and deleting is one unlink (an absent file is fine), so a concurrent
/// toggle by the extension can never leave a half-written file. A missing home is not created.
pub(crate) fn set_sentinel(path: &Path, on: bool) -> io::Result<()> {
    let home = crate::paths::claudegate_home();
    if home.as_os_str().is_empty() || path != home.join(HOOKLOG_SENTINEL) {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refusing to touch anything but the hook-log sentinel"));
    }
    if on {
        if !home.is_dir() {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("{} does not exist", home.display())));
        }
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Turns the hook's logging on or off for every ClaudeGate, the extension included.
pub fn set_hook_log(on: bool) -> io::Result<()> {
    set_sentinel(&crate::paths::claudegate_home().join(HOOKLOG_SENTINEL), on)
}

/// The extension's shared roots list, `<claudegate home>/workspace-roots.json`.
pub const ROOTS_FILE: &str = "workspace-roots.json";
/// Re-reads before giving up when another writer keeps changing the roots file under us.
const ROOTS_ATTEMPTS: usize = 5;

#[cfg(test)]
thread_local! {
    static ROOTS_RACE: std::cell::RefCell<Option<Box<dyn FnMut()>>> = const { std::cell::RefCell::new(None) };
}

/// Test seam: runs between the roots writer's read and its rename, where the extension's own
/// unlocked write can land.
#[cfg(test)]
pub(crate) fn set_roots_race_hook(h: Option<Box<dyn FnMut()>>) {
    ROOTS_RACE.with(|r| *r.borrow_mut() = h);
}

fn roots_race_point() {
    #[cfg(test)]
    ROOTS_RACE.with(|r| {
        if let Some(f) = r.borrow_mut().as_mut() {
            f();
        }
    });
}

fn roots_io(e: impl std::fmt::Display) -> RootsError {
    RootsError::Io(format!("Couldn't update {ROOTS_FILE}: {e}"))
}

/// The file's bytes, `None` when it does not exist.
fn read_roots_raw(target: &Path) -> Result<Option<String>, RootsError> {
    match fs::read_to_string(target) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(roots_io(e)),
    }
}

/// Every entry, verbatim and in order. Missing means `[]`; anything but a JSON array is refused,
/// so a file we cannot read is never overwritten.
fn parse_roots_raw(raw: Option<&str>) -> Result<Vec<serde_json::Value>, RootsError> {
    let Some(raw) = raw else { return Ok(Vec::new()) };
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Array(a)) => Ok(a),
        Ok(_) => Err(roots_io("it is not a JSON list; fix or delete it, then try again")),
        Err(e) => Err(roots_io(format!("it is not valid JSON ({e}); fix or delete it, then try again"))),
    }
}

fn same_root(v: &serde_json::Value, root: &Path) -> bool {
    v.as_str().is_some_and(|s| crate::paths::resolve_root(Path::new(s)) == root)
}

/// Read, edit, write: re-reads immediately before the rename and starts over if the file changed
/// since the read (the extension writes it without a lock), so its entry and ours both survive.
/// `edit` sees the freshly read list on every attempt; it returns `Ok(false)` when there is
/// nothing to change (nothing is written) and an error to refuse. The target is always exactly
/// `<home>/workspace-roots.json`, written atomically as 2-space pretty JSON (the extension's
/// `JSON.stringify(_, null, 2)`).
fn update_roots(
    home: &Path,
    mut edit: impl FnMut(&mut Vec<serde_json::Value>) -> Result<bool, RootsError>,
) -> Result<bool, RootsError> {
    if home.as_os_str().is_empty() || !home.is_absolute() {
        return Err(roots_io("the ClaudeGate folder is not an absolute path"));
    }
    let target = home.join(ROOTS_FILE);
    for _ in 0..ROOTS_ATTEMPTS {
        let raw = read_roots_raw(&target)?;
        let mut list = parse_roots_raw(raw.as_deref())?;
        if !edit(&mut list)? {
            return Ok(false);
        }
        let text = serde_json::to_string_pretty(&serde_json::Value::Array(list)).map_err(roots_io)?;
        roots_race_point();
        if read_roots_raw(&target)? != raw {
            continue;
        }
        atomic_write(&target, &text, true).map_err(roots_io)?;
        return Ok(true);
    }
    Err(roots_io("another program kept changing it; try again"))
}

/// The string entries of a roots list, as paths (for the nesting guard).
fn listed_paths(list: &[serde_json::Value]) -> Vec<PathBuf> {
    list.iter().filter_map(|v| v.as_str().map(PathBuf::from)).collect()
}

/// Appends each of `roots` (resolved) that is not already listed and passes the nesting guard
/// against the freshly read list (others are skipped). Every other entry is kept, unchanged and
/// in order, including roots that no longer exist: pruning those is the extension's job.
/// Returns the roots appended.
pub(crate) fn roots_append(home: &Path, roots: &[PathBuf]) -> Result<Vec<PathBuf>, RootsError> {
    let roots: Vec<PathBuf> = roots.iter().map(|r| crate::paths::resolve_root(r)).collect();
    let mut appended = Vec::new();
    update_roots(home, |list| {
        appended.clear();
        for r in &roots {
            if list.iter().any(|v| same_root(v, r)) || check_new_root(&listed_paths(list), r).is_err() {
                continue;
            }
            list.push(serde_json::Value::String(r.to_string_lossy().into_owned()));
            appended.push(r.clone());
        }
        Ok(!appended.is_empty())
    })?;
    Ok(appended)
}

/// Appends one root after the nesting guard, run against the list as read right before the
/// write. `Ok(false)`: it was already listed, nothing written.
pub(crate) fn roots_add(home: &Path, root: &Path) -> Result<bool, RootsError> {
    let root = crate::paths::resolve_root(root);
    update_roots(home, |list| {
        if list.iter().any(|v| same_root(v, &root)) {
            return Ok(false);
        }
        check_new_root(&listed_paths(list), &root)?;
        list.push(serde_json::Value::String(root.to_string_lossy().into_owned()));
        Ok(true)
    })
}

/// Removes `root` (every entry that resolves to it) and nothing else. A root that is not listed
/// writes nothing. Callers check the app added it first.
pub(crate) fn roots_remove(home: &Path, root: &Path) -> Result<(), RootsError> {
    let root = crate::paths::resolve_root(root);
    update_roots(home, |list| {
        let before = list.len();
        list.retain(|v| !same_root(v, &root));
        Ok(list.len() != before)
    })
    .map(|_| ())
}

/// The extension's shared exclude list, `<claudegate home>/excludes.json`:
/// `{ "<resolved root>": ["glob", ...] }`. hook.py reads it to skip capture.
pub const EXCLUDES_FILE: &str = "excludes.json";

fn excludes_io(e: impl std::fmt::Display) -> StoreError {
    StoreError::Shared(format!("Couldn't update {EXCLUDES_FILE}: {e}"))
}

/// Every key, as found. Missing means `{}`; anything but a JSON object is refused, so a file we
/// cannot read is never overwritten.
fn parse_excludes_raw(raw: Option<&str>) -> Result<serde_json::Map<String, serde_json::Value>, StoreError> {
    let Some(raw) = raw else { return Ok(serde_json::Map::new()) };
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        Ok(_) => Err(excludes_io("it is not a JSON object; fix or delete it, then try again")),
        Err(e) => Err(excludes_io(format!("it is not valid JSON ({e}); fix or delete it, then try again"))),
    }
}

/// Read, edit, write `<home>/excludes.json`, the same protocol as the roots file: re-reads right
/// before the rename and starts over if another window wrote it since our read. `edit` returns
/// `Ok(false)` when there is nothing to change, and then nothing is written. Every key `edit`
/// leaves alone is kept as a value (key order follows the JSON map's, sorted). Written
/// atomically as 2-space pretty JSON, like the extension's `JSON.stringify(_, null, 2)`.
pub(crate) fn update_excludes(
    home: &Path,
    mut edit: impl FnMut(&mut serde_json::Map<String, serde_json::Value>) -> Result<bool, StoreError>,
) -> Result<bool, StoreError> {
    if home.as_os_str().is_empty() || !home.is_absolute() {
        return Err(excludes_io("the ClaudeGate folder is not an absolute path"));
    }
    let target = home.join(EXCLUDES_FILE);
    let read = |t: &Path| match fs::read_to_string(t) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(excludes_io(e)),
    };
    for _ in 0..ROOTS_ATTEMPTS {
        let raw = read(&target)?;
        let mut map = parse_excludes_raw(raw.as_deref())?;
        if !edit(&mut map)? {
            return Ok(false);
        }
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(map)).map_err(excludes_io)?;
        roots_race_point();
        if read(&target)? != raw {
            continue;
        }
        atomic_write(&target, &text, true).map_err(excludes_io)?;
        return Ok(true);
    }
    Err(excludes_io("another program kept changing it; try again"))
}

/// The key the extension uses for `root` (`path.resolve`).
pub(crate) fn excludes_key(root: &Path) -> String {
    crate::paths::resolve_root(root).to_string_lossy().into_owned()
}

/// Sets `excludes.json[root]` to `globs`, keeping every other key. Writes nothing when the value
/// is already that. Unconditional, so only Take back uses it: the app's own sync goes through
/// `update_excludes` so it can refuse a key VS Code has taken over in the same read.
pub(crate) fn excludes_set(home: &Path, root: &Path, globs: &[String]) -> Result<(), StoreError> {
    let key = excludes_key(root);
    let want = serde_json::json!(globs);
    update_excludes(home, |map| {
        if map.get(&key) == Some(&want) {
            return Ok(false);
        }
        map.insert(key.clone(), want.clone());
        Ok(true)
    })
    .map(|_| ())
}

/// Deletes `excludes.json[root]` and nothing else. Writes nothing when it is not there.
pub(crate) fn excludes_remove(home: &Path, root: &Path) -> Result<(), StoreError> {
    let key = excludes_key(root);
    update_excludes(home, |map| Ok(map.remove(&key).is_some())).map(|_| ())
}

#[path = "../tests/unit/excludes_write.rs"]
#[cfg(test)]
mod excludes_write_tests;

#[path = "../tests/unit/roots_write.rs"]
#[cfg(test)]
mod roots_write_tests;

#[path = "../tests/unit/store.rs"]
#[cfg(test)]
mod store_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmp_name_is_twelve_lowercase_hex() {
        let t = tmp_name(Path::new("/s/abc.json"));
        let s = t.to_str().unwrap();
        let hex = s.strip_prefix("/s/abc.json.").unwrap().strip_suffix(".tmp").unwrap();
        assert_eq!(hex.len(), 12);
        assert!(hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)), "{hex}");
    }

    #[test]
    fn stamp_is_utc_with_millis() {
        let t = UNIX_EPOCH + Duration::from_millis(1_794_787_200_123); // 2026-11-16T00:00:00.123Z
        assert_eq!(backup_stamp(t), "20261116T000000.123Z");
        assert_eq!(backup_stamp(UNIX_EPOCH), "19700101T000000.000Z");
        let leap = UNIX_EPOCH + Duration::from_millis(951_782_399_999); // 2000-02-28T23:59:59.999Z
        assert_eq!(backup_stamp(leap), "20000228T235959.999Z");
    }

    #[test]
    fn backup_key_orders_suffixes_after_the_plain_name() {
        let mut v =
            vec!["20261116T000000.000Z-10.json", "20261116T000000.000Z-9.json", "20261116T000000.000Z.json"];
        v.sort_by_key(|n| backup_key(n).unwrap());
        assert_eq!(v, ["20261116T000000.000Z.json", "20261116T000000.000Z-9.json", "20261116T000000.000Z-10.json"]);
        assert!(backup_key("notes.json").is_none());
    }

    // Moved from tests/store.rs when atomic_write became crate-private.
    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_mode_and_follows_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real.sh");
        fs::write(&real, "old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o755)).unwrap();
        let link = tmp.path().join("link.sh");
        symlink(&real, &link).unwrap();

        atomic_write(&link, "new", true).unwrap();

        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "link survives");
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        assert_eq!(fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o755);

        // A new file gets its parent dirs and the default mode.
        let fresh = tmp.path().join("a/b/new.txt");
        atomic_write(&fresh, "x", true).unwrap();
        assert_eq!(fs::read_to_string(&fresh).unwrap(), "x");
        let names: Vec<_> = fs::read_dir(tmp.path()).unwrap().flatten().map(|d| d.file_name()).collect();
        assert!(names.iter().all(|n| !n.to_string_lossy().ends_with(".tmp")), "{names:?}");
    }

    #[test]
    fn set_sentinel_refuses_any_other_path() {
        let tmp = tempfile::tempdir().unwrap();
        // Same name, wrong folder: never the ClaudeGate home, whatever CLAUDEGATE_HOME says.
        let elsewhere = tmp.path().join("hooklog.enabled");
        assert!(set_sentinel(&elsewhere, true).is_err());
        assert!(!elsewhere.exists());
        // Right folder, wrong name.
        assert!(set_sentinel(&crate::paths::claudegate_home().join("hooklog.enabledx"), true).is_err());
        assert!(set_sentinel(&crate::paths::claudegate_home().join("sessions"), false).is_err());
    }

    #[test]
    fn atomic_write_leaves_no_tmp_on_error() {
        let tmp = tempfile::tempdir().unwrap();
        // The target is a non-empty directory: the rename onto it fails after the tmp is written.
        let target = tmp.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "k").unwrap();
        assert!(atomic_write(&target, "content", false).is_err());
        let left: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .flatten()
            .map(|d| d.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "tmp left behind: {left:?}");
    }
}
