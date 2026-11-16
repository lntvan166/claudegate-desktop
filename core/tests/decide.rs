//! decide: accept / reject / revert / reapply / clear across a project's scopes, ported from the
//! extension's `sessionManager.ts` (v1.16.1). Temp dirs only: nothing here touches
//! ~/.claudegate, ~/.claudegate-desktop or a real workspace.

use claudegate_core::decide::{
    decide, decide_with, Action, DecideError, DecidePaths, DecideReport, Target, UNKNOWN_SHAPE_MESSAGE,
};
use claudegate_core::paths::session_file_for;
use claudegate_core::store::StoreError;
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const CAP: &str = "2026-11-16T10:00:00.000Z";
const NOW: &str = "2026-11-16T12:00:00.000Z";

struct Fx {
    tmp: tempfile::TempDir,
    paths: DecidePaths,
    ws: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("cg");
        let desktop_home = tmp.path().join("cgd");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::create_dir_all(&ws).unwrap();
        Fx { paths: DecidePaths { home, desktop_home }, ws, tmp }
    }

    fn ws_str(&self) -> String {
        self.ws.to_string_lossy().into_owned()
    }

    /// A worktree scope nested in the workspace: `.git` is a file pointing at
    /// `<main>/.git/worktrees/<name>`, as `git worktree add` leaves it.
    fn worktree(&self, name: &str) -> PathBuf {
        let wt = self.ws.join(name);
        let gitdir = self.ws.join(".git").join("worktrees").join(name);
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        wt
    }

    fn file(&self, rel: &str, content: &str) -> String {
        let p = self.ws.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn session_file(&self, scope: &Path) -> PathBuf {
        session_file_for(&self.paths.home, scope)
    }

    fn write_session(&self, scope: &Path, v: Value) {
        fs::write(self.session_file(scope), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }

    fn session(&self, scope: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(self.session_file(scope)).unwrap()).unwrap()
    }

    fn excludes(&self, globs: &[&str]) {
        let v = json!({ self.ws_str(): globs });
        fs::write(self.paths.home.join("excludes.json"), v.to_string()).unwrap();
    }

    fn backups(&self, scope: &Path) -> usize {
        let hash = self.session_file(scope).file_stem().unwrap().to_owned();
        // Session backups only: the `files/` dir beside them holds workspace-file snapshots.
        fs::read_dir(self.paths.desktop_home.join("backups").join(hash))
            .map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).count())
            .unwrap_or(0)
    }

    fn files_dir(&self, scope: &Path) -> PathBuf {
        let hash = self.session_file(scope).file_stem().unwrap().to_owned();
        self.paths.desktop_home.join("backups").join(hash).join("files")
    }

    /// Every snapshot of `rel` (relative to the scope root), across decision stamps.
    fn snapshots(&self, scope: &Path, rel: &str) -> Vec<PathBuf> {
        let Ok(rd) = fs::read_dir(self.files_dir(scope)) else { return vec![] };
        rd.flatten().map(|e| e.path().join(rel)).filter(|p| p.exists()).collect()
    }

    fn run(&self, action: Action, target: Target) -> Result<DecideReport, DecideError> {
        decide(&self.paths, &self.ws, action, target, false, NOW)
    }

    fn file_target(&self, scope: &Path, key: &str) -> Target {
        Target::File { scope: scope.to_string_lossy().into_owned(), key: key.to_owned() }
    }
}

fn pending(orig: Option<&str>) -> Value {
    json!({ "originalContent": orig, "reviewStatus": "pending", "sessionId": "s1", "capturedAt": CAP })
}

fn pending_new() -> Value {
    json!({ "originalContent": null, "reviewStatus": "pending", "newFile": true, "sessionId": "s1", "capturedAt": CAP })
}

fn record(path: &str, before: Option<&str>, after: Option<&str>, at: &str) -> Value {
    json!({ "id": format!("{at}::{path}"), "path": path, "before": before, "after": after, "decidedAt": at, "sessionId": "s1" })
}

fn session_json(files: Value, accepted: Value, rejected: Value) -> Value {
    json!({ "sessionId": "2026-11-16T09:00:00.000Z", "status": "active", "files": files, "accepted": accepted, "rejected": rejected })
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

/// path → (size, mtime, content hash) of every file under `root`.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime, String)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            let m = fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                // Directories too: a created or removed entry (a lock, a tmp) changes their mtime.
                out.insert(p.clone(), (0, m.modified().unwrap(), "<dir>".to_owned()));
                stack.push(p);
            } else {
                let hash = format!("{:x}", Md5::digest(fs::read(&p).unwrap_or_default()));
                out.insert(p, (m.len(), m.modified().unwrap(), hash));
            }
        }
    }
    out
}

#[cfg(unix)]
fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();
}

/// True when permission bits do not stop us (running as root): the permission tests skip.
#[cfg(unix)]
fn perms_ignored(dir: &Path) -> bool {
    let probe = dir.join(".probe");
    let ok = fs::write(&probe, "x").is_ok();
    let _ = fs::remove_file(&probe);
    ok
}

