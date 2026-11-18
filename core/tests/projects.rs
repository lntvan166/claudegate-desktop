//! Add and remove projects: `workspace-roots.json` is appended to, never rewritten, and only the
//! roots the app added can be removed. Temp homes only.

use claudegate_core::projects::{add_project, ensure_app_roots_present, remove_project, vscode_uses_root, AppProjects};
use claudegate_core::roots::{check_new_root, read_roots, RootsError};
use std::fs;
use std::path::{Path, PathBuf};

/// The app's record of the roots it added (`projects[root].addedByApp` in config.json).
#[derive(Default)]
struct Cfg {
    roots: Vec<String>,
    fail_mark: bool,
}

impl AppProjects for Cfg {
    fn app_roots(&self) -> Vec<String> {
        self.roots.clone()
    }
    fn mark_added(&mut self, root: &str) -> Result<(), String> {
        if self.fail_mark {
            return Err("config unwritable".into());
        }
        if !self.roots.iter().any(|r| r == root) {
            self.roots.push(root.into());
        }
        Ok(())
    }
    fn forget(&mut self, root: &str) -> Result<(), String> {
        self.roots.retain(|r| r != root);
        Ok(())
    }
}

struct Env {
    _t: tempfile::TempDir,
    home: PathBuf,
    ws: PathBuf,
}

fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("cg");
    let ws = t.path().join("ws");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&ws).unwrap();
    Env { home, ws, _t: t }
}

impl Env {
    fn dir(&self, name: &str) -> String {
        let p = self.ws.join(name);
        fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    }
    fn set_roots(&self, raw: &str) {
        fs::write(self.home.join("workspace-roots.json"), raw).unwrap();
    }
    fn raw(&self) -> String {
        fs::read_to_string(self.home.join("workspace-roots.json")).unwrap()
    }
    fn roots(&self) -> Vec<serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(&self.raw()).unwrap().as_array().unwrap().clone()
    }
}

#[test]
fn add_appends_without_reordering_or_dropping_others() {
    let e = env();
    let a = e.dir("a");
    let b = e.dir("b");
    // "/gone/x" no longer exists and 7 is not a string: the extension would drop both; the app
    // keeps every entry it does not own.
    e.set_roots(&serde_json::to_string(&serde_json::json!([b, "/gone/x", 7, a])).unwrap());
    let c = e.dir("c");
    let mut cfg = Cfg::default();
    let added = add_project(&e.home, &mut cfg, &c).unwrap();
    assert_eq!(added.root, c);
    assert!(!added.already);
    assert_eq!(e.roots(), serde_json::json!([b, "/gone/x", 7, a, c]).as_array().unwrap().clone());
    assert_eq!(cfg.roots, vec![c.clone()]);
    // Pretty JSON with a 2-space indent, as the extension writes it.
    assert!(e.raw().starts_with("[\n  \""), "{}", e.raw());
}

#[test]
fn add_to_missing_file_starts_from_empty() {
    let e = env();
    let a = e.dir("a");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &a).unwrap();
    assert_eq!(read_roots(&e.home), vec![PathBuf::from(&a)]);
}

#[test]
fn add_resolves_the_path() {
    let e = env();
    let a = e.dir("a");
    let mut cfg = Cfg::default();
    let added = add_project(&e.home, &mut cfg, &format!("{a}/./x/../")).unwrap();
    assert_eq!(added.root, a);
    assert_eq!(read_roots(&e.home), vec![PathBuf::from(&a)]);
}

#[test]
fn adding_an_existing_root_is_idempotent_and_does_not_take_ownership() {
    let e = env();
    let a = e.dir("a");
    e.set_roots(&serde_json::to_string(&[&a]).unwrap());
    let before = e.raw();
    let mut cfg = Cfg::default();
    let added = add_project(&e.home, &mut cfg, &a).unwrap();
    assert!(added.already);
    assert_eq!(e.raw(), before, "no rewrite, no duplicate");
    assert!(cfg.roots.is_empty(), "a VS Code root never becomes app-added");
}

#[test]
fn nested_and_containing_roots_are_refused_naming_the_root() {
    let e = env();
    let a = e.dir("a");
    e.dir("a/inner");
    e.set_roots(&serde_json::to_string(&[&a]).unwrap());
    let before = e.raw();
    let mut cfg = Cfg::default();
    match add_project(&e.home, &mut cfg, &format!("{a}/inner")) {
        Err(RootsError::Nested { existing, worktree: false }) => assert_eq!(existing, a),
        other => panic!("{other:?}"),
    }
    match add_project(&e.home, &mut cfg, &e.ws.to_string_lossy()) {
        Err(RootsError::Contains { existing }) => assert_eq!(existing, a),
        other => panic!("{other:?}"),
    }
    let msg = RootsError::Contains { existing: a.clone() }.to_string();
    assert!(msg.contains(&a), "{msg}");
    assert_eq!(e.raw(), before);
    assert!(cfg.roots.is_empty());
}

