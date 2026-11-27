//! `get_audit` / `dismiss_audit` minus Tauri. Everything lives in temp dirs.
use claudegate_core::audit::WINDOW_MS;
use claudegate_core::paths::{audit_file_for, session_file_for};
use claudegate_core::settings::default_ext_sources;
use claudegate_desktop_lib::commands::get_audit_at;
use claudegate_desktop_lib::config::dismiss_audit_paths;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const NOW: i64 = 1_794_564_000_000; // 2026-11-13T10:00:00Z
const AT: &str = "2026-11-13T09:00:00+00:00";
const AT_MS: i64 = NOW - 3_600_000;

struct Fx {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    home: PathBuf,
    cfg: PathBuf,
    parent: PathBuf,
    nested: PathBuf,
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(tmp.path()).unwrap();
    let home = base.join("cg");
    let cfg = base.join("desktop").join("config.json");
    let parent = base.join("acme");
    let nested = parent.join("acme-api");
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(home.join("workspace-roots.json"), json!([s(&parent), s(&nested)]).to_string()).unwrap();
    Fx { _tmp: tmp, base, home, cfg, parent, nested }
}

fn audit_line(root: &Path, missed: &[PathBuf], uncertain: &[PathBuf]) -> String {
    let m: Vec<String> = missed.iter().map(|p| s(p)).collect();
    let u: Vec<String> = uncertain.iter().map(|p| s(p)).collect();
    json!({
        "at": AT, "root": s(root), "commandHead": "gofmt -w ./...", "background": false, "truncated": false,
        "missed": m, "uncertain": u, "phantom": [], "missedCount": m.len(), "uncertainCount": u.len(),
        "phantomCount": 0, "covered": 0, "implicit": 0, "outOfScope": 0, "tooLarge": 0, "captured": 0
    })
    .to_string()
}

fn put_audit(f: &Fx, root: &Path, lines: &[String]) {
    let file = audit_file_for(&f.home, root);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, lines.join("\n") + "\n").unwrap();
}

fn get(f: &Fx, root: &Path, now: i64) -> claudegate_core::audit::AuditView {
    get_audit_at(&f.home, &f.cfg, &s(root), &default_ext_sources(&f.base), now)
}

fn pend(f: &Fx, root: &Path, files: &[&PathBuf]) {
    let e = json!({ "originalContent": "old", "reviewStatus": "pending", "capturedAt": "2026-01-01T00:00:00Z" });
    let map: serde_json::Map<String, Value> = files.iter().map(|p| (s(p), e.clone())).collect();
    let session = json!({ "sessionId": "x", "status": "active", "files": map, "accepted": [], "rejected": {} });
    fs::write(session_file_for(&f.home, root), session.to_string()).unwrap();
}

fn touch(p: &PathBuf) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, "new").unwrap();
}

#[test]
fn nested_scope_included() {
    let f = fx();
    let a = f.parent.join("a.go");
    let b = f.nested.join("b.go");
    put_audit(&f, &f.parent, &[audit_line(&f.parent, &[a.clone()], &[])]);
    put_audit(&f, &f.nested, &[audit_line(&f.nested, &[b.clone()], &[])]);
    let v = get(&f, &f.parent, NOW);
    let mut got: Vec<String> = v.missed.iter().map(|i| i.path.clone()).collect();
    got.sort();
    assert_eq!(got, vec![s(&a), s(&b)]);
    assert_eq!(v.stats.calls, 2);
}

#[test]
fn pending_now_hidden() {
    let f = fx();
    let a = f.parent.join("a.go");
    let b = f.parent.join("b.go");
    let c = f.parent.join("c.go");
    for p in [&a, &b, &c] {
        touch(p);
    }
    put_audit(&f, &f.parent, &[audit_line(&f.parent, &[a.clone(), b.clone()], &[c.clone()])]);
    pend(&f, &f.parent, &[&a, &c]);
    let v = get(&f, &f.parent, NOW);
    assert_eq!(v.missed.iter().map(|i| i.path.clone()).collect::<Vec<_>>(), vec![s(&b)]);
    assert!(v.uncertain.is_empty());
    // Stats are not filtered by what is pending.
    assert_eq!((v.stats.missed, v.stats.uncertain), (2, 1));
}

#[test]
fn dismissed_hidden_until_newer_miss() {
    let f = fx();
    let a = f.parent.join("a.go");
    let b = f.parent.join("b.go");
    put_audit(&f, &f.parent, &[audit_line(&f.parent, &[a.clone(), b.clone()], &[])]);
    // Dismissed after the miss: hidden. Stats keep counting the call and its misses.
    dismiss_audit_paths(&f.cfg, &[s(&a)], AT_MS + 1000, WINDOW_MS, None).unwrap();
    let v = get(&f, &f.parent, NOW);
    assert_eq!(v.missed.iter().map(|i| i.path.clone()).collect::<Vec<_>>(), vec![s(&b)]);
    assert_eq!((v.stats.calls, v.stats.missed), (1, 2));
    // A newer miss of the same file brings it back.
    let mut newer: Value = serde_json::from_str(&audit_line(&f.parent, &[a.clone()], &[])).unwrap();
    newer["at"] = json!("2026-11-13T09:30:00+00:00");
    put_audit(&f, &f.parent, &[audit_line(&f.parent, &[a.clone(), b.clone()], &[]), newer.to_string()]);
    let v = get(&f, &f.parent, NOW);
    assert_eq!(v.missed.iter().map(|i| i.path.clone()).collect::<Vec<_>>(), vec![s(&a), s(&b)]);
}

#[test]
fn window_is_seven_days_before_now() {
    let f = fx();
    let a = f.parent.join("a.go");
    put_audit(&f, &f.parent, &[audit_line(&f.parent, &[a.clone()], &[])]);
    assert_eq!(get(&f, &f.parent, AT_MS + WINDOW_MS - 1).missed.len(), 1);
    assert_eq!(get(&f, &f.parent, AT_MS + WINDOW_MS + 1).missed.len(), 0);
    assert_eq!(get(&f, &f.parent, AT_MS + WINDOW_MS + 1).stats.calls, 0);
}

#[test]
fn dismiss_keeps_other_keys_and_prunes_old_entries() {
    let f = fx();
    fs::write(
        &f.cfg,
        json!({
            "editor": "zed {path}", "projects": { "/work/acme": { "extraHide": ["x"] } },
            "mystery": [1, 2],
            "auditDismissed": { "/old": NOW - WINDOW_MS - 1, "/recent": NOW - 1000 }
        })
        .to_string(),
    )
    .unwrap();
    dismiss_audit_paths(&f.cfg, &["/new".into(), "  ".into()], NOW, WINDOW_MS, None).unwrap();
    let v: Value = serde_json::from_str(&fs::read_to_string(&f.cfg).unwrap()).unwrap();
    assert_eq!(v["editor"], "zed {path}");
    assert_eq!(v["mystery"], json!([1, 2]));
    assert_eq!(v["projects"]["/work/acme"]["extraHide"], json!(["x"]));
    assert_eq!(v["auditDismissed"], json!({ "/recent": NOW - 1000, "/new": NOW }));
}

#[test]
fn dismiss_refuses_a_config_inside_the_claudegate_home() {
    let f = fx();
    let inside = f.home.join("config.json");
    assert!(dismiss_audit_paths(&inside, &["/a".into()], NOW, WINDOW_MS, Some(&f.home)).is_err());
    assert!(!inside.exists());
}