// ── Accept ──────────────────────────────────────────────────────────────────

#[test]
fn accept_file_records_disk_content_and_reviews_the_last() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "new");
    fx.write_session(
        &fx.ws,
        session_json(
            json!({ &a: pending(Some("old")) }),
            json!([]),
            json!({ &a: record(&a, Some("x"), Some("y"), "2026-11-16T08:00:00.000Z") }),
        ),
    );
    let r = fx.run(Action::Accept, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 1);
    assert!(r.errors.is_empty() && r.left.is_empty());
    let s = fx.session(&fx.ws);
    assert_eq!(s["status"], "reviewed");
    assert!(keys(&s["files"]).is_empty());
    let acc = s["accepted"].as_array().unwrap();
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0]["id"], format!("{NOW}::{a}"));
    assert_eq!(acc[0]["before"], "old");
    assert_eq!(acc[0]["after"], "new");
    assert_eq!(acc[0]["decidedAt"], NOW);
    // acceptEntry drops the prior reject, and the guard keeps the disk copy from restoring it.
    assert!(keys(&s["rejected"]).is_empty());
    assert_eq!(fs::read_to_string(&a).unwrap(), "new", "accept never touches the file");
}

#[test]
fn accept_file_keeps_status_active_while_others_pend() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "new");
    let b = fx.file("b.txt", "new");
    fx.write_session(
        &fx.ws,
        session_json(json!({ &a: pending(Some("old")), &b: pending(Some("old")) }), json!([]), json!({})),
    );
    fx.run(Action::Accept, fx.file_target(&fx.ws, &a)).unwrap();
    let s = fx.session(&fx.ws);
    assert_eq!(s["status"], "active");
    assert_eq!(keys(&s["files"]), vec![b]);
}

#[test]
fn single_file_actions_apply_to_excluded_and_noop_entries() {
    let fx = Fx::new();
    let a = fx.file("gen/a.txt", "same");
    fx.excludes(&["gen"]);
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("same")) }), json!([]), json!({})));
    let r = fx.run(Action::Accept, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 1);
    assert!(keys(&fx.session(&fx.ws)["files"]).is_empty());
}

#[test]
fn folder_accept_only_touches_paths_under_the_prefix() {
    let fx = Fx::new();
    let x = fx.file("src/x.txt", "new");
    let y = fx.file("src/deep/y.txt", "new");
    let z = fx.file("srcx/z.txt", "new"); // shares the prefix string, not the folder
    let o = fx.file("other.txt", "new");
    fx.write_session(
        &fx.ws,
        session_json(
            json!({ &x: pending(Some("old")), &y: pending(Some("old")), &z: pending(Some("old")), &o: pending(Some("old")) }),
            json!([]),
            json!({}),
        ),
    );
    let prefix = fx.ws.join("src").to_string_lossy().into_owned();
    let r = fx.run(Action::Accept, Target::Folder { scope: fx.ws_str(), prefix }).unwrap();
    assert_eq!(r.done, 2);
    let s = fx.session(&fx.ws);
    let mut left = keys(&s["files"]);
    left.sort();
    let mut want = vec![z, o];
    want.sort();
    assert_eq!(left, want);
    let accepted: Vec<&str> = s["accepted"].as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap()).collect();
    assert!(accepted.contains(&x.as_str()) && accepted.contains(&y.as_str()));
    // One timestamp for the whole bulk action.
    assert!(s["accepted"].as_array().unwrap().iter().all(|r| r["decidedAt"] == NOW));
}

/// Primary + worktree, one real edit each, plus an excluded edit, a no-op ("waiting") entry and
/// an entry outside the scope root. `All` decides exactly the two real ones.
fn all_fixture(fx: &Fx) -> (String, String, String, String, String, PathBuf) {
    let wt = fx.worktree("wt1");
    let a = fx.file("a.txt", "new");
    let ex = fx.file("excl/b.txt", "new");
    let noop = fx.file("c.txt", "same");
    let outside_dir = fx.tmp.path().join("elsewhere");
    fs::create_dir_all(&outside_dir).unwrap();
    let outside = outside_dir.join("q.txt");
    fs::write(&outside, "new").unwrap();
    let outside = outside.to_string_lossy().into_owned();
    let d = fx.file("wt1/d.txt", "new");
    fx.excludes(&["excl"]);
    fx.write_session(
        &fx.ws,
        session_json(
            json!({
                &a: pending(Some("old")),
                &ex: pending(Some("old")),
                &noop: pending(Some("same")),
                &outside: pending(Some("old")),
            }),
            json!([]),
            json!({}),
        ),
    );
    fx.write_session(&wt, session_json(json!({ &d: pending(Some("old")) }), json!([]), json!({})));
    (a, ex, noop, outside, d, wt)
}

