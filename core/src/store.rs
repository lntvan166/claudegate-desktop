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
    /// The session file on disk could not be read, decoded as UTF-8 or parsed under the lock:
    /// writing ours over it could destroy what another writer put there.
    Unreadable(String),
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
            StoreError::Unreadable(e) => write!(f, "the session file on disk could not be read, so it was left as is: {e}"),
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
/// The temp file is synced before the rename, so a crash leaves the old file or the new one.
///
/// Crate-private, and `no_writes` forbids it (and `atomic_write_bytes`) outside `store.rs` and
/// `workspace_write.rs`. Callers: `persist` (session files), `workspace_write` (which checks the
/// path is a key of the session being decided), the roots and excludes writers, and capture
/// setup (`install_capture`).
pub(crate) fn atomic_write(target: &Path, content: &str, preserve_existing: bool) -> io::Result<()> {
    atomic_write_bytes(target, content.as_bytes(), preserve_existing)
}

/// `atomic_write` for bytes (a settings.json restored from its backup byte for byte).
pub(crate) fn atomic_write_bytes(target: &Path, content: &[u8], preserve_existing: bool) -> io::Result<()> {
    atomic_write_checked(target, content, preserve_existing, || Ok(()))
}

/// `atomic_write_bytes` that runs `check` after the temp file is written and synced and
/// immediately before the rename; an error from `check` abandons the write. The settings writer
/// uses it to compare the target with what it read as late as possible: what remains is the
/// window between that compare and the rename syscall itself.
fn atomic_write_checked(
    target: &Path,
    content: &[u8],
    preserve_existing: bool,
    check: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
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
        f.write_all(content)?;
        f.sync_all()?;
        drop(f);
        if let Some(p) = perms {
            fs::set_permissions(&tmp, p)?;
        }
        check()?;
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
    if let Some((_, Shape::Unknown(_))) = read_disk_strict(&paths.session_file)? {
        return Err(StoreError::UnknownShape);
    }
    backup(&paths.desktop_home, &paths.session_file)
}

/// The session file as `persist` and `preflight` read it: `None` only when it does not exist;
/// any other read error, invalid UTF-8 or a parse error refuses (`Unreadable`).
fn read_disk_strict(file: &Path) -> Result<Option<(Session, Shape)>, StoreError> {
    match fs::read(file) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes).map_err(|e| StoreError::Unreadable(e.to_string()))?;
            Ok(Some(parse_session(&text).map_err(|e| StoreError::Unreadable(e.to_string()))?))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(StoreError::Unreadable(e.to_string())),
    }
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

    // Only a missing file means there is no disk copy to merge. Anything we cannot read, decode
    // or parse is refused, never overwritten with our state.
    let disk = read_disk_strict(&paths.session_file)?;
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

