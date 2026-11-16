//! The decide command's plain logic and the read-only switch. Temp homes only: the real
//! ~/.claudegate and ~/.claudegate-desktop are never touched.
use claudegate_core::decide::{DecideError, DecidePaths, DecideReport};
use claudegate_core::store::StoreError;
use claudegate_core::paths::session_file_for;
use claudegate_desktop_lib::commands::{desktop_home_for, error_json, iso_from_ms, run_decide, TargetDto};
use claudegate_desktop_lib::config::{load_or_create, save_editor, save_read_only, VSCODE};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-11-16T12:00:00.000Z";

struct Fx {
    _tmp: tempfile::TempDir,
    paths: DecidePaths,
    ws: PathBuf,
    cfg: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("cg");
        let desktop_home = tmp.path().join("cgd");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::create_dir_all(&ws).unwrap();
        let cfg = desktop_home.join("config.json");
        Fx { paths: DecidePaths { home, desktop_home }, ws, cfg, _tmp: tmp }
    }
    fn root(&self) -> String {
        self.ws.to_string_lossy().into_owned()
    }
    fn session_file(&self) -> PathBuf {
        session_file_for(&self.paths.home, &self.ws)
    }
    /// Pending entries for each (name, original) pair; the files on disk hold "new".
    fn seed(&self, files: &[(&str, &str)]) -> Vec<String> {
        let mut m = serde_json::Map::new();
        let mut keys = vec![];
        for (name, orig) in files {
            let p = self.ws.join(name);
            fs::write(&p, "new").unwrap();
            let k = p.to_string_lossy().into_owned();
            m.insert(
                k.clone(),
                json!({ "originalContent": orig, "reviewStatus": "pending", "sessionId": "s1",
                        "capturedAt": "2026-11-16T10:00:00.000Z" }),
            );
            keys.push(k);
        }
        let v = json!({ "sessionId": "2026-11-16T09:00:00.000Z", "status": "active",
                        "files": m, "accepted": [], "rejected": {} });
        fs::write(self.session_file(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
        keys
    }
    fn session(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.session_file()).unwrap()).unwrap()
    }
    fn run(&self, lock: &Mutex<()>, action: &str, t: &TargetDto) -> claudegate_desktop_lib::commands::DecideOutcome {
        run_decide(lock, &self.paths, &self.cfg, &self.root(), action, t, NOW)
    }
}

fn file(scope: &str, key: &str) -> TargetDto {
    TargetDto { kind: "file".into(), scope: Some(scope.into()), key: Some(key.into()), prefix: None, keys: None }
}

#[test]
fn unknown_action_is_an_error_before_anything_is_loaded() {
    let fx = Fx::new();
    let keys = fx.seed(&[("a.txt", "old")]);
    let before = fs::read_to_string(fx.session_file()).unwrap();
    let out = fx.run(&Mutex::new(()), "obliterate", &file(&fx.root(), &keys[0]));
    let e: Value = serde_json::from_str(&out.result.unwrap_err()).unwrap();
    assert_eq!(e["kind"], "invalid");
    assert!(!out.changed);
    assert_eq!(fs::read_to_string(fx.session_file()).unwrap(), before);
    assert!(!fx.paths.desktop_home.join("backups").exists());
}

#[test]
fn malformed_targets_are_errors() {
    let fx = Fx::new();
    fx.seed(&[("a.txt", "old")]);
    let lock = Mutex::new(());
    let folder = TargetDto { kind: "folder".into(), scope: Some(fx.root()), key: None, prefix: None, keys: None };
    let nokey = TargetDto { kind: "file".into(), scope: Some(fx.root()), key: None, prefix: None, keys: None };
    let noscope = TargetDto { kind: "file".into(), scope: None, key: Some("x".into()), prefix: None, keys: None };
    let weird = TargetDto { kind: "galaxy".into(), scope: None, key: None, prefix: None, keys: None };
    for t in [folder, nokey, noscope, weird] {
        let out = fx.run(&lock, "accept", &t);
        let e: Value = serde_json::from_str(&out.result.unwrap_err()).unwrap();
        assert_eq!(e["kind"], "invalid");
        assert!(!out.changed);
    }
    assert_eq!(fx.session()["files"].as_object().unwrap().len(), 1);
}