#[test]
fn accept_all_spans_scopes_and_skips_excluded_noop_and_outside_entries() {
    let fx = Fx::new();
    let (a, ex, noop, outside, d, wt) = all_fixture(&fx);
    let r = fx.run(Action::Accept, Target::All).unwrap();
    assert_eq!(r.done, 2);
    let s = fx.session(&fx.ws);
    let mut left = keys(&s["files"]);
    left.sort();
    let mut want = vec![ex, noop, outside];
    want.sort();
    assert_eq!(left, want);
    assert_eq!(s["accepted"][0]["path"], a.as_str());
    let w = fx.session(&wt);
    assert!(keys(&w["files"]).is_empty());
    assert_eq!(w["status"], "reviewed");
    assert_eq!(w["accepted"][0]["path"], d.as_str());
}

#[test]
fn reject_all_spans_scopes_and_leaves_skipped_files_alone() {
    let fx = Fx::new();
    let (a, ex, noop, outside, d, wt) = all_fixture(&fx);
    let r = fx.run(Action::Reject, Target::All).unwrap();
    assert_eq!(r.done, 2);
    assert_eq!(fs::read_to_string(&a).unwrap(), "old");
    assert_eq!(fs::read_to_string(&d).unwrap(), "old");
    assert_eq!(fs::read_to_string(&ex).unwrap(), "new", "excluded file untouched");
    assert_eq!(fs::read_to_string(&outside).unwrap(), "new", "file outside the scope root untouched");
    assert_eq!(fs::read_to_string(&noop).unwrap(), "same");
    assert_eq!(keys(&fx.session(&fx.ws)["files"]).len(), 3);
    assert!(keys(&fx.session(&wt)["files"]).is_empty());
}

// ── Reject ──────────────────────────────────────────────────────────────────

#[test]
fn reject_restores_the_original() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "claude");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("orig")) }), json!([]), json!({})));
    let r = fx.run(Action::Reject, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(fs::read_to_string(&a).unwrap(), "orig");
    let s = fx.session(&fx.ws);
    assert!(keys(&s["files"]).is_empty());
    assert_eq!(s["status"], "reviewed");
    let rec = &s["rejected"][&a];
    assert_eq!(rec["before"], "orig");
    assert_eq!(rec["after"], "claude");
    assert_eq!(rec["decidedAt"], NOW);
}

#[test]
fn reject_deletes_a_confident_new_file() {
    let fx = Fx::new();
    let a = fx.file("made.txt", "claude");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending_new() }), json!([]), json!({})));
    let r = fx.run(Action::Reject, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 1);
    assert!(r.left.is_empty());
    assert!(!Path::new(&a).exists());
    let s = fx.session(&fx.ws);
    assert_eq!(s["rejected"][&a]["after"], "claude");
    assert_eq!(s["rejected"][&a]["newFile"], true);
}

#[test]
fn reject_leaves_a_non_confident_new_file_and_reports_it() {
    let fx = Fx::new();
    let a = fx.file("maybe.txt", "content");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(None) }), json!([]), json!({})));
    let r = fx.run(Action::Reject, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(r.left, vec![a.clone()]);
    assert_eq!(fs::read_to_string(&a).unwrap(), "content");
    let s = fx.session(&fx.ws);
    assert!(keys(&s["files"]).is_empty());
    assert!(s["rejected"].get(&a).is_some());
}

/// Review Focus 3/4: one restore fails; that entry stays pending with an error, the rest of
/// the bulk reject (a restore and a new-file delete) still lands.
#[cfg(unix)]
#[test]
fn reject_all_with_an_unwritable_file_still_does_the_others() {
    let fx = Fx::new();
    let ro = fx.file("locked/ro.txt", "claude");
    let b = fx.file("b.txt", "claude");
    let n = fx.file("n.txt", "claude");
    fx.write_session(
        &fx.ws,
        session_json(json!({ &ro: pending(Some("orig")), &b: pending(Some("orig")), &n: pending_new() }), json!([]), json!({})),
    );
    let locked = fx.ws.join("locked");
    chmod(Path::new(&ro), 0o444);
    chmod(&locked, 0o555);
    if perms_ignored(&locked) {
        chmod(&locked, 0o755);
        eprintln!("skipping: running as root");
        return;
    }
    let r = fx.run(Action::Reject, Target::All);
    chmod(&locked, 0o755);
    let r = r.unwrap();
    assert_eq!(r.done, 2);
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.errors[0].0, ro);
    assert_eq!(fs::read_to_string(&ro).unwrap(), "claude");
    assert_eq!(fs::read_to_string(&b).unwrap(), "orig");
    assert!(!Path::new(&n).exists());
    let s = fx.session(&fx.ws);
    assert_eq!(keys(&s["files"]), vec![ro.clone()]);
    assert_eq!(s["status"], "active");
    assert!(s["rejected"].get(&ro).is_none());
    assert!(s["rejected"].get(&b).is_some() && s["rejected"].get(&n).is_some());
}

