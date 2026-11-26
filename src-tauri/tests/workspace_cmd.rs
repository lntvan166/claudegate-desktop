//! The workspace: the roots the user added to the rail (`workspace` in config.json). Temp
//! ClaudeGate home and temp config.json only; the real ~/.claudegate and ~/.claudegate-desktop
//! are never touched.
use claudegate_desktop_lib::commands::{add_project_with, remove_from_workspace_with};
use claudegate_desktop_lib::config::{add_to_workspace, config_path_in, ensure_workspace, load_or_create, remove_from_workspace, save_read_only};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

struct Fx {
    _t: tempfile::TempDir,
    cg: PathBuf,
    cfg: PathBuf,
    ws: PathBuf,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let base = fs::canonicalize(t.path()).unwrap();
    let cg = base.join("cg");
    let ws = base.join("ws");
    fs::create_dir_all(&cg).unwrap();
    fs::create_dir_all(&ws).unwrap();
    let cfg = config_path_in(&base.join("home"));
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    Fx { cg, cfg, ws, _t: t }
}

impl Fx {
    fn dir(&self, n: &str) -> String {
        let p = self.ws.join(n);
        fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    }
    fn set_roots(&self, roots: &[&String]) {
        fs::write(self.cg.join("workspace-roots.json"), json!(roots).to_string()).unwrap();
    }
    fn roots_bytes(&self) -> Option<Vec<u8>> {
        fs::read(self.cg.join("workspace-roots.json")).ok()
    }
    fn config(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(&self.cfg).unwrap()).unwrap()
    }
    fn write_config(&self, v: serde_json::Value) {
        fs::write(&self.cfg, v.to_string()).unwrap();
    }
}

#[test]
fn migration_derives_the_workspace_from_the_hidden_list_and_drops_it_in_one_write() {
    let f = fx();
    let (a, b, c) = (f.dir("shop-api"), f.dir("umbrella"), f.dir("field-notes"));
    let nested = f.dir("umbrella/umbrella-orders");
    f.set_roots(&[&a, &b, &nested, &c]);
    // `b` hidden under another spelling of the same folder.
    f.write_config(json!({
        "editor": "hx {path}",
        "hiddenProjects": [format!("{b}/")],
        "hiddenAt": { format!("{b}/"): "2026-11-20T10:00:00.000Z" },
        "projects": { "/x": { "extraHide": ["a"] } },
    }));
    let c1 = ensure_workspace(&f.cfg, &f.cg, Some(&f.cg)).unwrap();
    assert_eq!(c1.workspace, Some(vec![a.clone(), c.clone()]), "listed order, hidden left out, nested folded");
    let v = f.config();
    assert_eq!(v["workspace"], json!([a, c]));
    assert!(v.get("hiddenProjects").is_none() && v.get("hiddenAt").is_none(), "dropped in the same write");
    assert_eq!(v["editor"], json!("hx {path}"));
    assert_eq!(v["projects"]["/x"]["extraHide"], json!(["a"]));

    // Present: never derived again, nothing written, even a later root stays out.
    let d = f.dir("tide-sim");
    f.set_roots(&[&a, &b, &c, &d]);
    let before = fs::read(&f.cfg).unwrap();
    let c2 = ensure_workspace(&f.cfg, &f.cg, Some(&f.cg)).unwrap();
    assert_eq!(c2.workspace, Some(vec![a, c]));
    assert_eq!(fs::read(&f.cfg).unwrap(), before);
}

#[test]
fn migration_of_a_missing_config_lists_every_root_and_an_empty_workspace_stays_empty() {
    let f = fx();
    let a = f.dir("shop-api");
    f.set_roots(&[&a]);
    let c = ensure_workspace(&f.cfg, &f.cg, None).unwrap();
    assert_eq!(c.workspace, Some(vec![a.clone()]));
    remove_from_workspace(&f.cfg, &f.cg, &a, &[], None).unwrap();
    let c = ensure_workspace(&f.cfg, &f.cg, None).unwrap();
    assert_eq!(c.workspace, Some(vec![]), "an empty list is the user's, not a missing one");
    assert_eq!(f.config()["workspace"], json!([]));
}