#[test]
fn read_only_config_refuses_and_leaves_the_sandbox_alone() {
    let fx = Fx::new();
    let keys = fx.seed(&[("a.txt", "old")]);
    save_read_only(&fx.cfg, true, None).unwrap();
    let before = fs::read_to_string(fx.session_file()).unwrap();
    let out = fx.run(&Mutex::new(()), "reject", &file(&fx.root(), &keys[0]));
    let e: Value = serde_json::from_str(&out.result.unwrap_err()).unwrap();
    assert_eq!(e["kind"], "readOnly");
    assert!(e["message"].as_str().unwrap().contains("read-only"));
    assert_eq!(fs::read_to_string(fx.session_file()).unwrap(), before);
    assert_eq!(fs::read_to_string(&keys[0]).unwrap(), "new");
    assert!(!fx.paths.desktop_home.join("backups").exists());
    // flipping the switch takes effect on the very next call: nothing is cached
    save_read_only(&fx.cfg, false, None).unwrap();
    let out = fx.run(&Mutex::new(()), "accept", &file(&fx.root(), &keys[0]));
    assert_eq!(out.result.unwrap().done, 1);
    assert!(out.changed);
}

#[test]
fn concurrent_decides_on_one_project_both_apply() {
    let fx = Arc::new(Fx::new());
    let keys = fx.seed(&[("a.txt", "old"), ("b.txt", "old")]);
    let lock = Arc::new(Mutex::new(()));
    let handles: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let (fx, lock, k) = (fx.clone(), lock.clone(), k.clone());
            std::thread::spawn(move || {
                let action = if i == 0 { "accept" } else { "reject" };
                fx.run(&lock, action, &file(&fx.root(), &k)).result.unwrap().done
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), 1);
    }
    let s = fx.session();
    assert!(s["files"].as_object().unwrap().is_empty());
    assert_eq!(s["status"], "reviewed");
    let accepted = s["accepted"].as_array().unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0]["path"], keys[0]);
    assert!(s["rejected"].as_object().unwrap().contains_key(&keys[1]));
    assert_eq!(fs::read_to_string(&keys[0]).unwrap(), "new");
    assert_eq!(fs::read_to_string(&keys[1]).unwrap(), "old");
}

#[test]
fn set_read_only_persists_and_save_editor_keeps_it() {
    let fx = Fx::new();
    fs::create_dir_all(fx.cfg.parent().unwrap()).unwrap();
    fs::write(&fx.cfg, r#"{"editor":"zed {path}:{line}","extra":{"k":1}}"#).unwrap();
    let c = save_read_only(&fx.cfg, true, None).unwrap();
    assert!(c.read_only);
    assert!(load_or_create(&fx.cfg).read_only);
    let c = save_editor(&fx.cfg, VSCODE, None).unwrap();
    assert!(c.read_only);
    assert_eq!(c.editor, VSCODE);
    let v: Value = serde_json::from_str(&fs::read_to_string(&fx.cfg).unwrap()).unwrap();
    assert_eq!(v["readOnly"], true);
    assert_eq!(v["extra"], json!({"k":1}));
    // and the reverse: turning it off keeps the editor
    let c = save_read_only(&fx.cfg, false, None).unwrap();
    assert!(!c.read_only);
    assert_eq!(c.editor, VSCODE);
    // the guard refuses a config path inside the ClaudeGate home
    assert!(save_read_only(&fx.cfg, true, Some(fx.paths.desktop_home.as_path())).is_err());
}

#[test]
fn error_json_shapes() {
    let ro = serde_json::from_str::<Value>(&error_json(&DecideError::ReadOnly("nope".into()))).unwrap();
    assert_eq!(ro, json!({"kind":"readOnly","message":"nope"}));
    let nf = serde_json::from_str::<Value>(&error_json(&DecideError::NotFound)).unwrap();
    assert_eq!(nf["kind"], "notFound");
    let bt = serde_json::from_str::<Value>(&error_json(&DecideError::BadTarget("x".into()))).unwrap();
    assert_eq!(bt, json!({"kind":"badTarget","message":"x"}));
}

#[test]
fn iso_matches_to_iso_string() {
    assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso_from_ms(1_763_294_400_123), "2025-11-16T12:00:00.123Z");
    assert_eq!(iso_from_ms(951_782_400_000), "2000-02-29T00:00:00.000Z");
}

