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
fn the_summary_carries_the_newest_capture() {
    let f = fx();
    let known: KnownMap = Default::default();
    let list = list_projects_with(&f.home, &known);
    assert_eq!(list[0].newest_capture_ms, claudegate_core::time::parse_iso_ms("2026-01-01T00:00:00Z"));
}
