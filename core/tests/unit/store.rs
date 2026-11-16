//! Session store: the extension's lock, atomic write, backup and persist (v1.16.1 `sessionManager.ts`).
//! Temp dirs only; nothing here touches ~/.claudegate or ~/.claudegate-desktop.
//!
//! Compiled as a unit-test module of `store.rs` (`#[path]`), not as an integration test: `persist`,
//! `backup` and `acquire_lock` are crate-private so that only `decide` can reach them. It lives
//! under `core/tests/unit/` so `no_writes` (which scans `core/src`) does not count its writes and
//! cargo does not build it a second time as its own test target.

use crate::merge::MergeGuards;
use crate::model::{parse_session, Session, Status};
use crate::store::{acquire_lock, backup, persist, StoreError, StorePaths};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

fn session(v: Value) -> Session {
    parse_session(&serde_json::to_string(&v).unwrap()).unwrap().0
}

fn entry(orig: &str, at: &str) -> Value {
    json!({ "originalContent": orig, "reviewStatus": "pending", "sessionId": "s", "capturedAt": at })
}

struct Env {
    _tmp: tempfile::TempDir,
    paths: StorePaths,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("cg");
    let desktop_home = tmp.path().join("cgd");
    fs::create_dir_all(home.join("sessions")).unwrap();
    let session_file = home.join("sessions").join("0123456789abcdef0123456789abcdef.json");
    Env { paths: StorePaths { home, desktop_home, session_file }, _tmp: tmp }
}

fn lock_path(f: &Path) -> PathBuf {
    let mut s = f.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

fn backups_dir(e: &Env) -> PathBuf {
    e.paths.desktop_home.join("backups").join("0123456789abcdef0123456789abcdef")
}

#[test]
fn lock_excludes_second_writer() {
    let e = env();
    let first = acquire_lock(&e.paths.session_file).expect("first lock");
    let lp = lock_path(&e.paths.session_file);
    assert_eq!(fs::read_to_string(&lp).unwrap(), std::process::id().to_string());
    let t = Instant::now();
    let second = acquire_lock(&e.paths.session_file);
    let waited = t.elapsed();
    assert!(second.is_none(), "second writer must not get the lock");
    assert!(waited >= Duration::from_millis(190), "waited only {waited:?}");
    assert!(waited < Duration::from_millis(1500), "waited {waited:?}");
    drop(first);
    assert!(!lp.exists(), "drop releases the lock");
    assert!(acquire_lock(&e.paths.session_file).is_some());
}

#[test]
fn stale_lock_is_stolen() {
    let e = env();
    let lp = lock_path(&e.paths.session_file);
    fs::write(&lp, "99999").unwrap();
    let f = fs::File::options().write(true).open(&lp).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(4)).unwrap();
    drop(f);
    let t = Instant::now();
    let lock = acquire_lock(&e.paths.session_file);
    assert!(lock.is_some(), "a 4 s old lock is stale");
    assert!(t.elapsed() < Duration::from_millis(150));
    assert_eq!(fs::read_to_string(&lp).unwrap(), std::process::id().to_string());
}


/// M6: a stale lock that cannot be unlinked (read-only sessions dir) must not spin: the wait
/// ends at the 200 ms deadline like any other held lock, and we proceed unlocked.
#[cfg(unix)]
#[test]
fn an_unremovable_stale_lock_times_out_instead_of_spinning() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let lp = lock_path(&e.paths.session_file);
    fs::write(&lp, "99999").unwrap();
    let f = fs::File::options().write(true).open(&lp).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(4)).unwrap();
    drop(f);
    let dir = e.paths.session_file.parent().unwrap().to_path_buf();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.join(".probe");
    if fs::write(&probe, "x").is_ok() {
        let _ = fs::remove_file(&probe);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipping: running as root");
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let file = e.paths.session_file.clone();
    std::thread::spawn(move || {
        let t = Instant::now();
        let got = acquire_lock(&file).is_some();
        let _ = tx.send((got, t.elapsed()));
    });
    let r = rx.recv_timeout(Duration::from_secs(3));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let (got, waited) = r.expect("acquire_lock spun past its deadline");
    assert!(!got, "the lock could not be taken");
    assert!(waited >= Duration::from_millis(190) && waited < Duration::from_millis(1500), "{waited:?}");
    assert!(lp.exists(), "the stale lock is still there");
}

#[test]
fn backup_keeps_newest_50() {
    let e = env();
    for i in 0..55 {
        fs::write(&e.paths.session_file, format!("{i}")).unwrap();
        backup(&e.paths.desktop_home, &e.paths.session_file).unwrap();
    }
    let dir = backups_dir(&e);
    let mut contents: Vec<u32> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|d| fs::read_to_string(d.path()).unwrap().parse().unwrap())
        .collect();
    contents.sort();
    assert_eq!(contents, (5..55).collect::<Vec<u32>>());
    for d in fs::read_dir(&dir).unwrap().flatten() {
        let n = d.file_name().to_string_lossy().into_owned();
        assert!(n.ends_with(".json"), "{n}");
        // yyyymmddThhmmss.mmmZ[-N].json
        assert_eq!(&n[8..9], "T", "{n}");
        assert_eq!(&n[15..16], ".", "{n}");
        assert_eq!(&n[19..20], "Z", "{n}");
    }
}