/// Sets `excludes.json[root]` to `globs` under the resolved key, dropping any other spelling of
/// the same root and keeping every other key. Writes nothing when the resolved key alone already
/// holds that value. Unconditional, so only Take back uses it: the app's own sync goes through
/// `update_excludes` so it can refuse a key VS Code has taken over in the same read.
pub(crate) fn excludes_set(home: &Path, root: &Path, globs: &[String]) -> Result<(), StoreError> {
    let resolved = crate::paths::resolve_root(root);
    let key = excludes_key(&resolved);
    let want = serde_json::json!(globs);
    update_excludes(home, |map| {
        let others: Vec<String> = map
            .keys()
            .filter(|k| **k != key && crate::paths::resolve_root(Path::new(k)) == resolved)
            .cloned()
            .collect();
        if others.is_empty() && map.get(&key) == Some(&want) {
            return Ok(false);
        }
        for k in others {
            map.remove(&k);
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

// ---- Capture setup -------------------------------------------------------------------------
//
// A port of the extension's `HookInstaller.setup` (`hookInstaller.ts` v1.16.1) for a machine
// without the extension: the hook files, a marker, then `registerHookInSettings` and its guarded
// write. `~/.claude/settings.json` is the user's whole Claude Code configuration, so:
//
// - it is written only at the realpath of `<HOME>/.claude/settings.json` (a dotfiles symlink
//   keeps its link), and only when it reads as a plain JSON object;
// - a timestamped copy goes beside the literal path first (`settings.json.claudegate-<ISO>.bak`,
//   the extension's name and its 5-copy ring), and a failed copy refuses the write;
// - the result is re-read and checked with `verify_settings_content`; anything wrong restores the
//   copy byte for byte.
//
// Any failure after the first write rolls back what this call wrote and nothing else: a file is
// removed only if this call created it and it still holds the bytes we wrote; a replaced file (the
// marker, or an adopted hook.py) gets its old bytes back while it still holds ours; a folder is
// removed only if this call created it and it is empty.

use crate::capture::{
    backups_to_prune, capture_status, compute_settings_patch, verify_settings_content, CaptureStatus, InstallError,
    InstallReport, HOOK_PY_FILE, HOOK_SH_CONTENT, HOOK_SH_FILE, MARKER_FILE, SETTINGS_BACKUP_RETENTION,
};

#[cfg(test)]
thread_local! {
    static INSTALL_FAULT: std::cell::RefCell<Option<Box<dyn FnMut(&str) -> io::Result<()>>>> = const { std::cell::RefCell::new(None) };
}

/// Test seam: called at each install stage (`before_hook_py`, `after_hooks`, `before_backup`,
/// `before_write`, `after_settings_write`, `before_undo`); an error aborts the install there.
#[cfg(test)]
pub(crate) fn set_install_fault(f: Option<Box<dyn FnMut(&str) -> io::Result<()>>>) {
    INSTALL_FAULT.with(|r| *r.borrow_mut() = f);
}

#[cfg(test)]
thread_local! {
    static INSTALL_CORRUPT: std::cell::RefCell<Option<Box<dyn FnMut(&mut Vec<u8>)>>> = const { std::cell::RefCell::new(None) };
}

/// Test seam: rewrites the settings bytes just before we write them, to stand for a write of
/// ours that lands wrong content.
#[cfg(test)]
pub(crate) fn set_install_corrupt(f: Option<Box<dyn FnMut(&mut Vec<u8>)>>) {
    INSTALL_CORRUPT.with(|r| *r.borrow_mut() = f);
}

fn corrupt_point(_bytes: &mut Vec<u8>) {
    #[cfg(test)]
    INSTALL_CORRUPT.with(|r| {
        if let Some(f) = r.borrow_mut().as_mut() {
            f(_bytes);
        }
    });
}

fn install_fault(_stage: &str) -> io::Result<()> {
    #[cfg(test)]
    return INSTALL_FAULT.with(|r| r.borrow_mut().as_mut().map_or(Ok(()), |f| f(_stage)));
    #[cfg(not(test))]
    Ok(())
}

fn refused(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, format!("refusing to write {what}"))
}

/// The hook files may only be `<claudegate home>/{hook.py,hook.sh,.installed-by-desktop}`.
pub(crate) fn guard_hook_file(claudegate_home: &Path, path: &Path) -> io::Result<()> {
    let ok = path.parent() == Some(claudegate_home)
        && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| [HOOK_PY_FILE, HOOK_SH_FILE, MARKER_FILE].contains(&n));
    if ok { Ok(()) } else { Err(refused(&path.display().to_string())) }
}

fn settings_literal(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

/// Where a settings write must land: the realpath of `<home>/.claude/settings.json`, or that
/// literal path when nothing is there yet. A dangling symlink has no safe answer.
fn settings_target(home: &Path) -> io::Result<PathBuf> {
    let literal = settings_literal(home);
    match fs::symlink_metadata(&literal) {
        // Nothing there yet: the path the new file's realpath will have, from the realpath of the
        // nearest existing folder (`~/.claude`, or HOME itself), so a symlinked HOME or ~/.claude
        // names the same file before and after it exists.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let claude = home.join(".claude");
            match fs::canonicalize(&claude) {
                Ok(dir) => Ok(dir.join("settings.json")),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    if fs::symlink_metadata(&claude).is_ok() {
                        return Err(io::Error::new(e.kind(), format!("{} is a link to nothing", claude.display())));
                    }
                    Ok(fs::canonicalize(home)?.join(".claude").join("settings.json"))
                }
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
        Ok(_) => fs::canonicalize(&literal).map_err(|e| {
            io::Error::new(e.kind(), format!("{} is a link to nothing ({e})", literal.display()))
        }),
    }
}

/// A settings write may only target `settings_target(home)`.
pub(crate) fn guard_settings_target(home: &Path, path: &Path) -> io::Result<()> {
    if settings_target(home)? == path { Ok(()) } else { Err(refused(&path.display().to_string())) }
}

/// `new Date().toISOString().replace(/:/g, "-")`: `2026-11-18T10-20-30.123Z`.
fn iso_backup_stamp(t: SystemTime) -> String {
    let ms = t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s, milli) = (rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}-{mi:02}-{s:02}.{milli:03}Z")
}

#[cfg(unix)]
fn mode_of(p: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).ok().map(|m| m.permissions().mode() & 0o777)
}

/// Create `path` with `content`, never replacing anything: the bytes go to a temp file first and
/// are published with a hard link, which fails if the name exists. Where hard links are not
/// supported, falls back to `create_new` on the path itself.
#[cfg(unix)]
fn create_file_exclusive(path: &Path, content: &[u8], mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tmp_name(path);
    let staged = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        drop(f);
        if let Some(m) = mode {
            fs::set_permissions(&tmp, fs::Permissions::from_mode(m))?;
        }
        fs::hard_link(&tmp, path)
    })();
    let _ = fs::remove_file(&tmp);
    match staged {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists && fs::symlink_metadata(path).is_err() => {
            let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
            let written = f.write_all(content).and_then(|()| f.sync_all());
            drop(f);
            if let Err(e) = written.and_then(|()| match mode {
                Some(m) => fs::set_permissions(path, fs::Permissions::from_mode(m)),
                None => Ok(()),
            }) {
                let _ = fs::remove_file(path);
                return Err(e);
            }
            Ok(())
        }
        other => other,
    }
}