/// Ruling 3 + C1: the workspace write happened but the session write failed. The error names the
/// file already restored, and Claude's bytes survive in the file snapshot taken before the restore.
#[cfg(unix)]
#[test]
fn persist_failure_after_a_restore_keeps_claudes_bytes_in_the_snapshot() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "claude");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("orig")) }), json!([]), json!({})));
    let sessions = fx.paths.home.join("sessions");
    let mut root = false;
    let r = decide_with(&fx.paths, &fx.ws, Action::Reject, Target::All, false, NOW, &mut |_| {
        chmod(&sessions, 0o555);
        root = perms_ignored(&sessions);
    });
    chmod(&sessions, 0o755);
    if root {
        eprintln!("skipping: running as root");
        return;
    }
    let e = r.unwrap_err();
    let DecideError::Store { changed, partial, snapshot, .. } = &e else { panic!("expected Store, got {e:?}") };
    assert_eq!(changed, &vec![a.clone()]);
    assert_eq!(partial, &DecideReport::default());
    assert!(e.to_string().contains(&a), "{e}");
    assert!(!e.to_string().contains("retry"), "a reject must not be retried: {e}");
    let dir = PathBuf::from(snapshot.as_ref().expect("the snapshot dir is reported"));
    assert!(dir.starts_with(fx.files_dir(&fx.ws)), "{dir:?}");
    assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"claude", "Claude's version was saved first");
    assert_eq!(fs::read_to_string(&a).unwrap(), "orig");
    assert!(keys(&fx.session(&fx.ws)["files"]).contains(&a), "session file unchanged");
}

/// C1(a): the session backup is taken before any workspace write. When it fails, nothing changes.
#[test]
fn a_failing_preflight_backup_changes_no_file() {
    for action in [Action::Reject, Action::Reapply] {
        let fx = Fx::new();
        let a = fx.file("a.txt", "claude");
        let n = fx.file("made.txt", "new file");
        let t = "2026-11-16T11:00:00.000Z";
        let (files, rejected) = if action == Action::Reject {
            (json!({ &a: pending(Some("orig")), &n: pending_new() }), json!({}))
        } else {
            (json!({}), json!({ &a: record(&a, Some("orig"), Some("again"), t) }))
        };
        fx.write_session(&fx.ws, session_json(files, json!([]), rejected));
        // A file where the backups dir must go makes the backup fail.
        fs::create_dir_all(&fx.paths.desktop_home).unwrap();
        fs::write(fx.paths.desktop_home.join("backups"), "").unwrap();
        let before = snapshot(fx.tmp.path());
        let e = fx.run(action, Target::All).unwrap_err();
        let DecideError::Store { error, changed, snapshot: snap, .. } = &e else { panic!("{action:?}: got {e:?}") };
        assert!(matches!(error, StoreError::Backup(_)), "{action:?}: {error:?}");
        assert!(changed.is_empty() && snap.is_none(), "{action:?}");
        assert_eq!(snapshot(fx.tmp.path()), before, "{action:?}: nothing changed");
    }
}

/// C1(a): a session that turned to a newer shape after our load is refused before any write.
#[test]
fn a_session_that_turned_unknown_before_the_first_write_changes_no_file() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "claude");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("orig")) }), json!([]), json!({})));
    let sf = fx.session_file(&fx.ws);
    // decide_with's hook runs before persist; turn the file Unknown via a pre-load race instead:
    // load_checked reads it Known, then the preflight sees it Unknown.
    let r = claudegate_core::decide::decide_with_preflight_hook(&fx.paths, &fx.ws, Action::Reject, Target::All, NOW, &mut || {
        let mut v: Value = serde_json::from_str(&fs::read_to_string(&sf).unwrap()).unwrap();
        v["futureField"] = json!(1);
        fs::write(&sf, v.to_string()).unwrap();
    });
    let e = r.unwrap_err();
    let DecideError::Store { error, changed, .. } = &e else { panic!("got {e:?}") };
    assert!(matches!(error, StoreError::UnknownShape), "{error:?}");
    assert!(changed.is_empty());
    assert_eq!(fs::read_to_string(&a).unwrap(), "claude");
    assert_eq!(fx.backups(&fx.ws), 0, "no backup of a file we will not write");
}

/// C1(b): a non-UTF-8 file Claude created is deleted on reject; its raw bytes are in the snapshot.
#[test]
fn reject_of_a_non_utf8_new_file_snapshots_its_bytes_before_deleting() {
    let fx = Fx::new();
    let bytes: &[u8] = &[0xff, 0xfe, 0x00, b'x', 0x80];
    let n = fx.ws.join("bin/made.dat");
    fs::create_dir_all(n.parent().unwrap()).unwrap();
    fs::write(&n, bytes).unwrap();
    let n = n.to_string_lossy().into_owned();
    fx.write_session(&fx.ws, session_json(json!({ &n: pending_new() }), json!([]), json!({})));
    let r = fx.run(Action::Reject, fx.file_target(&fx.ws, &n)).unwrap();
    assert_eq!(r.done, 1);
    assert!(!Path::new(&n).exists());
    let saved = fx.snapshots(&fx.ws, "bin/made.dat");
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert_eq!(fs::read(&saved[0]).unwrap(), bytes);
}

