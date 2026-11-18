//! The app owns `excludes.json[root]` for the projects it added, and only while VS Code has not
//! taken that key over. Temp homes only; the hook test runs the extension's real hook.py.

use claudegate_core::projects::{
    add_project, owned_exclude, remove_project, sync_excludes, validate_globs, vscode_uses_root, AppProjects,
    ProjectRecord, RootChange,
};
use claudegate_core::settings::DEFAULT_EXCLUDES;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The app's config in memory: `projects[root]`.
#[derive(Default)]
struct Cfg {
    projects: BTreeMap<String, ProjectRecord>,
}

impl AppProjects for Cfg {
    fn app_roots(&self) -> Vec<String> {
        self.projects.iter().filter(|(_, p)| p.added_by_app).map(|(r, _)| r.clone()).collect()
    }
    fn mark_added(&mut self, root: &str) -> Result<(), String> {
        self.projects.entry(root.into()).or_default().added_by_app = true;
        Ok(())
    }
    fn forget(&mut self, root: &str) -> Result<(), String> {
        self.projects.remove(root);
        Ok(())
    }
    fn record(&self, root: &str) -> ProjectRecord {
        self.projects.get(root).cloned().unwrap_or_default()
    }
    fn set_exclude_written(&mut self, root: &str, globs: &[String]) -> Result<(), String> {
        self.projects.entry(root.into()).or_default().exclude_written = Some(globs.to_vec());
        Ok(())
    }
    fn set_managed_by_vscode(&mut self, root: &str) -> Result<(), String> {
        self.projects.entry(root.into()).or_default().managed_by_vscode = true;
        Ok(())
    }
}

struct Env {
    _t: tempfile::TempDir,
    /// The sandbox `$HOME`.
    user: PathBuf,
    /// `<user>/.claudegate`, the ClaudeGate home hook.py reads.
    home: PathBuf,
    ws: PathBuf,
}

fn env() -> Env {
    let t = tempfile::tempdir().unwrap();
    let user = t.path().join("user");
    let home = user.join(".claudegate");
    let ws = t.path().join("ws");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&ws).unwrap();
    Env { user, home, ws, _t: t }
}