#[test]
fn guard_compares_components_not_strings() {
    let roots = vec![PathBuf::from("/ws/app")];
    // "/ws/app-two" starts with "/ws/app" as a string, not as a path.
    assert!(check_new_root(&roots, Path::new("/ws/app-two")).is_ok());
    assert!(check_new_root(&roots, Path::new("/ws/ap")).is_ok());
    assert!(check_new_root(&roots, Path::new("/ws/app")).is_ok());
    assert!(matches!(check_new_root(&roots, Path::new("/ws/app/x")), Err(RootsError::Nested { .. })));
    assert!(matches!(check_new_root(&roots, Path::new("/ws")), Err(RootsError::Contains { .. })));
    // Existing entries are compared in resolved form.
    let messy = vec![PathBuf::from("/ws/./app/")];
    assert!(matches!(check_new_root(&messy, Path::new("/ws/app/x")), Err(RootsError::Nested { .. })));
}

#[test]
fn worktree_under_a_root_is_refused_with_a_hint() {
    let e = env();
    let a = e.dir("a");
    let wt = e.dir("a/.worktrees/feat");
    fs::write(Path::new(&wt).join(".git"), "gitdir: /x/.git/worktrees/feat\n").unwrap();
    e.set_roots(&serde_json::to_string(&[&a]).unwrap());
    let mut cfg = Cfg::default();
    let err = add_project(&e.home, &mut cfg, &wt).unwrap_err();
    assert!(matches!(&err, RootsError::Nested { existing, worktree: true } if *existing == a), "{err:?}");
    assert!(err.to_string().contains("worktrees inside a project are found automatically"), "{err}");
}

#[test]
fn relative_paths_and_files_are_refused() {
    let e = env();
    let mut cfg = Cfg::default();
    assert!(matches!(add_project(&e.home, &mut cfg, "some/dir"), Err(RootsError::NotAbsolute)));
    let f = e.ws.join("file.txt");
    fs::write(&f, "x").unwrap();
    assert!(matches!(add_project(&e.home, &mut cfg, &f.to_string_lossy()), Err(RootsError::NotADirectory)));
    assert!(matches!(
        add_project(&e.home, &mut cfg, &e.ws.join("nope").to_string_lossy()),
        Err(RootsError::NotADirectory)
    ));
    assert!(!e.home.join("workspace-roots.json").exists());
}

#[test]
fn malformed_roots_file_is_refused_and_left_alone() {
    let e = env();
    let a = e.dir("a");
    for raw in ["{\"x\": 1}", "not json"] {
        e.set_roots(raw);
        let mut cfg = Cfg::default();
        assert!(matches!(add_project(&e.home, &mut cfg, &a), Err(RootsError::Io(_))), "{raw}");
        assert_eq!(e.raw(), raw);
        assert!(cfg.roots.is_empty());
    }
}

#[test]
fn failing_to_record_ownership_takes_the_root_back_out() {
    let e = env();
    let a = e.dir("a");
    let b = e.dir("b");
    e.set_roots(&serde_json::to_string(&[&a]).unwrap());
    let mut cfg = Cfg { fail_mark: true, ..Cfg::default() };
    assert!(matches!(add_project(&e.home, &mut cfg, &b), Err(RootsError::Io(_))));
    assert_eq!(read_roots(&e.home), vec![PathBuf::from(&a)]);
}

#[test]
fn removing_a_vs_code_root_is_refused() {
    let e = env();
    let a = e.dir("a");
    e.set_roots(&serde_json::to_string(&[&a]).unwrap());
    let before = e.raw();
    let mut cfg = Cfg::default();
    assert!(matches!(remove_project(&e.home, &mut cfg, &a, &[]), Err(RootsError::NotAppAdded)));
    assert_eq!(e.raw(), before);
}

#[test]
fn remove_drops_only_the_app_root_and_keeps_the_session() {
    let e = env();
    let a = e.dir("a");
    let b = e.dir("b");
    e.set_roots(&serde_json::to_string(&serde_json::json!([a, "/gone/x"])).unwrap());
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    let session = claudegate_core::paths::session_file_for(&e.home, Path::new(&b));
    fs::create_dir_all(session.parent().unwrap()).unwrap();
    fs::write(&session, "{}").unwrap();
    let r = remove_project(&e.home, &mut cfg, &b, &[]).unwrap();
    assert!(r.unregistered);
    assert_eq!(e.roots(), serde_json::json!([a, "/gone/x"]).as_array().unwrap().clone());
    assert!(cfg.roots.is_empty());
    assert!(session.exists(), "review history is kept");
}