#[test]
fn migration_from_an_unreadable_roots_file_writes_nothing() {
    let f = fx();
    let a = f.dir("shop-api");
    f.write_config(json!({ "editor": "hx {path}", "hiddenProjects": [a], "hiddenAt": { &a: "2026-11-20T10:00:00.000Z" } }));
    let before = fs::read(&f.cfg).unwrap();
    for bad in [format!("[\"{a}\", \"/w/hal"), "{\"not\": \"a list\"}".to_string(), "[1, 2]".to_string()] {
        fs::write(f.cg.join("workspace-roots.json"), &bad).unwrap();
        assert!(ensure_workspace(&f.cfg, &f.cg, None).is_err(), "{bad}");
        assert!(add_to_workspace(&f.cfg, &f.cg, &a, None).is_err(), "{bad}");
        assert!(remove_from_workspace(&f.cfg, &f.cg, &a, &[], None).is_err(), "{bad}");
        assert_eq!(fs::read(&f.cfg).unwrap(), before, "config.json untouched for {bad}");
    }
    // Missing reads as no roots: the workspace is derived empty, the hidden list dropped.
    fs::remove_file(f.cg.join("workspace-roots.json")).unwrap();
    assert_eq!(ensure_workspace(&f.cfg, &f.cg, None).unwrap().workspace, Some(vec![]));
    assert!(f.config().get("hiddenProjects").is_none());
}

#[test]
fn the_config_reports_the_workspace_resolved_and_add_matches_it() {
    let f = fx();
    let a = f.dir("shop-api");
    f.write_config(json!({ "workspace": [format!("{a}/./"), format!("{a}/")] }));
    let c = add_to_workspace(&f.cfg, &f.cg, &a, None).unwrap();
    assert_eq!(c.workspace, Some(vec![a.clone()]), "one entry, spelled as the rail spells it");
    assert_eq!(load_or_create(&f.cfg).workspace, Some(vec![a]));
}

#[test]
fn migration_refuses_a_config_inside_the_claudegate_home() {
    let f = fx();
    let inside = f.cg.join("config.json");
    assert!(ensure_workspace(&inside, &f.cg, Some(&f.cg)).is_err());
}

#[test]
fn adding_is_add_only_under_concurrency() {
    let f = fx();
    let keep = f.dir("keep");
    f.write_config(json!({ "editor": "hx {path}", "workspace": [keep] }));
    let roots: Vec<String> = (0..16).map(|i| f.dir(&format!("p{i}"))).collect();
    std::thread::scope(|s| {
        for r in &roots {
            let (cfg, cg) = (f.cfg.clone(), f.cg.clone());
            s.spawn(move || add_to_workspace(&cfg, &cg, r, None).unwrap());
        }
    });
    let ws = load_or_create(&f.cfg).workspace.unwrap();
    assert_eq!(ws.len(), 17);
    assert_eq!(ws[0], keep);
    for r in &roots {
        assert!(ws.contains(r), "{r} lost to a concurrent add");
    }
    // Another spelling of a listed root is not added twice.
    let c = add_to_workspace(&f.cfg, &f.cg, &format!("{}/", roots[3]), None).unwrap();
    assert_eq!(c.workspace.unwrap().len(), 17);
    assert_eq!(f.config()["editor"], json!("hx {path}"));
}

#[test]
fn removing_is_remove_only_under_concurrency() {
    let f = fx();
    let roots: Vec<String> = (0..16).map(|i| f.dir(&format!("p{i}"))).collect();
    f.write_config(json!({ "workspace": roots }));
    std::thread::scope(|s| {
        for r in roots.iter().step_by(2) {
            let (cfg, cg) = (f.cfg.clone(), f.cg.clone());
            s.spawn(move || remove_from_workspace(&cfg, &cg, r, &[], None).unwrap());
        }
    });
    let left: Vec<String> = roots.iter().skip(1).step_by(2).cloned().collect();
    assert_eq!(load_or_create(&f.cfg).workspace.unwrap(), left, "the rest, in the user's order");
}

#[test]
fn removing_a_project_takes_only_the_nested_roots_it_folds() {
    let f = fx();
    let (umb, other) = (f.dir("umbrella"), f.dir("umbrellax"));
    let nested = f.dir("umbrella/umbrella-orders");
    // Inside the parent's folder, but not one of its folded nested roots: it stays.
    let unfolded = f.dir("umbrella/scratch");
    f.write_config(json!({ "workspace": [umb, nested, unfolded, other] }));
    let c = remove_from_workspace(&f.cfg, &f.cg, &umb, &[nested.clone()], None).unwrap();
    assert_eq!(c.workspace, Some(vec![unfolded, other]), "a sibling sharing the name prefix stays");
}

