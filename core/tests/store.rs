//! Session store: the extension's lock, atomic write, backup and persist (v1.16.1 `sessionManager.ts`).
//! Temp dirs only; nothing here touches ~/.claudegate or ~/.claudegate-desktop.

use claudegate_core::merge::MergeGuards;
use claudegate_core::model::{parse_session, Session, Status};
use claudegate_core::store::{acquire_lock, backup, persist, StoreError, StorePaths};
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
    assert!(matches!(persist(&p, mine.clone(), &MergeGuards::default()), Err(StoreError::OutsideHome)));
    p.session_file = e.paths.home.join("sessions").join("nested").join("x.json");
    assert!(matches!(persist(&p, mine.clone(), &MergeGuards::default()), Err(StoreError::OutsideHome)));
    p.session_file = e.paths.home.join("sessions").join("..").join("x.json");
    assert!(matches!(persist(&p, mine, &MergeGuards::default()), Err(StoreError::OutsideHome)));
    assert!(!e.paths.home.join("other").exists());
    assert!(!e.paths.home.join("x.json").exists());
}

#[test]
fn persist_sets_status_reviewed_when_files_empty() {
    let e = env();
    let mine = session(json!({ "sessionId": "a", "status": "active", "files": {} }));
    let out = persist(&e.paths, mine, &MergeGuards::default()).unwrap();
    assert_eq!(out.status, Status::Reviewed);
    let disk: Value = serde_json::from_str(&fs::read_to_string(&e.paths.session_file).unwrap()).unwrap();
    assert_eq!(disk["status"], "reviewed");

    let mine = session(json!({
        "sessionId": "a", "status": "reviewed",
        "files": { "/w/a.ts": entry("x", "2026-11-16T00:00:00.000Z") }
    }));
    let out = persist(&e.paths, mine, &MergeGuards::default()).unwrap();
    assert_eq!(out.status, Status::Active);
}

#[test]
fn persist_writes_pretty_json_two_space() {
    let e = env();
    let mine = session(json!({
        "sessionId": "a", "status": "active",
        "files": { "/w/a.ts": entry("x", "2026-11-16T00:00:00.000Z") }
    }));
    let out = persist(&e.paths, mine, &MergeGuards::default()).unwrap();
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

    let out = persist(&e.paths, mine, &MergeGuards::default()).unwrap();
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
    let out = persist(&e.paths, mine, &MergeGuards::default()).unwrap();
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
    assert!(matches!(persist(&e.paths, mine, &MergeGuards::default()), Err(StoreError::UnknownShape)));
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
    assert!(matches!(persist(&e.paths, mine, &MergeGuards::default()), Err(StoreError::Backup(_))));
    assert_eq!(fs::read_to_string(&e.paths.session_file).unwrap(), text, "no write without a backup");
}