/// C1(b): reapply snapshots what it overwrites; a file outside the scope root lands under _outside.
#[test]
fn reapply_snapshots_the_file_it_overwrites() {
    let fx = Fx::new();
    let a = fx.file("src/a.txt", "the user's edit");
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(&fx.ws, session_json(json!({}), json!([]), json!({ &a: record(&a, Some("orig"), Some("claude"), t) })));
    fx.run(Action::Reapply, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(fs::read_to_string(&a).unwrap(), "claude");
    let saved = fx.snapshots(&fx.ws, "src/a.txt");
    assert_eq!(saved.len(), 1);
    assert_eq!(fs::read_to_string(&saved[0]).unwrap(), "the user's edit");
}

#[test]
fn a_file_outside_the_scope_root_is_snapshotted_under_outside() {
    let fx = Fx::new();
    let dir = fx.tmp.path().join("elsewhere");
    fs::create_dir_all(&dir).unwrap();
    let q = dir.join("q.txt");
    fs::write(&q, "claude").unwrap();
    let q = q.to_string_lossy().into_owned();
    fx.write_session(&fx.ws, session_json(json!({ &q: pending(Some("orig")) }), json!([]), json!({})));
    fx.run(Action::Reject, fx.file_target(&fx.ws, &q)).unwrap();
    assert_eq!(fs::read_to_string(&q).unwrap(), "orig");
    let rel = format!("_outside{q}");
    let saved = fx.snapshots(&fx.ws, &rel);
    assert_eq!(saved.len(), 1, "{rel}");
    assert_eq!(fs::read_to_string(&saved[0]).unwrap(), "claude");
}

/// C1(b): a file whose snapshot cannot be taken is refused and reported; the others still land.
#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_snapshotted_is_refused() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "claude");
    let unreadable = fx.file("secret.txt", "claude");
    fx.write_session(
        &fx.ws,
        session_json(json!({ &a: pending(Some("orig")), &unreadable: pending(Some("orig")) }), json!([]), json!({})),
    );
    chmod(Path::new(&unreadable), 0o200); // writable, not readable: the restore alone would succeed
    if fs::read(&unreadable).is_ok() {
        chmod(Path::new(&unreadable), 0o644);
        eprintln!("skipping: running as root");
        return;
    }
    let r = fx.run(Action::Reject, Target::All);
    chmod(Path::new(&unreadable), 0o644);
    let r = r.unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.errors[0].0, unreadable);
    assert!(r.errors[0].1.contains("Claude's version"), "{:?}", r.errors);
    assert_eq!(fs::read_to_string(&unreadable).unwrap(), "claude", "not restored without a snapshot");
    assert_eq!(fs::read_to_string(&a).unwrap(), "orig");
    assert_eq!(keys(&fx.session(&fx.ws)["files"]), vec![unreadable]);
}

// ── Revert ──────────────────────────────────────────────────────────────────

#[test]
fn revert_reopens_only_when_disk_equals_the_records_after() {
    let fx = Fx::new();
    let same = fx.file("same.txt", "accepted");
    let moved = fx.file("moved.txt", "edited since");
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(
        &fx.ws,
        session_json(
            json!({}),
            json!([record(&same, Some("before"), Some("accepted"), t), record(&moved, Some("before"), Some("accepted"), t)]),
            json!({}),
        ),
    );
    let r = fx.run(Action::Revert, Target::All).unwrap();
    assert_eq!(r.done, 2);
    let s = fx.session(&fx.ws);
    assert!(s["accepted"].as_array().unwrap().is_empty());
    assert_eq!(keys(&s["files"]), vec![same.clone()]);
    assert_eq!(s["files"][&same]["originalContent"], "before");
    assert_eq!(s["files"][&same]["capturedAt"], NOW);
    assert_eq!(s["status"], "active");
    assert_eq!(fs::read_to_string(&same).unwrap(), "accepted", "revert never touches the file");
}

#[test]
fn revert_file_targets_one_record_by_id() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "accepted");
    let b = fx.file("b.txt", "accepted");
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(
        &fx.ws,
        session_json(
            json!({}),
            json!([record(&a, Some("x"), Some("accepted"), t), record(&b, Some("x"), Some("accepted"), t)]),
            json!({}),
        ),
    );
    let r = fx.run(Action::Revert, fx.file_target(&fx.ws, &format!("{t}::{a}"))).unwrap();
    assert_eq!(r.done, 1);
    let s = fx.session(&fx.ws);
    assert_eq!(s["accepted"].as_array().unwrap().len(), 1);
    assert_eq!(s["accepted"][0]["path"], b.as_str());
    assert_eq!(keys(&s["files"]), vec![a]);
}

// ── Reapply ─────────────────────────────────────────────────────────────────