/// What to put back if the install fails.
#[derive(Default)]
struct Undo {
    /// Files this call created, with the bytes it wrote.
    created: Vec<(PathBuf, Vec<u8>)>,
    /// Files this call replaced: the path, its old bytes, and the bytes we wrote. The old bytes go
    /// back only while the file still holds ours.
    replaced: Vec<(PathBuf, Vec<u8>, Vec<u8>)>,
    /// Folders this call created (removed only while empty).
    dirs: Vec<PathBuf>,
    /// Our settings write, once it happened, while it is still ours to undo.
    settings: Option<SettingsUndo>,
    /// The settings file, once we wrote it. From then on the hook files go only when that file has
    /// no ClaudeGate entry (`hook_files_may_go`), whether or not its own undo succeeded.
    written: Option<PathBuf>,
}

struct SettingsUndo {
    home: PathBuf,
    target: PathBuf,
    /// The pre-setup copy; `None`: there was no file, so undoing ours removes it.
    backup: Option<PathBuf>,
    /// The exact bytes we wrote. The file is restored or removed only while it still holds them:
    /// anything else is another program's write (Claude Code rewrites this file often), and
    /// putting the backup over it would destroy that change.
    ours: Vec<u8>,
}

fn concurrent_note(backup: Option<&Path>) -> String {
    let copy = match backup {
        Some(b) => format!("Your pre-setup copy is {}.", b.display()),
        None => "There was no settings.json before setup.".to_owned(),
    };
    format!("settings.json was changed by another program right after setup; it was left as is. {copy}")
}

/// What the settings file says about capture, by the status's own test.
enum OnDisk {
    /// A ClaudeGate entry is registered (any spelling `capture_status` counts).
    Registered,
    /// Readable, and no entry.
    NotRegistered,
    /// The file is gone.
    Absent,
    /// Unreadable or unparseable: we cannot tell.
    Unknown(String),
}