#[test]
fn store_error_json_carries_changed_and_partial() {
    let partial = DecideReport { done: 2, left: vec!["l".into()], errors: vec![("p".into(), "m".into())] };
    let e = DecideError::Store {
        error: StoreError::Write("disk full".into()),
        changed: vec!["/ws/a.txt".into()],
        partial,
        snapshot: Some("/d/backups/h/files/20261116T120000.000Z".into()),
    };
    let v: Value = serde_json::from_str(&error_json(&e)).unwrap();
    assert_eq!(v["kind"], "store");
    assert!(v["message"].as_str().unwrap().contains("disk full"));
    assert_eq!(v["changed"], json!(["/ws/a.txt"]));
    assert_eq!(v["partial"], json!({"done":2,"left":["l"],"errors":[["p","m"]]}));
    assert_eq!(v["snapshot"], "/d/backups/h/files/20261116T120000.000Z");
    let none = DecideError::Store {
        error: StoreError::Backup("x".into()),
        changed: vec![],
        partial: DecideReport::default(),
        snapshot: None,
    };
    let v: Value = serde_json::from_str(&error_json(&none)).unwrap();
    assert_eq!(v["snapshot"], Value::Null);
}

#[test]
fn desktop_home_is_absolute_and_outside_the_claudegate_home() {
    let cg = std::path::Path::new("/h/.claudegate");
    assert_eq!(
        desktop_home_for(std::path::Path::new("/h/.claudegate-desktop/config.json"), cg).unwrap(),
        PathBuf::from("/h/.claudegate-desktop")
    );
    assert!(desktop_home_for(std::path::Path::new("config.json"), cg).is_err());
    assert!(desktop_home_for(std::path::Path::new(""), cg).is_err());
    assert!(desktop_home_for(std::path::Path::new("/h/.claudegate/config.json"), cg).is_err());
    assert!(desktop_home_for(std::path::Path::new("/h/.claudegate/x/config.json"), cg).is_err());
}

#[test]
fn run_decide_refuses_an_unsafe_desktop_home_before_touching_anything() {
    let mut fx = Fx::new();
    let keys = fx.seed(&[("a.txt", "old")]);
    let before = fs::read_to_string(fx.session_file()).unwrap();
    for bad in [PathBuf::from("rel/dir"), fx.paths.home.join("desk")] {
        fx.paths.desktop_home = bad;
        let out = fx.run(&Mutex::new(()), "accept", &file(&fx.root(), &keys[0]));
        let e: Value = serde_json::from_str(&out.result.unwrap_err()).unwrap();
        assert_eq!(e["kind"], "invalid");
        assert!(!out.changed);
        assert_eq!(fs::read_to_string(fx.session_file()).unwrap(), before);
    }
    assert!(!std::path::Path::new("rel").exists());
}

#[test]
fn malformed_config_pauses_decisions_and_a_missing_one_does_not() {
    let fx = Fx::new();
    let keys = fx.seed(&[("a.txt", "old")]);
    let before = fs::read_to_string(fx.session_file()).unwrap();
    for bad in ["{ not json", "[]", r#"{"readOnly":"yes"}"#] {
        fs::create_dir_all(fx.cfg.parent().unwrap()).unwrap();
        fs::write(&fx.cfg, bad).unwrap();
        let out = fx.run(&Mutex::new(()), "accept", &file(&fx.root(), &keys[0]));
        let e: Value = serde_json::from_str(&out.result.unwrap_err()).unwrap();
        assert_eq!(e["kind"], "readOnly");
        assert!(e["message"].as_str().unwrap().contains("couldn't read its settings file"));
        assert!(!out.changed);
        assert_eq!(fs::read_to_string(fx.session_file()).unwrap(), before);
        assert_eq!(fs::read_to_string(&fx.cfg).unwrap(), bad, "config must be left alone");
    }
    // missing: defaults, decision applies, and decide does not create the config
    fs::remove_file(&fx.cfg).unwrap();
    let out = fx.run(&Mutex::new(()), "accept", &file(&fx.root(), &keys[0]));
    assert_eq!(out.result.unwrap().done, 1);
    assert!(!fx.cfg.exists());
}

/// I4: the UI sends the keys it counted; a capture landing after the count is not decided.
#[test]
fn bulk_keys_limit_the_action_to_what_was_confirmed() {
    let fx = Fx::new();
    let keys = fx.seed(&[("a.txt", "old"), ("late.txt", "old")]);
    let all = TargetDto { kind: "all".into(), scope: None, key: None, prefix: None, keys: Some(vec![keys[0].clone()]) };
    let out = fx.run(&Mutex::new(()), "accept", &all);
    assert_eq!(out.result.unwrap().done, 1);
    let files = fx.session()["files"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(files, vec![keys[1].clone()]);
    let dto: TargetDto = serde_json::from_value(json!({ "kind": "folder", "scope": "/w", "prefix": "/w/src", "keys": ["/w/src/a"] })).unwrap();
    assert_eq!(dto.keys, Some(vec!["/w/src/a".to_owned()]));
    let dto: TargetDto = serde_json::from_value(json!({ "kind": "all" })).unwrap();
    assert_eq!(dto.keys, None);
}