#[test]
fn backup_of_missing_file_is_a_no_op() {
    let e = env();
    backup(&e.paths.desktop_home, &e.paths.session_file).unwrap();
    assert!(!backups_dir(&e).exists());
}

#[test]
fn persist_refuses_path_outside_sessions() {
    let e = env();
    let mut p = StorePaths {
        home: e.paths.home.clone(),
        desktop_home: e.paths.desktop_home.clone(),
        session_file: e.paths.home.join("other").join("x.json"),
    };
    let mine = session(json!({ "sessionId": "a", "status": "active", "files": {} }));
    assert!(matches!(persist(&p, mine.clone(), &MergeGuards::default(), true), Err(StoreError::OutsideHome)));
    p.session_file = e.paths.home.join("sessions").join("nested").join("x.json");
    assert!(matches!(persist(&p, mine.clone(), &MergeGuards::default(), true), Err(StoreError::OutsideHome)));
    p.session_file = e.paths.home.join("sessions").join("..").join("x.json");
    assert!(matches!(persist(&p, mine, &MergeGuards::default(), true), Err(StoreError::OutsideHome)));
    assert!(!e.paths.home.join("other").exists());
    assert!(!e.paths.home.join("x.json").exists());
}

#[test]
fn persist_sets_status_reviewed_when_files_empty() {
    let e = env();
    let mine = session(json!({ "sessionId": "a", "status": "active", "files": {} }));
    let out = persist(&e.paths, mine, &MergeGuards::default(), true).unwrap();
    assert_eq!(out.status, Status::Reviewed);
    let disk: Value = serde_json::from_str(&fs::read_to_string(&e.paths.session_file).unwrap()).unwrap();
    assert_eq!(disk["status"], "reviewed");

    let mine = session(json!({
        "sessionId": "a", "status": "reviewed",
        "files": { "/w/a.ts": entry("x", "2026-11-16T00:00:00.000Z") }
    }));
    let out = persist(&e.paths, mine, &MergeGuards::default(), true).unwrap();
    assert_eq!(out.status, Status::Active);
}

#[test]
fn persist_writes_pretty_json_two_space() {
    let e = env();
    let mine = session(json!({
        "sessionId": "a", "status": "active",
        "files": { "/w/a.ts": entry("x", "2026-11-16T00:00:00.000Z") }
    }));
    let out = persist(&e.paths, mine, &MergeGuards::default(), true).unwrap();
    let text = fs::read_to_string(&e.paths.session_file).unwrap();
    assert_eq!(text, serde_json::to_string_pretty(&out).unwrap());
    assert!(text.starts_with("{\n  \"sessionId\": \"a\",\n  \"status\": \"active\",\n  \"files\": {\n    \"/w/a.ts\""));
    assert!(!text.ends_with('\n'), "JSON.stringify writes no trailing newline");
    assert!(!lock_path(&e.paths.session_file).exists(), "lock released");
}

#[test]
fn persist_keeps_capture_written_after_load() {
    let e = env();
    let base = json!({
        "sessionId": "a", "status": "active",
        "files": { "/w/a.ts": entry("a0", "2026-11-16T00:00:00.000Z") }
    });
    fs::write(&e.paths.session_file, serde_json::to_string_pretty(&base).unwrap()).unwrap();
    let mine = session(base.clone());

    // The hook captures b.ts after we loaded.
    let mut disk = base;
    disk["files"]["/w/b.ts"] = entry("b0", "2026-11-16T00:00:05.000Z");
    fs::write(&e.paths.session_file, serde_json::to_string_pretty(&disk).unwrap()).unwrap();

    let out = persist(&e.paths, mine, &MergeGuards::default(), true).unwrap();
    assert!(out.files.contains_key("/w/a.ts"));
    assert!(out.files.contains_key("/w/b.ts"));
    let written = parse_session(&fs::read_to_string(&e.paths.session_file).unwrap()).unwrap().0;
    assert_eq!(written, out);
    // The pre-write disk copy was backed up.
    let backups: Vec<_> = fs::read_dir(backups_dir(&e)).unwrap().flatten().collect();
    assert_eq!(backups.len(), 1);
    assert!(fs::read_to_string(backups[0].path()).unwrap().contains("/w/b.ts"));
}

#[test]
fn persist_writes_ours_when_disk_unparsable() {
    let e = env();
    fs::write(&e.paths.session_file, "{ not json").unwrap();
    let mine = session(json!({
        "sessionId": "a", "status": "active",
        "files": { "/w/a.ts": entry("a0", "2026-11-16T00:00:00.000Z") }
    }));
    let out = persist(&e.paths, mine, &MergeGuards::default(), true).unwrap();
    assert!(out.files.contains_key("/w/a.ts"));
    assert_eq!(fs::read_to_string(&e.paths.session_file).unwrap(), serde_json::to_string_pretty(&out).unwrap());
}