fn registration_of(target: &Path) -> OnDisk {
    match fs::read(target) {
        Ok(b) => match capture_status(Some(&String::from_utf8_lossy(&b)), true, true) {
            CaptureStatus::SetUpByApp { .. } | CaptureStatus::SetUpByExtension { .. } => OnDisk::Registered,
            CaptureStatus::NotSetUp => OnDisk::NotRegistered,
            CaptureStatus::SettingsUnreadable(r) => OnDisk::Unknown(r),
            CaptureStatus::PythonMissing => OnDisk::Unknown("unexpected status".into()),
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => OnDisk::Absent,
        Err(e) => OnDisk::Unknown(format!("could not read it: {e}")),
    }
}

/// The one rule for removing hook files once settings.json was written: only when the file now
/// has no ClaudeGate entry (or is gone). `Err` says why they must stay, so a registered entry is
/// never left pointing at a deleted hook.
fn hook_files_may_go(target: &Path) -> Result<(), String> {
    match registration_of(target) {
        OnDisk::NotRegistered | OnDisk::Absent => Ok(()),
        OnDisk::Registered => {
            Err("the hook files and marker were kept, because settings.json still has a ClaudeGate entry".into())
        }
        OnDisk::Unknown(r) => Err(format!(
            "the hook files and marker were kept, because settings.json could not be checked for a ClaudeGate entry ({r})"
        )),
    }
}

impl Undo {
    /// Best-effort, settings first. Returns what could not be put back, or was left alone.
    fn run(self) -> Option<String> {
        let mut problems = Vec::new();
        if self.written.is_some() {
            let _ = install_fault("before_undo");
        }
        if let Some(su) = self.settings {
            // The "still ours?" compare runs right before the restore's rename (or the unlink).
            let concurrent = std::cell::Cell::new(false);
            let still_ours = || -> io::Result<()> {
                match fs::read(&su.target) {
                    Ok(now) if now == su.ours => Ok(()),
                    Ok(_) => {
                        concurrent.set(true);
                        Err(io::Error::other("settings.json holds another program's write"))
                    }
                    Err(e) => Err(io::Error::new(e.kind(), format!("could not read it: {e}"))),
                }
            };
            let res = guard_settings_target(&su.home, &su.target).and_then(|()| match &su.backup {
                Some(b) => fs::read(b)
                    .map_err(|e| io::Error::new(e.kind(), format!("could not read the backup: {e}")))
                    .and_then(|bytes| atomic_write_checked(&su.target, &bytes, true, &still_ours)),
                None => match fs::symlink_metadata(&su.target) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                    _ => still_ours().and_then(|()| fs::remove_file(&su.target)),
                },
            });
            match res {
                Ok(()) => {}
                Err(_) if concurrent.get() => problems.push(concurrent_note(su.backup.as_deref())),
                Err(e) => {
                    let from =
                        su.backup.map(|b| format!("; your previous settings are in {}", b.display())).unwrap_or_default();
                    problems.push(format!("could not restore {}: {e}{from}", su.target.display()));
                }
            }
        }
        let files_may_go = match &self.written {
            Some(target) => hook_files_may_go(target),
            None => Ok(()),
        };
        match files_may_go {
            Ok(()) => {
                for (p, ours) in self.created.into_iter().rev() {
                    if fs::read(&p).is_ok_and(|now| now == ours) {
                        if let Err(e) = fs::remove_file(&p) {
                            problems.push(format!("could not remove {}: {e}", p.display()));
                        }
                    }
                }
                for (p, old, ours) in self.replaced {
                    // A plain file holding other bytes is someone else's write since: left alone,
                    // like a created file. Anything we cannot check (a failed stat or read, or no
                    // longer a plain file) is left too, but reported.
                    let still_ours = || match (fs::symlink_metadata(&p), fs::read(&p)) {
                        (Ok(m), Ok(now)) if m.file_type().is_file() && now == ours => Ok(()),
                        _ => Err(io::Error::other("changed since")),
                    };
                    match (fs::symlink_metadata(&p), fs::read(&p)) {
                        (Ok(m), Ok(now)) if m.file_type().is_file() => {
                            if now != ours {
                                continue;
                            }
                        }
                        (Ok(_), Ok(_)) => {
                            problems.push(format!("{} was not restored: it is no longer a regular file", p.display()));
                            continue;
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            problems.push(format!("{} was not restored: could not read it: {e}", p.display()));
                            continue;
                        }
                    }
                    if let Err(e) = atomic_write_checked(&p, &old, true, still_ours) {
                        if fs::read(&p).is_ok_and(|now| now == ours) {
                            problems.push(format!("could not restore {}: {e}", p.display()));
                        }
                    }
                }
                for d in self.dirs.into_iter().rev() {
                    let _ = fs::remove_dir(d); // only succeeds while empty
                }
            }
            Err(kept) => problems.push(kept),
        }
        (!problems.is_empty()).then(|| problems.join("; "))
    }
}

fn failed(undo: Undo, reason: impl std::fmt::Display) -> InstallError {
    InstallError::Failed { reason: reason.to_string(), rollback: undo.run() }
}

/// Keep the newest `SETTINGS_BACKUP_RETENTION` of `<literal>.claudegate-*.bak` (best-effort).
fn prune_settings_backups(literal: &Path) {
    if let (Some(dir), Some(base)) = (literal.parent(), literal.file_name().and_then(|b| b.to_str())) {
        if let Ok(rd) = fs::read_dir(dir) {
            let names: Vec<String> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
            for name in backups_to_prune(&names, base, SETTINGS_BACKUP_RETENTION) {
                let _ = fs::remove_file(dir.join(name));
            }
        }
    }
}