impl Env {
    fn dir(&self, name: &str) -> String {
        let p = self.ws.join(name);
        fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    }
    fn excludes_path(&self) -> PathBuf {
        self.home.join("excludes.json")
    }
    fn raw(&self) -> String {
        fs::read_to_string(self.excludes_path()).unwrap()
    }
    fn excludes(&self) -> serde_json::Value {
        serde_json::from_str(&self.raw()).unwrap()
    }
    fn set_excludes(&self, v: serde_json::Value) {
        fs::write(self.excludes_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }
}

fn defaults() -> Vec<String> {
    DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect()
}

#[test]
fn add_writes_the_default_exclude_for_its_own_key_only() {
    let e = env();
    let vs = e.dir("vs");
    // Another window's key, formatted its own way, plus a root that no longer exists.
    e.set_excludes(json!({ vs.clone(): ["**/b"], "/gone/x": ["**/g", 3] }));
    let app = e.dir("app");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    let v = e.excludes();
    assert_eq!(v[&vs], json!(["**/b"]));
    assert_eq!(v["/gone/x"], json!(["**/g", 3]), "never pruned by the app");
    assert_eq!(v[&app], json!(defaults()));
    assert_eq!(cfg.record(&app).exclude_written, Some(defaults()), "recorded exactly as written");
    assert!(!cfg.record(&app).managed_by_vscode);
}

#[test]
fn adding_a_vs_code_root_writes_no_exclude() {
    let e = env();
    let vs = e.dir("vs");
    fs::write(e.home.join("workspace-roots.json"), json!([vs]).to_string()).unwrap();
    let mut cfg = Cfg::default();
    let added = add_project(&e.home, &mut cfg, &vs).unwrap();
    assert!(added.already);
    assert!(!e.excludes_path().exists());
    assert!(sync_excludes(&e.home, &mut cfg).unwrap().is_empty());
    assert!(!e.excludes_path().exists());
}

#[test]
fn a_key_vs_code_already_wrote_makes_the_new_root_managed() {
    let e = env();
    let app = e.dir("app");
    // A VS Code window has this folder open (its exclude is there) but the roots file lost it.
    e.set_excludes(json!({ app.clone(): ["**/vscode-only"] }));
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    assert_eq!(e.excludes()[&app], json!(["**/vscode-only"]));
    assert!(cfg.record(&app).managed_by_vscode);
}

#[test]
fn sync_is_quiet_when_nothing_changed() {
    let e = env();
    let app = e.dir("app");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    let before = e.raw();
    let mtime = fs::metadata(e.excludes_path()).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    // Its own write's watch event must not loop.
    assert!(sync_excludes(&e.home, &mut cfg).unwrap().is_empty());
    assert!(sync_excludes(&e.home, &mut cfg).unwrap().is_empty());
    assert_eq!(e.raw(), before);
    assert_eq!(fs::metadata(e.excludes_path()).unwrap().modified().unwrap(), mtime);
}

#[test]
fn sync_writes_the_apps_own_list_and_restores_a_dropped_key() {
    let e = env();
    let app = e.dir("app");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    // The user edits the project's exclude in the app.
    cfg.projects.get_mut(&app).unwrap().exclude = Some(vec!["**/gen/**".into()]);
    let ch = sync_excludes(&e.home, &mut cfg).unwrap();
    assert_eq!(ch, vec![RootChange::Written { root: app.clone(), globs: vec!["**/gen/**".into()] }]);
    assert_eq!(e.excludes()[&app], json!(["**/gen/**"]));
    assert_eq!(cfg.record(&app).exclude_written, Some(vec!["**/gen/**".to_string()]));
    // The extension prunes or drops the key (its folder looked gone): it is written again.
    e.set_excludes(json!({}));
    sync_excludes(&e.home, &mut cfg).unwrap();
    assert_eq!(e.excludes()[&app], json!(["**/gen/**"]));
}

#[test]
fn a_takeover_is_detected_and_the_key_is_never_written_again() {
    let e = env();
    let app = e.dir("app");
    let other = e.dir("other");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    // A VS Code window opens the folder and persists its own exclude.
    e.set_excludes(json!({ app.clone(): ["**/from-vscode"], other.clone(): ["**/o"] }));
    let ch = sync_excludes(&e.home, &mut cfg).unwrap();
    assert_eq!(ch, vec![RootChange::ManagedByVsCode { root: app.clone() }]);
    assert!(cfg.record(&app).managed_by_vscode);
    assert_eq!(e.excludes()[&app], json!(["**/from-vscode"]));

    // From now on the app leaves the key alone, whatever happens to it.
    cfg.projects.get_mut(&app).unwrap().exclude = Some(vec!["**/mine".into()]);
    let raw = e.raw();
    assert!(sync_excludes(&e.home, &mut cfg).unwrap().is_empty());
    assert_eq!(e.raw(), raw);
    e.set_excludes(json!({ other.clone(): ["**/o"] }));
    assert!(sync_excludes(&e.home, &mut cfg).unwrap().is_empty());
    assert_eq!(e.excludes(), json!({ other: ["**/o"] }));
}

#[test]
fn a_value_equal_to_what_the_app_wants_is_adopted_not_taken_over() {
    let e = env();
    let app = e.dir("app");
    let mut cfg = Cfg::default();
    cfg.mark_added(&app).unwrap();
    // The write landed but recording it did not (a crash in between).
    e.set_excludes(json!({ app.clone(): defaults() }));
    assert!(sync_excludes(&e.home, &mut cfg).unwrap().is_empty());
    assert!(!cfg.record(&app).managed_by_vscode);
    assert_eq!(cfg.record(&app).exclude_written, Some(defaults()));
}

#[test]
fn removing_a_project_deletes_only_its_own_key() {
    let e = env();
    let app = e.dir("app");
    let vs = e.dir("vs");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    let mut ex = e.excludes();
    ex[&vs] = json!(["**/v"]);
    e.set_excludes(ex);
    let r = remove_project(&e.home, &mut cfg, &app, &[]).unwrap();
    assert!(r.unregistered, "a key equal to what the app wrote is not VS Code evidence");
    assert_eq!(e.excludes(), json!({ vs: ["**/v"] }));
}

#[test]
fn a_forget_only_remove_never_touches_excludes() {
    let e = env();
    let app = e.dir("app");
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    // VS Code took the key over before the takeover was synced.
    e.set_excludes(json!({ app.clone(): ["**/from-vscode"] }));
    let raw = e.raw();
    let r = remove_project(&e.home, &mut cfg, &app, &[]).unwrap();
    assert!(!r.unregistered);
    assert_eq!(e.raw(), raw);

    // A takeover already recorded is evidence too, even once the key is gone again.
    let b = e.dir("b");
    add_project(&e.home, &mut cfg, &b).unwrap();
    cfg.set_managed_by_vscode(&b).unwrap();
    let raw = e.raw();
    let r = remove_project(&e.home, &mut cfg, &b, &[]).unwrap();
    assert!(!r.unregistered);
    assert_eq!(e.raw(), raw);
}

#[test]
fn only_a_key_that_differs_from_what_the_app_wrote_is_evidence() {
    let e = env();
    let app = e.dir("app");
    let written = vec!["**/x".to_string()];
    e.set_excludes(json!({ app.clone(): ["**/x"] }));
    assert!(!vscode_uses_root(&e.home, Path::new(&app), Some(&written), &[]));
    assert!(vscode_uses_root(&e.home, Path::new(&app), None, &[]), "never written: VS Code's");
    e.set_excludes(json!({ app.clone(): ["**/y"] }));
    assert!(vscode_uses_root(&e.home, Path::new(&app), Some(&written), &[]));
}

#[test]
fn owned_exclude_is_the_apps_list_else_the_defaults() {
    assert_eq!(owned_exclude(&ProjectRecord::default()), defaults());
    let r = ProjectRecord { exclude: Some(vec!["**/a".into()]), ..Default::default() };
    assert_eq!(owned_exclude(&r), vec!["**/a".to_string()]);
    // An empty list is a choice too: nothing excluded.
    let r = ProjectRecord { exclude: Some(vec![]), ..Default::default() };
    assert!(owned_exclude(&r).is_empty());
}

#[test]
fn globs_are_validated() {
    assert_eq!(validate_globs(&[" **/a ".into(), "**/a".into(), "b/*.txt".into()]).unwrap(), vec!["**/a", "b/*.txt"]);
    assert!(validate_globs(&["".into()]).is_err());
    assert!(validate_globs(&["   ".into()]).is_err());
    assert!(validate_globs(&[]).unwrap().is_empty());
}

/// The extension's hook.py, from the pinned checkout `xcheck/fetch-extension.sh` makes.
fn hook_py() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../.xcheck-cache/claudegate/hooks/hook.py")
}