#[test]
fn adding_a_known_root_never_writes_workspace_roots() {
    let f = fx();
    let (a, b) = (f.dir("shop-api"), f.dir("umbrella"));
    f.set_roots(&[&a, &b]);
    f.write_config(json!({ "workspace": [a] }));
    let before = f.roots_bytes().unwrap();
    let mtime = fs::metadata(f.cg.join("workspace-roots.json")).unwrap().modified().unwrap();
    let c = add_to_workspace(&f.cfg, &f.cg, &b, Some(&f.cg)).unwrap();
    assert_eq!(c.workspace, Some(vec![a, b.clone()]));
    assert_eq!(f.roots_bytes().unwrap(), before);
    assert_eq!(fs::metadata(f.cg.join("workspace-roots.json")).unwrap().modified().unwrap(), mtime);
    assert!(f.config().get("projects").is_none(), "no ownership taken");
    assert!(!f.cg.join("excludes.json").exists());
}

#[test]
fn add_project_also_adds_the_root_to_the_workspace() {
    let f = fx();
    let vs = f.dir("vs");
    f.set_roots(&[&vs]);
    f.write_config(json!({ "workspace": [] }));
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    assert_eq!(f.config()["workspace"], json!([app]));
    // An existing root (`already`) is added too.
    let r = add_project_with(&f.cg, &f.cfg, &vs).unwrap();
    assert!(r.already);
    assert_eq!(f.config()["workspace"], json!([app, vs]));
}

#[test]
fn remove_from_workspace_alone_never_touches_capture() {
    let f = fx();
    let app = f.dir("app");
    f.write_config(json!({ "workspace": [] }));
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let roots = f.roots_bytes();
    let r = remove_from_workspace_with(&f.cg, &f.cfg, &app, &[], false, &[]).unwrap();
    assert!(r.removed.is_none());
    assert_eq!(r.config.workspace, Some(vec![]));
    assert_eq!(f.roots_bytes(), roots);
    assert_eq!(f.config()["projects"][&app]["addedByApp"], json!(true));
}

#[test]
fn remove_with_stop_capture_runs_the_remove_flow() {
    let f = fx();
    let (app, keep) = (f.dir("app"), f.dir("keep"));
    f.write_config(json!({ "workspace": [keep] }));
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let r = remove_from_workspace_with(&f.cg, &f.cfg, &app, &[], true, &[]).unwrap();
    assert!(r.removed.unwrap().unregistered);
    assert_eq!(r.config.workspace, Some(vec![keep]));
    assert_eq!(f.roots_bytes().map(|b| serde_json::from_slice::<serde_json::Value>(&b).unwrap()), Some(json!([])));
    assert!(f.config()["projects"].get(&app).is_none());
}

#[test]
fn stop_capture_keeps_the_evidence_rules_forget_only_when_vs_code_uses_the_root() {
    let f = fx();
    let app = f.dir("app");
    f.write_config(json!({ "workspace": [] }));
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let storage = f.ws.join("storage.json");
    fs::write(&storage, json!({ "windowsState": { "lastActiveWindow": { "folder": format!("file://{app}") } } }).to_string()).unwrap();
    let r = remove_from_workspace_with(&f.cg, &f.cfg, &app, &[], true, &[storage]).unwrap();
    assert!(!r.removed.unwrap().unregistered, "capture stays on");
    assert_eq!(f.roots_bytes().map(|b| serde_json::from_slice::<serde_json::Value>(&b).unwrap()), Some(json!([app])));
    assert_eq!(r.config.workspace, Some(vec![]));
}

#[test]
fn stop_capture_that_fails_leaves_the_workspace_alone() {
    let f = fx();
    let (app, vs) = (f.dir("app"), f.dir("vs"));
    f.write_config(json!({ "workspace": [] }));
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    add_to_workspace(&f.cfg, &f.cg, &vs, None).unwrap();
    // A root the app did not add is refused by the remove flow.
    assert!(remove_from_workspace_with(&f.cg, &f.cfg, &vs, &[], true, &[]).is_err());
    save_read_only(&f.cfg, true, None).unwrap();
    let e = remove_from_workspace_with(&f.cg, &f.cfg, &app, &[], true, &[]).unwrap_err();
    assert!(e.contains("Read-only mode is on"));
    assert_eq!(f.config()["workspace"], json!([app, vs]));
    // Read-only still lets the view change: the workspace is the app's own.
    let r = remove_from_workspace_with(&f.cg, &f.cfg, &app, &[], false, &[]).unwrap();
    assert_eq!(r.config.workspace, Some(vec![vs]));
}