/// Copy `bytes` (the settings as read) to `<literal>.claudegate-<stamp>[_n].bak` with the
/// original's mode. Pruning the ring waits until the settings write has
/// verified (`prune_settings_backups`), so a failed setup never costs an older copy.
#[cfg(unix)]
fn backup_settings(literal: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    use std::os::unix::fs::OpenOptionsExt;
    let stamp = iso_backup_stamp(SystemTime::now());
    let mode = mode_of(literal).unwrap_or(0o600);
    let mut n = 0u32;
    let path = loop {
        let candidate =
            if n == 0 { with_suffix(literal, &format!(".claudegate-{stamp}.bak")) } else { with_suffix(literal, &format!(".claudegate-{stamp}_{n}.bak")) };
        match OpenOptions::new().write(true).create_new(true).mode(mode).open(&candidate) {
            Ok(mut f) => {
                if let Err(e) = f.write_all(bytes).and_then(|()| f.sync_all()) {
                    drop(f);
                    let _ = fs::remove_file(&candidate);
                    return Err(e);
                }
                break candidate;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 10_000 => n += 1,
            Err(e) => return Err(e),
        }
    };
    Ok(path)
}

fn create_dir_tracked(dir: &Path, undo: &mut Undo) -> io::Result<()> {
    match fs::create_dir(dir) {
        Ok(()) => {
            undo.dirs.push(dir.to_path_buf());
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => Ok(()),
        Err(e) => Err(e),
    }
}

/// Sets capture up under `home` (the user's HOME, injectable for tests). Order:
/// 1. refuse unless read-write, the bundled hook is stamped, and `capture_status` is exactly
///    `NotSetUp` (python3 present, settings strict JSON object or absent, no claudegate entry);
///    refuse an existing hook.py unless it is a plain file `adoptable_hook` accepts, and a
///    hook.sh that is not the extension's wrapper;
/// 2. adopt that hook.py (replace it with the bundled hook, only while it holds the bytes read),
///    or create hook.py; create hook.sh (0755) without replacing anything; then the marker;
/// 3. back up settings.json (refusing if it changed since it was read);
/// 4. re-compare with what was read, then write `compute_settings_patch`'s content atomically
///    to the realpath;
/// 5. re-read: our bytes are verified with `verify_settings_content` (restored on failure);
///    anyone else's are left alone, and setup holds only if our entry survived in them;
/// 6. prune the backup ring. On any failure from step 2 on, roll back what this call wrote.
///
/// The wrapper runs `$HOME/.claudegate/hook.py`, so the hook files go to `<home>/.claudegate`
/// whatever `CLAUDEGATE_HOME` says.
pub(crate) fn install_capture(
    home: &Path,
    bundled_hook: &str,
    python3: bool,
    read_only: bool,
) -> Result<InstallReport, InstallError> {
    if read_only {
        return Err(InstallError::ReadOnly);
    }
    if !cfg!(unix) {
        return Err(InstallError::Unsupported);
    }
    let version = crate::capture::bundled_hook_version(bundled_hook).ok_or(InstallError::BundledHook)?;
    if !home.is_absolute() || !home.is_dir() {
        return Err(InstallError::Failed { reason: format!("{} is not a home folder", home.display()), rollback: None });
    }
    install_unix(home, bundled_hook, &version, python3)
}

#[cfg(not(unix))]
fn install_unix(_: &Path, _: &str, _: &str, _: bool) -> Result<InstallReport, InstallError> {
    Err(InstallError::Unsupported)
}

#[cfg(unix)]
fn install_unix(home: &Path, bundled_hook: &str, version: &str, python3: bool) -> Result<InstallReport, InstallError> {
    let unreadable = |e: io::Error| InstallError::SettingsUnreadable(e.to_string());
    let cg = home.join(".claudegate");
    let (py, sh, marker) = (cg.join(HOOK_PY_FILE), cg.join(HOOK_SH_FILE), cg.join(MARKER_FILE));
    let claude_dir = home.join(".claude");
    let literal = settings_literal(home);
    let hook_command = sh
        .to_str()
        .ok_or_else(|| InstallError::Failed { reason: "the home folder's path is not valid UTF-8".into(), rollback: None })?
        .to_owned();

    // 1. Read and decide. Only "not found" means a fresh install; any other read error refuses.
    let target = settings_target(home).map_err(unreadable)?;
    let raw: Option<String> = match fs::read(&target) {
        Ok(b) => Some(String::from_utf8(b).map_err(|_| InstallError::SettingsUnreadable("not UTF-8 text".into()))?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(unreadable(e)),
    };
    let ours = crate::capture::app_owns_hook(fs::symlink_metadata(&marker).is_ok(), fs::read(&py).ok().as_deref());
    match capture_status(raw.as_deref(), ours, python3) {
        CaptureStatus::NotSetUp => {}
        CaptureStatus::PythonMissing => return Err(InstallError::PythonMissing),
        CaptureStatus::SettingsUnreadable(r) => return Err(InstallError::SettingsUnreadable(r)),
        other => return Err(InstallError::AlreadySetUp(other)),
    }
    let patch = compute_settings_patch(raw.as_deref().unwrap_or(""), &hook_command);
    if patch.parse_error {
        return Err(InstallError::SettingsUnreadable("not a JSON object".into()));
    }
    if let Some(why) = patch.shape_error {
        return Err(InstallError::SettingsUnreadable(why));
    }
    if !patch.changed {
        return Err(InstallError::AlreadySetUp(CaptureStatus::NotSetUp));
    }
    let leftover = |marker_there: bool| if marker_there { InstallError::HookFilesFromApp } else { InstallError::HookFilesExist };
    // An existing hook.py is refused unless it is a plain file holding exactly a form setup may
    // adopt (`adoptable_hook`): the bundled hook, or the extension's copy of it (most often left
    // behind when the extension was uninstalled). Those bytes are what the replace compares
    // against right before it writes.
    let adopt: Option<Vec<u8>> = match fs::symlink_metadata(&py) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Ok(m) if m.file_type().is_file() => match fs::read(&py) {
            Ok(b) if crate::capture::adoptable_hook(&b, bundled_hook) => Some(b),
            _ => return Err(leftover(fs::symlink_metadata(&marker).is_ok())),
        },
        _ => return Err(leftover(fs::symlink_metadata(&marker).is_ok())),
    };
    let write_sh = match fs::read(&sh) {
        Ok(b) if b == HOOK_SH_CONTENT.as_bytes() => false,
        Ok(_) => return Err(InstallError::ForeignWrapper),
        Err(e) if e.kind() == io::ErrorKind::NotFound && fs::symlink_metadata(&sh).is_err() => true,
        Err(_) => return Err(InstallError::ForeignWrapper),
    };
    // The marker is replaced in place and restored on rollback; only a regular file can be.
    match fs::symlink_metadata(&marker) {
        Ok(m) if !m.file_type().is_file() => return Err(InstallError::MarkerNotFile),
        _ => {}
    }
    let old_marker = match fs::read(&marker) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(InstallError::Failed { reason: format!("could not read {}: {e}", marker.display()), rollback: None }),
    };

    let mut undo = Undo::default();

    // 2. Hook files, then the marker.
    if let Err(e) = create_dir_tracked(&cg, &mut undo) {
        return Err(failed(undo, format!("could not create {}: {e}", cg.display())));
    }
    if let Err(e) = install_fault("before_hook_py") {
        return Err(failed(undo, e));
    }
    if let Some(old) = &adopt {
        // Replace the leftover, but only while it still holds the bytes we decided on: the compare
        // runs after the temp file is synced, right before the rename. Any change since is
        // someone else's write, refused like any other existing hook.py.
        let changed = std::cell::Cell::new(false);
        let unchanged = || match (fs::symlink_metadata(&py), fs::read(&py)) {
            (Ok(m), Ok(now)) if m.file_type().is_file() && &now == old => Ok(()),
            _ => {
                changed.set(true);
                Err(io::Error::other("hook.py changed while setting up"))
            }
        };
        let res = guard_hook_file(&cg, &py).and_then(|()| atomic_write_checked(&py, bundled_hook.as_bytes(), true, unchanged));
        match res {
            Ok(()) => undo.replaced.push((py.clone(), old.clone(), bundled_hook.as_bytes().to_vec())),
            Err(_) if changed.get() => {
                let marker_there = fs::symlink_metadata(&marker).is_ok();
                return Err(match undo.run() {
                    None => leftover(marker_there),
                    Some(r) => InstallError::Failed { reason: leftover(marker_there).to_string(), rollback: Some(r) },
                });
            }
            Err(e) => return Err(failed(undo, format!("could not write {}: {e}", py.display()))),
        }
    }
    for (path, content, mode, wanted) in
        [(&py, bundled_hook.as_bytes(), None, adopt.is_none()), (&sh, HOOK_SH_CONTENT.as_bytes(), Some(0o755), write_sh)]
    {
        if !wanted {
            continue;
        }
        let res = guard_hook_file(&cg, path).and_then(|()| create_file_exclusive(path, content, mode));
        match res {
            Ok(()) => undo.created.push((path.clone(), content.to_vec())),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && path == &py => {
                let marker_there = fs::symlink_metadata(&marker).is_ok();
                return Err(match undo.run() {
                    None => leftover(marker_there),
                    Some(r) => InstallError::Failed { reason: leftover(marker_there).to_string(), rollback: Some(r) },
                });
            }
            Err(e) => return Err(failed(undo, format!("could not write {}: {e}", path.display()))),
        }
    }
    if old_marker.as_deref() != Some(version.as_bytes()) {
        let res = guard_hook_file(&cg, &marker).and_then(|()| match &old_marker {
            None => create_file_exclusive(&marker, version.as_bytes(), None),
            Some(_) => atomic_write_bytes(&marker, version.as_bytes(), true),
        });
        match res {
            Ok(()) => match old_marker {
                None => undo.created.push((marker.clone(), version.as_bytes().to_vec())),
                Some(old) => undo.replaced.push((marker.clone(), old, version.as_bytes().to_vec())),
            },
            Err(e) => return Err(failed(undo, format!("could not write {}: {e}", marker.display()))),
        }
    }
    if let Err(e) = install_fault("after_hooks") {
        return Err(failed(undo, e));
    }

    // 3. Back up what we read; a file that changed since then is someone else's write, not ours to
    //    clobber.
    if let Err(e) = create_dir_tracked(&claude_dir, &mut undo) {
        return Err(failed(undo, format!("could not create {}: {e}", claude_dir.display())));
    }
    if let Err(e) = install_fault("before_backup") {
        return Err(failed(undo, e));
    }
    let unchanged_since_read = |what: &str| -> Result<(), String> {
        match (&raw, fs::read(&target)) {
            (Some(text), Ok(now)) if now == text.as_bytes() => Ok(()),
            (Some(_), Ok(_)) => Err(format!("settings.json changed {what}; try again")),
            (Some(_), Err(e)) => Err(format!("could not re-read settings.json: {e}")),
            (None, _) => match fs::symlink_metadata(&literal) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("could not check settings.json: {e}")),
                Ok(_) => Err(format!("settings.json appeared {what}; try again")),
            },
        }
    };
    if let Err(why) = unchanged_since_read("while setting up") {
        return Err(failed(undo, why));
    }
    let backup = match &raw {
        None => None,
        Some(text) => match backup_settings(&literal, text.as_bytes()) {
            Ok(p) => Some(p),
            Err(e) => return Err(failed(undo, format!("could not back up settings.json, so it was not changed: {e}"))),
        },
    };

    // 4. Write to the realpath. The compare against what we read runs inside the atomic write,
    //    after the temp file is written and synced, immediately before the rename. What remains
    //    is the gap between that read and the rename syscall (microseconds; this file has no
    //    lock): a write by Claude Code landing inside it is replaced by ours. The backup holds the
    //    file as it was before setup; the README notes this.
    if let Err(e) = guard_settings_target(home, &target) {
        return Err(failed(undo, e));
    }
    let mut ours = patch.content.into_bytes();
    corrupt_point(&mut ours);
    if let Err(e) = install_fault("before_write") {
        return Err(failed(undo, e));
    }
    let refused_why = std::cell::RefCell::new(None);
    let check = || unchanged_since_read("while setting up").map_err(|why| {
        *refused_why.borrow_mut() = Some(why.clone());
        io::Error::other(why)
    });
    if let Err(e) = atomic_write_checked(&target, &ours, true, check) {
        let why = refused_why.borrow_mut().take().unwrap_or_else(|| format!("could not write settings.json: {e}"));
        return Err(failed(undo, why));
    }
    undo.settings = Some(SettingsUndo { home: home.to_path_buf(), target: target.clone(), backup: backup.clone(), ours: ours.clone() });
    undo.written = Some(target.clone());
    if let Err(e) = install_fault("after_settings_write") {
        return Err(failed(undo, e));
    }

    // 5. Verify what is on disk now. Bytes other than ours are another program's write: never
    //    restore over it. Setup worked if that write kept our entry; otherwise it did not, and the
    //    hook files this call created go.
    let created: Vec<PathBuf> = undo.created.iter().map(|(p, _)| p.clone()).filter(|p| p != &marker).collect();
    let adopted = adopt.is_some();
    let report = |note| InstallReport {
        settings: target.clone(),
        backup: backup.clone(),
        created: created.clone(),
        version: version.to_owned(),
        note,
        adopted,
    };
    let after = match fs::read(&target) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Deleted since our write: no entry, so capture is not set up. Nothing to restore over.
            undo.settings = None;
            let rollback = undo.run();
            return Err(InstallError::Failed {
                reason: "settings.json was removed by another program right after setup, so capture is not set up".into(),
                rollback,
            });
        }
        // We cannot tell what is there, so we cannot tell whether our entry is: keep everything.
        Err(e) => {
            return Ok(report(Some(format!(
                "Setup wrote settings.json but could not read it back to check it ({e}). The hook files were kept; \
                 check capture status before relying on it."
            ))))
        }
    };
    if after != ours {
        // Another program's write: never restored over. Whether setup held is the status's own
        // test; the hook files go (in `Undo::run`, by `hook_files_may_go`) only without an entry.
        let note = concurrent_note(backup.as_deref());
        match registration_of(&target) {
            OnDisk::Registered => {
                prune_settings_backups(&literal);
                return Ok(report(Some(note)));
            }
            OnDisk::NotRegistered | OnDisk::Absent => {
                undo.settings = None; // theirs now: leave it
                let rollback = undo.run();
                return Err(InstallError::Failed {
                    reason: format!("{note} It no longer has ClaudeGate's entry, so capture is not set up"),
                    rollback,
                });
            }
            OnDisk::Unknown(r) => {
                return Ok(report(Some(format!(
                    "{note} It could not be checked for ClaudeGate's entry ({r}), so the hook files were kept; \
                     check capture status before relying on it."
                ))))
            }
        }
    }
    let text = String::from_utf8_lossy(&after);
    if let Some(problem) = verify_settings_content(&text, raw.as_deref().unwrap_or(""), &hook_command) {
        return Err(failed(undo, format!("settings.json did not verify after the write: {problem}")));
    }
    // Prune only once setup has succeeded: a failed one never costs an older copy.
    prune_settings_backups(&literal);
    Ok(report(None))
}