#[test]
fn persist_refuses_unknown_disk_shape() {
    let e = env();
    let disk = json!({ "sessionId": "a", "status": "active", "files": {}, "fromTheFuture": 1 });
    let text = serde_json::to_string_pretty(&disk).unwrap();
    fs::write(&e.paths.session_file, &text).unwrap();
    let mine = session(json!({ "sessionId": "a", "status": "active", "files": {} }));
    assert!(matches!(persist(&e.paths, mine, &MergeGuards::default(), true), Err(StoreError::UnknownShape)));
    assert_eq!(fs::read_to_string(&e.paths.session_file).unwrap(), text, "untouched");
    assert!(!lock_path(&e.paths.session_file).exists(), "lock released on refusal");
}

#[cfg(unix)]
#[test]
fn persist_aborts_when_backup_fails() {
    let e = env();
    let disk = json!({ "sessionId": "a", "status": "active", "files": {} });
    let text = serde_json::to_string_pretty(&disk).unwrap();
    fs::write(&e.paths.session_file, &text).unwrap();
    // A file where the backups dir must go makes the backup fail.
    fs::create_dir_all(&e.paths.desktop_home).unwrap();
    fs::write(e.paths.desktop_home.join("backups"), "").unwrap();
    let mine = session(json!({
        "sessionId": "a", "status": "active",
        "files": { "/w/a.ts": entry("a0", "2026-11-16T00:00:00.000Z") }
    }));
    assert!(matches!(persist(&e.paths, mine, &MergeGuards::default(), true), Err(StoreError::Backup(_))));
    assert_eq!(fs::read_to_string(&e.paths.session_file).unwrap(), text, "no write without a backup");
}

// ── File snapshots (C1(b)) ──────────────────────────────────────────────────

use crate::store::{preflight, FileSnapshots};

#[test]
fn file_snapshots_keep_raw_bytes_and_skip_missing_files() {
    let e = env();
    let src = e._tmp.path().join("w/x.bin");
    fs::create_dir_all(src.parent().unwrap()).unwrap();
    fs::write(&src, [0xffu8, 0, 0x80]).unwrap();
    let mut snaps = FileSnapshots::new(&e.paths.desktop_home, &e.paths.session_file).unwrap();
    assert!(snaps.dir().is_none(), "nothing saved, no dir");
    assert!(!snaps.save(Path::new("gone.txt"), &e._tmp.path().join("gone.txt")).unwrap());
    assert!(snaps.dir().is_none(), "a missing file creates no stamp dir");
    assert!(snaps.save(Path::new("w/x.bin"), &src).unwrap());
    let dir = snaps.dir().unwrap().to_path_buf();
    assert!(dir.starts_with(backups_dir(&e).join("files")), "{dir:?}");
    assert_eq!(fs::read(dir.join("w/x.bin")).unwrap(), [0xffu8, 0, 0x80]);
}

#[test]
fn file_snapshots_refuse_a_path_that_leaves_the_stamp_dir() {
    let e = env();
    let src = e._tmp.path().join("a.txt");
    fs::write(&src, "x").unwrap();
    let mut snaps = FileSnapshots::new(&e.paths.desktop_home, &e.paths.session_file).unwrap();
    for bad in ["../a.txt", "/abs/a.txt", "", "x/../../a.txt", "./a.txt"] {
        assert!(snaps.save(Path::new(bad), &src).is_err(), "{bad:?}");
    }
    assert!(!e._tmp.path().join("cgd/backups/0123456789abcdef0123456789abcdef/a.txt").exists());
}

#[test]
fn file_snapshots_keep_the_newest_50_decisions() {
    let e = env();
    let src = e._tmp.path().join("a.txt");
    fs::write(&src, "x").unwrap();
    for _ in 0..55 {
        let mut snaps = FileSnapshots::new(&e.paths.desktop_home, &e.paths.session_file).unwrap();
        snaps.save(Path::new("a.txt"), &src).unwrap();
    }
    let n = fs::read_dir(backups_dir(&e).join("files")).unwrap().count();
    assert_eq!(n, 50);
}

#[test]
fn preflight_checks_shape_before_backing_up() {
    let e = env();
    let text = serde_json::to_string(&json!({ "sessionId": "s", "status": "active", "files": {}, "future": 1 })).unwrap();
    fs::write(&e.paths.session_file, &text).unwrap();
    assert!(matches!(preflight(&e.paths), Err(StoreError::UnknownShape)));
    assert!(!backups_dir(&e).exists(), "no backup for a write that will not happen");
    let known = serde_json::to_string(&json!({ "sessionId": "s", "status": "active", "files": {} })).unwrap();
    fs::write(&e.paths.session_file, &known).unwrap();
    preflight(&e.paths).unwrap();
    assert_eq!(fs::read_dir(backups_dir(&e)).unwrap().count(), 1);
    // persist told the backup was taken does not take a second one.
    persist(&e.paths, session(json!({ "sessionId": "s", "status": "active", "files": {} })), &MergeGuards::default(), false)
        .unwrap();
    assert_eq!(fs::read_dir(backups_dir(&e)).unwrap().count(), 1);
}