#[test]
fn reapply_writes_after_back_and_reopens_the_entry() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "orig");
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(&fx.ws, session_json(json!({}), json!([]), json!({ &a: record(&a, Some("orig"), Some("claude"), t) })));
    let r = fx.run(Action::Reapply, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(fs::read_to_string(&a).unwrap(), "claude");
    let s = fx.session(&fx.ws);
    assert!(keys(&s["rejected"]).is_empty());
    assert_eq!(s["files"][&a]["originalContent"], "orig");
    assert_eq!(s["files"][&a]["capturedAt"], NOW);
    assert_eq!(s["status"], "active");
}

#[test]
fn reapply_recreates_a_deleted_new_file() {
    let fx = Fx::new();
    let a = fx.ws.join("gone/new.txt").to_string_lossy().into_owned();
    let mut rec = record(&a, None, Some("claude"), "2026-11-16T11:00:00.000Z");
    rec["newFile"] = json!(true);
    fx.write_session(&fx.ws, session_json(json!({}), json!([]), json!({ &a: rec })));
    fx.run(Action::Reapply, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(fs::read_to_string(&a).unwrap(), "claude");
    let s = fx.session(&fx.ws);
    assert_eq!(s["files"][&a]["originalContent"], Value::Null);
    assert_eq!(s["files"][&a]["newFile"], true);
}

#[test]
fn reapply_without_a_saved_after_is_an_error_and_changes_nothing() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "orig");
    fx.write_session(
        &fx.ws,
        session_json(json!({}), json!([]), json!({ &a: record(&a, Some("orig"), None, "2026-11-16T11:00:00.000Z") })),
    );
    let before = snapshot(fx.tmp.path());
    let r = fx.run(Action::Reapply, fx.file_target(&fx.ws, &a)).unwrap();
    assert_eq!(r.done, 0);
    assert_eq!(r.errors.len(), 1);
    assert_eq!(r.errors[0].0, a);
    assert!(r.errors[0].1.contains("was not saved"), "{:?}", r.errors);
    assert_eq!(snapshot(fx.tmp.path()), before);
}

#[test]
fn reapply_all_skips_excluded_records() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "orig");
    let g = fx.file("gen/g.txt", "orig");
    fx.excludes(&["gen"]);
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(
        &fx.ws,
        session_json(
            json!({}),
            json!([]),
            json!({ &a: record(&a, Some("orig"), Some("claude"), t), &g: record(&g, Some("orig"), Some("claude"), t) }),
        ),
    );
    let r = fx.run(Action::Reapply, Target::All).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(fs::read_to_string(&a).unwrap(), "claude");
    assert_eq!(fs::read_to_string(&g).unwrap(), "orig");
    assert_eq!(keys(&fx.session(&fx.ws)["rejected"]), vec![g]);
}

// ── Clear ───────────────────────────────────────────────────────────────────

#[test]
fn clear_accepted_tombstones_ids_but_keeps_a_concurrent_accept() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "x");
    let b = fx.file("b.txt", "x");
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(&fx.ws, session_json(json!({}), json!([record(&a, Some("o"), Some("x"), t)]), json!({})));
    let sf = fx.session_file(&fx.ws);
    let r = decide_with(&fx.paths, &fx.ws, Action::ClearAccepted, Target::All, false, NOW, &mut |_| {
        // VS Code accepts another file between our load and our write.
        let mut v: Value = serde_json::from_str(&fs::read_to_string(&sf).unwrap()).unwrap();
        v["accepted"].as_array_mut().unwrap().push(record(&b, Some("o"), Some("x"), "2026-11-16T11:30:00.000Z"));
        fs::write(&sf, v.to_string()).unwrap();
    })
    .unwrap();
    assert_eq!(r.done, 1);
    let acc = fx.session(&fx.ws)["accepted"].as_array().unwrap().clone();
    assert_eq!(acc.len(), 1, "cleared record stays cleared, concurrent one survives: {acc:?}");
    assert_eq!(acc[0]["path"], b.as_str());
}

#[test]
fn clear_rejected_tombstones_paths() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "x");
    let b = fx.file("b.txt", "x");
    let t = "2026-11-16T11:00:00.000Z";
    fx.write_session(
        &fx.ws,
        session_json(json!({}), json!([]), json!({ &a: record(&a, Some("o"), Some("x"), t), &b: record(&b, Some("o"), Some("x"), t) })),
    );
    let r = fx.run(Action::ClearRejected, Target::All).unwrap();
    assert_eq!(r.done, 2);
    let s = fx.session(&fx.ws);
    assert!(keys(&s["rejected"]).is_empty(), "the disk copy must not resurrect them");
    assert_eq!(fs::read_to_string(&a).unwrap(), "x");
}

// ── Concurrency with the hook (Review Focus 1) ──────────────────────────────

