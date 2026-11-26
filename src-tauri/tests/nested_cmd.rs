//! A nested registered root through the app's commands: listed inside its parent, its session
//! file mapped to the parent for the watcher, filtered by its own effective exclude. Temp dirs only.

use claudegate_core::paths::session_file_for;
use claudegate_desktop_lib::commands::{get_project_with, list_projects_opts, list_projects_with, LoadOpts};
use claudegate_desktop_lib::watch::{affected, KnownMap};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

struct Fx {
    _tmp: tempfile::TempDir,
    home: PathBuf,
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
    let parent = base.join("umbrella");
    let nested = parent.join("umbrella-auth");
    fs::create_dir_all(home.join("sessions")).unwrap();
    fs::create_dir_all(&nested).unwrap();
    fs::write(home.join("workspace-roots.json"), json!([s(&parent), s(&nested)]).to_string()).unwrap();
    for (root, rel) in [(&parent, "a.txt"), (&nested, "gen/b.txt"), (&nested, "c.txt")] {
        let f = root.join(rel);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, "new").unwrap();
    }
    let session = |files: serde_json::Value| {
        json!({ "sessionId": "x", "status": "active", "files": files, "accepted": [], "rejected": {} }).to_string()
    };
    let e = json!({ "originalContent": "old", "reviewStatus": "pending", "capturedAt": "2026-01-01T00:00:00Z" });
    fs::write(session_file_for(&home, &parent), session(json!({ s(&parent.join("a.txt")): e }))).unwrap();
    fs::write(
        session_file_for(&home, &nested),
        session(json!({ s(&nested.join("gen/b.txt")): e, s(&nested.join("c.txt")): e })),
    )
    .unwrap();
    Fx { _tmp: tmp, home, parent, nested }
}

#[test]
fn the_list_shows_the_parent_only_with_the_nested_count_summed() {
    let f = fx();
    let known: KnownMap = Default::default();
    let list = list_projects_with(&f.home, &known);
    assert_eq!(list.iter().map(|p| p.root.clone()).collect::<Vec<_>>(), vec![s(&f.parent)]);
    assert_eq!(list[0].pending, 3);
    assert_eq!(list[0].nested, vec![s(&f.nested)]);
}

#[test]
fn a_change_of_the_nested_session_refreshes_the_parent() {
    let f = fx();
    let known: KnownMap = Default::default();
    get_project_with(&f.home, &known, &s(&f.parent));
    let k = known.lock().unwrap().clone();
    let a = affected(&[session_file_for(&f.home, &f.nested)], &k);
    assert_eq!(a.roots, vec![s(&f.parent)]);
    assert!(!a.relist);
}

#[test]
fn the_nested_scope_uses_the_apps_exclude_when_the_app_owns_that_root() {
    let f = fx();
    let known: KnownMap = Default::default();
    let nested = f.nested.clone();
    // The app owns the nested root with `gen` excluded; the parent's exclude says nothing.
    let opts = |r: &Path| LoadOpts {
        max_attached: 256,
        exclude: (r == nested.as_path()).then(|| vec!["gen".to_string()]),
        extra_hide: vec![],
    };
    let list = list_projects_opts(&f.home, &known, &opts);
    assert_eq!(list[0].pending, 2);
}

#[test]
fn the_summary_counts_the_nested_roots_ready_rows_toward_the_parent() {
    let f = fx();
    let known: KnownMap = Default::default();
    let list = list_projects_with(&f.home, &known);
    assert_eq!(list.len(), 1, "the nested root is never a project of its own");
    assert!(list[0].pending > 0);
    assert_eq!(list[0].ready, list[0].pending, "settled captures, none waiting");
}