#[test]
fn a_dropped_app_root_is_re_added_exactly_once() {
    let e = env();
    let a = e.dir("a");
    let b = e.dir("b");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    // The extension rewrites the file from a stale read and loses the app's root.
    e.set_roots(&serde_json::to_string(&[&a]).unwrap());
    assert_eq!(ensure_app_roots_present(&e.home, &cfg).unwrap(), vec![b.clone()]);
    assert_eq!(read_roots(&e.home), vec![PathBuf::from(&a), PathBuf::from(&b)]);
    // The watcher sees our own write next: nothing is missing, so nothing is written.
    let mtime = fs::metadata(e.home.join("workspace-roots.json")).unwrap().modified().unwrap();
    let raw = e.raw();
    assert!(ensure_app_roots_present(&e.home, &cfg).unwrap().is_empty());
    assert_eq!(e.raw(), raw);
    assert_eq!(fs::metadata(e.home.join("workspace-roots.json")).unwrap().modified().unwrap(), mtime);
}

#[test]
fn a_deleted_app_folder_is_not_re_added_until_it_returns() {
    let e = env();
    let b = e.dir("b");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    fs::remove_dir_all(&b).unwrap();
    e.set_roots("[]");
    assert!(ensure_app_roots_present(&e.home, &cfg).unwrap().is_empty());
    assert_eq!(e.raw(), "[]");
    fs::create_dir_all(&b).unwrap();
    assert_eq!(ensure_app_roots_present(&e.home, &cfg).unwrap(), vec![b.clone()]);
}

#[test]
fn re_adding_skips_a_root_that_would_now_nest() {
    let e = env();
    let b = e.dir("b");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    // VS Code since registered the parent folder; re-adding b would pull captures out of it.
    e.set_roots(&serde_json::to_string(&[e.ws.to_string_lossy()]).unwrap());
    assert!(ensure_app_roots_present(&e.home, &cfg).unwrap().is_empty());
}

#[test]
fn remove_only_forgets_when_vs_code_has_an_excludes_key_for_the_root() {
    let e = env();
    let b = e.dir("b");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    fs::write(e.home.join("excludes.json"), serde_json::json!({ b.clone(): ["**/x"] }).to_string()).unwrap();
    let before = e.raw();
    let r = remove_project(&e.home, &mut cfg, &b, &[]).unwrap();
    assert!(!r.unregistered);
    assert_eq!(e.raw(), before, "capture stays on");
    assert!(cfg.roots.is_empty(), "now a VS Code project");
}

#[test]
fn remove_only_forgets_when_the_folder_is_open_in_vs_code() {
    let e = env();
    let b = e.dir("my proj");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    let storage = e.ws.join("Code/User/globalStorage/storage.json");
    fs::create_dir_all(storage.parent().unwrap()).unwrap();
    let uri = format!("file://{}", b.replace(' ', "%20"));
    for v in [
        serde_json::json!({ "windowsState": { "lastActiveWindow": { "folder": uri } } }),
        serde_json::json!({ "windowsState": { "openedWindows": [{ "folder": "vscode-remote://x/y" }, { "folder": uri }] } }),
        serde_json::json!({ "backupWorkspaces": { "folders": [{ "folderUri": uri }] } }),
    ] {
        fs::write(&storage, v.to_string()).unwrap();
        assert!(vscode_uses_root(&e.home, Path::new(&b), &[storage.clone()]), "{v}");
    }
    let before = e.raw();
    let r = remove_project(&e.home, &mut cfg, &b, &[e.ws.join("absent.json"), storage.clone()]).unwrap();
    assert!(!r.unregistered);
    assert_eq!(e.raw(), before);
}

#[test]
fn no_evidence_unregisters_and_bad_storage_is_no_evidence() {
    let e = env();
    let b = e.dir("b");
    let other = e.dir("other");
    let storage = e.ws.join("storage.json");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &b).unwrap();
    fs::write(e.home.join("excludes.json"), serde_json::json!({ other.clone(): [] }).to_string()).unwrap();
    fs::write(&storage, "{ not json").unwrap();
    assert!(!vscode_uses_root(&e.home, Path::new(&b), &[storage.clone()]));
    let r = remove_project(&e.home, &mut cfg, &b, &[storage]).unwrap();
    assert!(r.unregistered);
    assert!(read_roots(&e.home).is_empty());
}
