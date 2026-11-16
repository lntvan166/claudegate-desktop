//! Session store: a port of the extension's write protocol (`sessionManager.ts`, v1.16.1):
//! `acquireLock`, `releaseLock`, `persist`, `persistLocked` and `atomicWrite`. hook.py and the
//! extension write the same session files concurrently, so the lock path, the timings, the
//! merge-on-write and the temp-file naming all match theirs.
//!
//! Every write first copies the on-disk session to `<desktop_home>/backups/<hash>/`; a failed
//! backup refuses the write. `persist` refuses any path whose parent is not `<home>/sessions`.

use crate::merge::{merge_fresh_captures, MergeGuards};
use crate::model::{parse_session, Session, Shape, Status};
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
pub fn acquire_lock(session_file: &Path) -> Option<Lock> {
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
                    let _ = fs::remove_file(&path); // raced: someone else removed it
                    continue;
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
pub fn atomic_write(target: &Path, content: &str, preserve_existing: bool) -> io::Result<()> {
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
    let stem = name.strip_suffix(".json")?;
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
pub fn backup(desktop_home: &Path, session_file: &Path) -> Result<(), StoreError> {
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

/// Read-merge-write `mine` under the lock, as the extension's `persist`: re-read the disk copy
/// and merge in what other writers added, set `status`, back up, then write atomically.
/// Returns the session as written. Refuses a disk copy of unknown shape without writing.
pub fn persist(paths: &StorePaths, mine: Session, guards: &MergeGuards) -> Result<Session, StoreError> {
    let sessions = paths.home.join("sessions");
    if paths.session_file.parent() != Some(sessions.as_path()) || paths.session_file.file_name().is_none() {
        return Err(StoreError::OutsideHome);
    }
    let _lock = acquire_lock(&paths.session_file);

    // No readable/parseable disk copy: write our own state (never lose it).
    let disk = fs::read_to_string(&paths.session_file).ok().and_then(|t| parse_session(&t).ok());
    let mut session = match disk {
        Some((_, Shape::Unknown(_))) => return Err(StoreError::UnknownShape),
        Some((disk, Shape::Known)) => merge_fresh_captures(mine, disk, guards),
        None => mine,
    };

    session.status = if session.files.is_empty() { Status::Reviewed } else { Status::Active };

    backup(&paths.desktop_home, &paths.session_file)?;

    let text = serde_json::to_string_pretty(&session).map_err(|e| StoreError::Write(e.to_string()))?;
    atomic_write(&paths.session_file, &text, false).map_err(|e| StoreError::Write(e.to_string()))?;
    Ok(session)
}

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
}