#[test]
fn the_decide_command_routes_scoped_keys_to_that_scope_only() {
    use claudegate_core::decide::DecidePaths;
    use claudegate_desktop_lib::commands::{run_decide, TargetDto};
    let f = fx();
    let p = s(&f.nested.join("x.go"));
    let rec = json!({ "id": "r", "path": &p, "before": "a", "after": "b", "decidedAt": "2026-01-01T00:00:00Z" });
    for root in [&f.parent, &f.nested] {
        let v = json!({ "sessionId": "x", "status": "reviewed", "files": {}, "accepted": [], "rejected": { &p: rec } });
        fs::write(session_file_for(&f.home, root), v.to_string()).unwrap();
    }
    let target: TargetDto =
        serde_json::from_value(json!({ "kind": "all", "keys": [&p], "scopedKeys": [{ "scope": s(&f.nested), "key": &p }] })).unwrap();
    let desktop = f.home.parent().unwrap().join("cgd");
    let paths = DecidePaths { home: f.home.clone(), desktop_home: desktop.clone() };
    let lock = std::sync::Mutex::new(());
    let out = run_decide(&lock, &paths, &desktop.join("config.json"), &s(&f.parent), "clearRejected", &target, "2026-11-25T00:00:00.000Z");
    assert_eq!(out.result.unwrap().done, 1);
    let read = |r: &Path| -> serde_json::Value { serde_json::from_str(&fs::read_to_string(session_file_for(&f.home, r)).unwrap()).unwrap() };
    assert!(read(&f.nested)["rejected"].get(&p).is_none());
    assert!(read(&f.parent)["rejected"].get(&p).is_some());
}

fn ext_sources_in(base: &Path) -> claudegate_core::settings::ExtSources {
    claudegate_core::settings::ExtSources {
        vscode_user: Some(base.join("Code/User/settings.json")),
        cursor_user: Some(base.join("Cursor/User/settings.json")),
        excludes_json: base.join("cg/excludes.json"),
        hooklog_sentinel: base.join("cg/hooklog.enabled"),
    }
}

#[test]
fn the_views_counts_leave_out_rows_the_hide_patterns_hide() {
    use claudegate_desktop_lib::commands::get_project_for;
    let f = fx();
    let base = f.home.parent().unwrap().to_path_buf();
    let cfg = base.join("cgd/config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, json!({ "editor": "zed {path}:{line}:{col}", "projects": { s(&f.parent): { "extraHide": ["a.txt"] } } }).to_string()).unwrap();
    let known: KnownMap = Default::default();
    let view = get_project_for(&f.home, &known, &cfg, &s(&f.parent), &ext_sources_in(&base));
    assert_eq!(view.pending.len(), 3);
    assert_eq!((view.counted_pending, view.counted_ready), (2, 2), "the parent's a.txt is hidden by its pattern");
}

#[test]
fn each_root_hides_its_own_rows_with_its_own_patterns() {
    let f = fx();
    let known: KnownMap = Default::default();
    let (parent, nested) = (f.parent.clone(), f.nested.clone());
    let count = |parent_hide: &[&str], nested_hide: &[&str]| {
        let opts = |r: &Path| LoadOpts {
            max_attached: 256,
            exclude: None,
            extra_hide: if r == parent.as_path() {
                parent_hide.iter().map(|x| x.to_string()).collect()
            } else if r == nested.as_path() {
                nested_hide.iter().map(|x| x.to_string()).collect()
            } else {
                vec![]
            },
        };
        list_projects_opts(&f.home, &known, &opts)[0].pending
    };
    // The parent's pattern never reaches the nested root's rows (a.txt, gen/b.txt, c.txt)...
    assert_eq!(count(&["c.txt"], &[]), 3);
    // ...and the nested root's own pattern hides its rows.
    assert_eq!(count(&[], &["gen"]), 2);
}

#[test]
fn the_parents_settings_list_the_nested_roots_hide_patterns() {
    use claudegate_desktop_lib::commands::settings_view;
    let f = fx();
    let base = f.home.parent().unwrap().to_path_buf();
    let cfg = base.join("cgd/config.json");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, json!({ "editor": "zed {path}:{line}:{col}", "projects": { s(&f.nested): { "extraHide": ["gen"] } } }).to_string()).unwrap();
    let v = settings_view(&cfg, Some(&s(&f.parent)), &ext_sources_in(&base));
    let got: Vec<(String, Vec<String>)> = v.nested_extra_hide.iter().map(|n| (n.root.clone(), n.patterns.clone())).collect();
    assert_eq!(got, vec![(s(&f.nested), vec!["gen".to_string()])]);
    assert!(v.extra_hide.is_empty());
}