#[test]
fn a_hook_capture_between_load_and_persist_survives() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "new");
    let late = fx.file("late.txt", "new");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("old")) }), json!([]), json!({})));
    let sf = fx.session_file(&fx.ws);
    decide_with(&fx.paths, &fx.ws, Action::Accept, Target::All, false, NOW, &mut |_| {
        let mut v: Value = serde_json::from_str(&fs::read_to_string(&sf).unwrap()).unwrap();
        let mut e = pending(Some("old"));
        e["capturedAt"] = json!("2026-11-16T11:59:00.000Z");
        v["files"][&late] = e;
        fs::write(&sf, v.to_string()).unwrap();
    })
    .unwrap();
    let s = fx.session(&fx.ws);
    assert_eq!(keys(&s["files"]), vec![late]);
    assert_eq!(s["status"], "active");
    assert_eq!(s["accepted"][0]["path"], a.as_str());
}

// ── Refusals (Review Focus 5) ───────────────────────────────────────────────

fn every_action() -> Vec<Action> {
    vec![Action::Accept, Action::Reject, Action::Revert, Action::Reapply, Action::ClearAccepted, Action::ClearRejected]
}

#[test]
fn read_only_refuses_every_action_and_changes_nothing() {
    let fx = Fx::new();
    let (a, ..) = all_fixture(&fx);
    let t = "2026-11-16T11:00:00.000Z";
    let mut v = fx.session(&fx.ws);
    v["accepted"] = json!([record(&a, Some("o"), Some("new"), t)]);
    v["rejected"] = json!({ &a: record(&a, Some("o"), Some("claude"), t) });
    fx.write_session(&fx.ws, v);
    let before = snapshot(fx.tmp.path());
    for action in every_action() {
        for target in [Target::All, fx.file_target(&fx.ws, &a), Target::Folder { scope: fx.ws_str(), prefix: fx.ws_str() }] {
            match decide(&fx.paths, &fx.ws, action, target, true, NOW) {
                Err(DecideError::ReadOnly(msg)) => assert!(msg.contains("read-only"), "{msg}"),
                other => panic!("{action:?}: expected ReadOnly, got {other:?}"),
            }
        }
    }
    assert_eq!(snapshot(fx.tmp.path()), before);
}

#[test]
fn an_unknown_shape_scope_refuses_the_whole_action() {
    let fx = Fx::new();
    let (_a, _ex, _noop, _outside, _d, wt) = all_fixture(&fx);
    let mut w = fx.session(&wt);
    w["futureField"] = json!(1);
    fx.write_session(&wt, w);
    let before = snapshot(fx.tmp.path());
    for action in every_action() {
        match decide(&fx.paths, &fx.ws, action, Target::All, false, NOW) {
            Err(DecideError::ReadOnly(msg)) => assert_eq!(msg, UNKNOWN_SHAPE_MESSAGE),
            other => panic!("{action:?}: expected ReadOnly, got {other:?}"),
        }
    }
    // Targeting the unknown scope alone is refused too.
    let d = fx.ws.join("wt1/d.txt").to_string_lossy().into_owned();
    assert!(matches!(fx.run(Action::Reject, fx.file_target(&wt, &d)), Err(DecideError::ReadOnly(_))));
    assert_eq!(snapshot(fx.tmp.path()), before);
    assert_eq!(
        UNKNOWN_SHAPE_MESSAGE,
        "This project was written by a newer ClaudeGate. Update the app to make decisions here."
    );
}

#[test]
fn unknown_scope_or_key_is_not_found() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "new");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("old")) }), json!([]), json!({})));
    let before = snapshot(fx.tmp.path());
    let elsewhere = fx.tmp.path().join("nope");
    assert!(matches!(fx.run(Action::Accept, fx.file_target(&elsewhere, &a)), Err(DecideError::NotFound)));
    let missing = fx.ws.join("missing.txt").to_string_lossy().into_owned();
    assert!(matches!(fx.run(Action::Reject, fx.file_target(&fx.ws, &missing)), Err(DecideError::NotFound)));
    assert!(matches!(fx.run(Action::Revert, fx.file_target(&fx.ws, "no::such")), Err(DecideError::NotFound)));
    assert!(matches!(fx.run(Action::Reapply, fx.file_target(&fx.ws, &a)), Err(DecideError::NotFound)));
    assert_eq!(snapshot(fx.tmp.path()), before);
}

// ── Backups ─────────────────────────────────────────────────────────────────

#[test]
fn every_session_write_makes_a_backup() {
    let fx = Fx::new();
    let a = fx.file("a.txt", "new");
    let b = fx.file("b.txt", "new");
    fx.write_session(&fx.ws, session_json(json!({ &a: pending(Some("old")), &b: pending(Some("old")) }), json!([]), json!({})));
    let steps: Vec<(Action, Target)> = vec![
        (Action::Accept, fx.file_target(&fx.ws, &a)),
        (Action::Reject, fx.file_target(&fx.ws, &b)),
        (Action::Revert, Target::All),
        (Action::Reapply, Target::All),
        (Action::ClearRejected, Target::All),
        (Action::ClearAccepted, Target::All),
    ];
    // The last two may have nothing left to clear; only count writes that did something.
    let mut expected = 0;
    let backups = fx.paths.desktop_home.join("backups").join(fx.session_file(&fx.ws).file_stem().unwrap());
    let list = || -> std::collections::BTreeSet<PathBuf> {
        fs::read_dir(&backups)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
            .unwrap_or_default()
    };
    for (action, target) in steps {
        let pre = fs::read(fx.session_file(&fx.ws)).unwrap();
        let seen = list();
        let r = fx.run(action, target).unwrap();
        if r.done > 0 {
            expected += 1;
            // Same-millisecond backups get a `-N` suffix, so name order is not age order.
            let new: Vec<PathBuf> = list().difference(&seen).cloned().collect();
            assert_eq!(new.len(), 1, "{action:?}");
            assert_eq!(fs::read(&new[0]).unwrap(), pre, "{action:?}: backup holds the pre-write session");
        }
        assert_eq!(fx.backups(&fx.ws), expected, "{action:?}");
    }
    assert!(expected >= 4);
}