// ---- Updating the app's own hook.py ----------------------------------------------------------

//
// The app ships a newer hook.py than the one it installed: rewrite it, but only the copy this app
// put there. Every one of these must hold, or nothing is written:
// - read-only mode is off, and python3 was found;
// - the marker exists (refused outright otherwise: it is the record that the setup is ours);
// - the installed hook.py exists, is a regular file and carries a `CLAUDEGATE_HOOK_VERSION`
//   stamp (`app_owns_hook`): the extension's copy has none, so an extension that rewrote hook.py
//   since setup keeps it;
// - `capture_status` is exactly `SetUpByApp` (an entry registered, and the setup ours);
// - the bundled version is newer than the installed hook.py's own stamp. The marker can disagree
//   with the file (a half-finished earlier update); the file is what runs.
// hook.py is replaced atomically (its mode kept), comparing it with what was read right before
// the rename; then the marker gets the new version. settings.json and hook.sh are never written.

pub(crate) fn update_app_hook(
    home: &Path,
    bundled_hook: &str,
    python3: bool,
    read_only: bool,
) -> Result<crate::capture::HookUpdate, String> {
    use crate::capture::{app_owns_hook, bundled_hook_version, version_newer, HookUpdate};
    let skip = |why: &str| Ok(HookUpdate::Skipped(why.to_owned()));
    if read_only {
        return skip("read-only mode is on");
    }
    if !python3 {
        return skip("python3 was not found");
    }
    let to = bundled_hook_version(bundled_hook).ok_or("the bundled hook has no version stamp")?;
    let cg = home.join(".claudegate");
    let (py, marker) = (cg.join(HOOK_PY_FILE), cg.join(MARKER_FILE));
    match fs::symlink_metadata(&marker) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return skip("capture was not set up by this app (no marker)"),
        Err(e) => return Err(format!("could not read {}: {e}", marker.display())),
    }
    let installed = match fs::symlink_metadata(&py) {
        Ok(m) if m.file_type().is_file() => fs::read(&py).map_err(|e| format!("could not read {}: {e}", py.display()))?,
        Ok(_) => return skip("hook.py is not a regular file"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return skip("hook.py is missing"),
        Err(e) => return Err(format!("could not read {}: {e}", py.display())),
    };
    if !app_owns_hook(true, Some(&installed)) {
        return skip("hook.py has no version stamp: it is not this app's copy");
    }
    let raw = match settings_target(home).and_then(fs::read) {
        Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return skip(&format!("settings.json could not be read: {e}")),
    };
    match capture_status(raw.as_deref(), true, python3) {
        CaptureStatus::SetUpByApp { .. } => {}
        other => return skip(&format!("capture status is {other:?}, not set up by this app")),
    }
    let from = bundled_hook_version(&String::from_utf8_lossy(&installed)).unwrap_or_default();
    if !version_newer(&to, &from) {
        return skip(&format!("hook.py is {from}, the bundled one is {to}: not newer"));
    }
    guard_hook_file(&cg, &py).map_err(|e| e.to_string())?;
    let unchanged = || match fs::read(&py) {
        Ok(now) if now == installed => Ok(()),
        Ok(_) => Err(io::Error::other("hook.py changed while it was being updated")),
        Err(e) => Err(e),
    };
    atomic_write_checked(&py, bundled_hook.as_bytes(), true, unchanged)
        .map_err(|e| format!("could not update {}: {e}", py.display()))?;
    guard_hook_file(&cg, &marker)
        .and_then(|()| atomic_write_bytes(&marker, to.as_bytes(), true))
        .map_err(|e| format!("updated hook.py, but could not update {}: {e}", marker.display()))?;
    Ok(HookUpdate::Updated { from, to })
}

#[path = "../tests/unit/install_capture.rs"]
#[cfg(test)]
mod install_capture_tests;

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
