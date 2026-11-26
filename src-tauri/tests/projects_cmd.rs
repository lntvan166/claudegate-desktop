//! Add / remove project commands: `workspace-roots.json` in a temp ClaudeGate home and
//! `projects[root].addedByApp` in a temp config.json. The real ~/.claudegate and
//! ~/.claudegate-desktop are never touched.
use claudegate_desktop_lib::commands::{add_project_with, ensure_roots_with, remove_project_with};
use claudegate_desktop_lib::config::{app_roots, config_path_in, load_or_create};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

struct Fx {
    _t: tempfile::TempDir,
    cg: PathBuf,
    cfg: PathBuf,
    ws: PathBuf,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let cg = t.path().join("cg");
    let ws = t.path().join("ws");
    fs::create_dir_all(&cg).unwrap();
    fs::create_dir_all(&ws).unwrap();
    let cfg = config_path_in(&t.path().join("home"));
    Fx { cg, cfg, ws, _t: t }
}

impl Fx {
    fn dir(&self, n: &str) -> String {
        let p = self.ws.join(n);
        fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    }
    fn roots(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.cg.join("workspace-roots.json")).unwrap()).unwrap()
    }
    fn config(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(&self.cfg).unwrap()).unwrap()
    }
}

#[test]
fn add_records_ownership_and_keeps_other_config_keys() {
    let f = fx();
    let vs = f.dir("vs");
    fs::write(f.cg.join("workspace-roots.json"), json!([vs]).to_string()).unwrap();
    fs::create_dir_all(f.cfg.parent().unwrap()).unwrap();
    fs::write(&f.cfg, json!({ "editor": "hx {path}", "projects": { "/x": { "extraHide": ["a"] } } }).to_string()).unwrap();
    let app = f.dir("app");
    let added = add_project_with(&f.cg, &f.cfg, &app).unwrap();
    assert_eq!(added.root, app);
    assert_eq!(f.roots(), json!([vs, app]));
    let c = f.config();
    assert_eq!(c["projects"][&app]["addedByApp"], json!(true));
    assert_eq!(c["projects"]["/x"]["extraHide"], json!(["a"]));
    assert_eq!(c["editor"], json!("hx {path}"));
    assert_eq!(app_roots(&f.cfg), vec![app.clone()]);
    assert_eq!(load_or_create(&f.cfg).app_roots, vec![app]);
}

#[test]
fn nested_add_is_refused_with_a_message_naming_the_root() {
    let f = fx();
    let vs = f.dir("vs");
    f.dir("vs/sub");
    fs::write(f.cg.join("workspace-roots.json"), json!([vs]).to_string()).unwrap();
    let err = add_project_with(&f.cg, &f.cfg, &format!("{vs}/sub")).unwrap_err();
    assert!(err.contains(&vs), "{err}");
    assert!(!f.cfg.exists(), "nothing recorded");
}

#[test]
fn remove_refuses_vs_code_roots_and_drops_the_app_record() {
    let f = fx();
    let vs = f.dir("vs");
    fs::write(f.cg.join("workspace-roots.json"), json!([vs]).to_string()).unwrap();
    let err = remove_project_with(&f.cg, &f.cfg, &vs, &[]).unwrap_err();
    assert!(err.contains("VS Code"), "{err}");
    assert_eq!(f.roots(), json!([vs]));

    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    claudegate_desktop_lib::config::save_extra_hide(&f.cfg, &app, &["x".into()], None).unwrap();
    assert!(remove_project_with(&f.cg, &f.cfg, &app, &[]).unwrap().unregistered);
    assert_eq!(f.roots(), json!([vs]));
    assert!(f.config()["projects"].get(&app).is_none());
    assert!(app_roots(&f.cfg).is_empty());
}

#[test]
fn ensure_re_adds_a_dropped_app_root_once() {
    let f = fx();
    let vs = f.dir("vs");
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    fs::write(f.cg.join("workspace-roots.json"), json!([vs]).to_string()).unwrap();
    assert_eq!(ensure_roots_with(&f.cg, &f.cfg), vec![app.clone()]);
    assert_eq!(f.roots(), json!([vs, app]));
    assert!(ensure_roots_with(&f.cg, &f.cfg).is_empty());
}

#[test]
fn ensure_with_no_config_writes_nothing() {
    let f = fx();
    assert!(ensure_roots_with(&f.cg, Path::new(&f.cfg)).is_empty());
    assert!(!f.cg.join("workspace-roots.json").exists());
    assert!(!f.cfg.exists());
}

#[test]
fn read_only_blocks_add_and_remove() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    claudegate_desktop_lib::config::save_read_only(&f.cfg, true, None).unwrap();
    let before = f.roots();
    let other = f.dir("other");
    assert!(add_project_with(&f.cg, &f.cfg, &other).unwrap_err().contains("Read-only mode is on"));
    assert!(remove_project_with(&f.cg, &f.cfg, &app, &[]).unwrap_err().contains("Read-only mode is on"));
    assert_eq!(f.roots(), before);
    assert_eq!(app_roots(&f.cfg), vec![app]);
}

#[test]
fn add_joins_the_workspace_and_remove_clears_every_spelling() {
    let f = fx();
    let app = f.dir("app");
    let keep = f.dir("keep");
    claudegate_desktop_lib::config::add_to_workspace(&f.cfg, &f.cg, &keep, None).unwrap();
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    assert_eq!(f.config()["workspace"], json!([keep, app]));
    // A second spelling of the same root in projects.
    claudegate_desktop_lib::config::save_extra_hide(&f.cfg, &format!("{app}/"), &["x".into()], None).unwrap();
    remove_project_with(&f.cg, &f.cfg, &app, &[]).unwrap();
    assert_eq!(f.config()["projects"], json!({}));
}

#[test]
fn remove_of_a_root_vs_code_uses_keeps_it_registered() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let storage = f.ws.join("storage.json");
    fs::write(&storage, json!({ "windowsState": { "lastActiveWindow": { "folder": format!("file://{app}") } } }).to_string()).unwrap();
    let r = remove_project_with(&f.cg, &f.cfg, &app, &[storage]).unwrap();
    assert!(!r.unregistered);
    assert_eq!(f.roots(), json!([app]));
    assert!(app_roots(&f.cfg).is_empty());
}