// ── Fix round 1 ─────────────────────────────────────────────────────────────

/// F2: the primary persists (1 left, 1 restore error), then the worktree's persist fails. The
/// error carries the primary's report as `partial` and names the worktree's restored file.
#[cfg(unix)]
#[test]
fn a_later_scope_failing_keeps_the_earlier_scopes_report() {
    let fx = Fx::new();
    let wt = fx.worktree("wt1");
    let left = fx.file("maybe.txt", "content");
    let locked = fx.file("locked/ro.txt", "claude");
    let d = fx.file("wt1/d.txt", "claude");
    fx.write_session(&fx.ws, session_json(json!({ &left: pending(None), &locked: pending(Some("orig")) }), json!([]), json!({})));
    fx.write_session(&wt, session_json(json!({ &d: pending(Some("orig")) }), json!([]), json!({})));
    let locked_dir = fx.ws.join("locked");
    chmod(Path::new(&locked), 0o444);
    chmod(&locked_dir, 0o555);
    if perms_ignored(&locked_dir) {
        chmod(&locked_dir, 0o755);
        eprintln!("skipping: running as root");
        return;
    }
    let sessions = fx.paths.home.join("sessions");
    let wt_file = fx.session_file(&wt);
    let r = decide_with(&fx.paths, &fx.ws, Action::Reject, Target::All, false, NOW, &mut |f| {
        if f == wt_file {
            chmod(&sessions, 0o555);
        }
    });
    chmod(&sessions, 0o755);
    chmod(&locked_dir, 0o755);
    let e = r.unwrap_err();
    let DecideError::Store { changed, partial, .. } = &e else { panic!("expected Store, got {e:?}") };
    assert_eq!(changed, &vec![d.clone()]);
    assert_eq!(partial.done, 1);
    assert_eq!(partial.left, vec![left.clone()]);
    assert_eq!(partial.errors.len(), 1);
    assert_eq!(partial.errors[0].0, locked);
    let msg = e.to_string();
    assert!(msg.contains(&d) && msg.contains("1 file"), "{msg}");
    // The primary's decision is on disk; the worktree's log is untouched but its file restored.
    assert!(fx.session(&fx.ws)["rejected"].get(&left).is_some());
    assert!(keys(&fx.session(&wt)["files"]).contains(&d));
    assert_eq!(fs::read_to_string(&d).unwrap(), "orig");
}

/// F3: a folder prefix must be absolute and strictly under the scope root.
#[test]
fn a_bad_folder_prefix_is_refused_and_changes_nothing() {
    let fx = Fx::new();
    let (..) = all_fixture(&fx);
    let outside = fx.tmp.path().join("elsewhere").to_string_lossy().into_owned();
    let traversal = format!("{}/excl/../..", fx.ws_str());
    let before = snapshot(fx.tmp.path());
    for prefix in ["/".to_owned(), String::new(), "excl".to_owned(), outside, fx.ws_str(), traversal] {
        for action in every_action() {
            let target = Target::Folder { scope: fx.ws_str(), prefix: prefix.clone() };
            match fx.run(action, target) {
                Err(DecideError::BadTarget(_)) => {}
                other => panic!("{action:?} with prefix {prefix:?}: expected BadTarget, got {other:?}"),
            }
        }
    }
    assert_eq!(snapshot(fx.tmp.path()), before);
}

#[test]
fn a_corrupt_session_is_refused_and_changes_nothing() {
    for bytes in [b"{ not json".to_vec(), vec![b'{', 0xff, 0xfe, b'}']] {
        let fx = Fx::new();
        let a = fx.file("a.txt", "new");
        fs::write(fx.session_file(&fx.ws), &bytes).unwrap();
        let before = snapshot(fx.tmp.path());
        for action in every_action() {
            for target in [Target::All, fx.file_target(&fx.ws, &a)] {
                match fx.run(action, target) {
                    Err(DecideError::ReadOnly(msg)) => assert!(msg.contains("could not be read"), "{msg}"),
                    other => panic!("{action:?}: expected ReadOnly, got {other:?}"),
                }
            }
        }
        assert_eq!(snapshot(fx.tmp.path()), before);
    }
}