fn python3() -> bool {
    std::process::Command::new("python3").arg("--version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn the_hooks_real_matcher_honours_what_the_app_wrote() {
    if !python3() {
        eprintln!("skipped: python3 is not installed");
        return;
    }
    let hook = hook_py();
    assert!(hook.exists(), "run ./xcheck/fetch-extension.sh first ({} missing)", hook.display());
    let e = env();
    let app = e.dir("app");
    fs::create_dir_all(Path::new(&app).join("node_modules/pkg")).unwrap();
    fs::write(Path::new(&app).join("node_modules/pkg/index.js"), "x").unwrap();
    fs::write(Path::new(&app).join("main.rs"), "fn main() {}").unwrap();
    let mut cfg = Cfg::default();
    add_project(&e.home, &mut cfg, &app).unwrap();
    assert!(e.excludes()[&app].as_array().unwrap().contains(&json!("**/node_modules/**")));
    fs::write(e.home.join("hooklog.enabled"), "").unwrap();

    let run = |file: &str| {
        use std::io::Write;
        let payload = json!({
            "session_id": "s1",
            "cwd": app,
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "tool_input": { "file_path": file },
        });
        let mut child = std::process::Command::new("python3")
            .arg("-I")
            .arg(&hook)
            .env_clear()
            .env("HOME", &e.user)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
    };
    let excluded = format!("{app}/node_modules/pkg/index.js");
    run(&excluded);
    let log = fs::read_to_string(e.home.join("hook.log")).unwrap();
    assert!(log.contains("skip-excluded") && log.contains(&excluded), "{log}");
    let session = claudegate_core::paths::session_file_for(&e.home, Path::new(&app));
    assert!(!session.exists(), "no pending entry for an excluded file");

    // Control: a file the exclude does not match is captured by the same hook.
    let kept = format!("{app}/main.rs");
    run(&kept);
    let s: serde_json::Value = serde_json::from_str(&fs::read_to_string(&session).unwrap()).unwrap();
    assert!(s["files"].get(&kept).is_some(), "{s}");
    assert!(s["files"].get(&excluded).is_none());
}
