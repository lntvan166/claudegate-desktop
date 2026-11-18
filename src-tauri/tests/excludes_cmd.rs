//! Exclude ownership through the commands: `excludes.json` in a temp ClaudeGate home,
//! `projects[root]` in a temp config.json. The real ~/.claudegate and ~/.claudegate-desktop are
//! never touched.
use claudegate_core::paths::session_file_for;
use claudegate_core::projects::RootChange;
use claudegate_core::settings::{ExtSources, Source, DEFAULT_EXCLUDES};
use claudegate_desktop_lib::commands::{
    add_project_with, get_project_for, list_projects_for, remove_project_with, set_project_exclude_with,
    settings_view, sync_excludes_with, take_back_exclude_with, ExcludeOwner,
};
use claudegate_desktop_lib::config::{config_path_in, save_extra_hide, save_read_only};
use claudegate_desktop_lib::watch::KnownMap;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

struct Fx {
    _t: tempfile::TempDir,
    cg: PathBuf,
    cfg: PathBuf,
    ws: PathBuf,
    src: ExtSources,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let cg = t.path().join("cg");
    let ws = t.path().join("ws");
    fs::create_dir_all(&cg).unwrap();
    fs::create_dir_all(&ws).unwrap();
    let cfg = config_path_in(&t.path().join("home"));
    let src = ExtSources {
        vscode_user: Some(t.path().join("Code/User/settings.json")),
        cursor_user: Some(t.path().join("Cursor/User/settings.json")),
        excludes_json: cg.join("excludes.json"),
        hooklog_sentinel: cg.join("hooklog.enabled"),
    };
    Fx { cg, cfg, ws, src, _t: t }
}

impl Fx {
    fn dir(&self, n: &str) -> String {
        let p = self.ws.join(n);
        fs::create_dir_all(&p).unwrap();
        fs::canonicalize(p).unwrap().to_string_lossy().into_owned()
    }
    fn excludes(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.cg.join("excludes.json")).unwrap()).unwrap()
    }
    fn config(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(&self.cfg).unwrap()).unwrap()
    }
}

fn defaults() -> Vec<String> {
    DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect()
}

#[test]
fn add_writes_the_default_and_records_it() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    assert_eq!(f.excludes()[&app], json!(defaults()));
    assert_eq!(f.config()["projects"][&app]["excludeWritten"], json!(defaults()));
    let s = settings_view(&f.cfg, Some(&app), &f.src);
    assert_eq!(s.exclude_owner, ExcludeOwner::Owned);
    assert_eq!(s.effective.exclude.value, defaults());
    assert_eq!(s.effective.exclude.source, Source::Default);
}

#[test]
fn set_project_exclude_saves_syncs_and_resets() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let s = set_project_exclude_with(&f.cg, &f.cfg, &app, Some(vec![" **/gen/** ".into(), "**/gen/**".into()]), &f.src)
        .unwrap();
    assert_eq!(s.effective.exclude.value, vec!["**/gen/**".to_string()]);
    assert_eq!(s.effective.exclude.source, Source::App);
    assert_eq!(f.excludes()[&app], json!(["**/gen/**"]));
    let c = f.config();
    assert_eq!(c["projects"][&app]["exclude"], json!(["**/gen/**"]));
    assert_eq!(c["projects"][&app]["excludeWritten"], json!(["**/gen/**"]));

    let s = set_project_exclude_with(&f.cg, &f.cfg, &app, None, &f.src).unwrap();
    assert_eq!(s.effective.exclude.value, defaults());
    assert_eq!(s.effective.exclude.source, Source::Default);
    assert_eq!(f.excludes()[&app], json!(defaults()));
    assert!(f.config()["projects"][&app].get("exclude").is_none());
}

#[test]
fn set_project_exclude_is_refused_where_the_app_does_not_own_the_root() {
    let f = fx();
    let vs = f.dir("vs");
    fs::write(f.cg.join("workspace-roots.json"), json!([vs]).to_string()).unwrap();
    let e = set_project_exclude_with(&f.cg, &f.cfg, &vs, Some(vec!["**/x".into()]), &f.src).unwrap_err();
    assert!(e.contains("VS Code"), "{e}");
    assert!(!f.cg.join("excludes.json").exists());

    // An app root VS Code took over.
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let mut ex = f.excludes();
    ex[&app] = json!(["**/from-vscode"]);
    fs::write(f.cg.join("excludes.json"), ex.to_string()).unwrap();
    let ch = sync_excludes_with(&f.cg, &f.cfg);
    assert_eq!(ch, vec![RootChange::ManagedByVsCode { root: app.clone() }]);
    let s = settings_view(&f.cfg, Some(&app), &f.src);
    assert_eq!(s.exclude_owner, ExcludeOwner::ManagedByVsCode);
    assert_eq!(s.effective.exclude.value, vec!["**/from-vscode".to_string()]);
    let e = set_project_exclude_with(&f.cg, &f.cfg, &app, Some(vec!["**/x".into()]), &f.src).unwrap_err();
    assert!(e.contains("Managed by VS Code"), "{e}");
    assert_eq!(f.excludes()[&app], json!(["**/from-vscode"]));
    // The settings view of a plain VS Code root says so too.
    assert_eq!(settings_view(&f.cfg, Some(&vs), &f.src).exclude_owner, ExcludeOwner::VsCode);
}

#[test]
fn bad_globs_and_read_only_are_refused_without_writing() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let before = (fs::read_to_string(&f.cfg).unwrap(), fs::read_to_string(f.cg.join("excludes.json")).unwrap());
    assert!(set_project_exclude_with(&f.cg, &f.cfg, &app, Some(vec!["  ".into()]), &f.src).is_err());
    save_read_only(&f.cfg, true, None).unwrap();
    let ro = fs::read_to_string(&f.cfg).unwrap();
    let e = set_project_exclude_with(&f.cg, &f.cfg, &app, Some(vec!["**/x".into()]), &f.src).unwrap_err();
    assert!(e.contains("Read-only"), "{e}");
    assert_eq!(fs::read_to_string(&f.cfg).unwrap(), ro);
    assert_eq!(fs::read_to_string(f.cg.join("excludes.json")).unwrap(), before.1);
}

#[test]
fn remove_takes_the_key_out_with_the_root() {
    let f = fx();
    let app = f.dir("app");
    let vs = f.dir("vs");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    let mut ex = f.excludes();
    ex[&vs] = json!(["**/v"]);
    fs::write(f.cg.join("excludes.json"), ex.to_string()).unwrap();
    let r = remove_project_with(&f.cg, &f.cfg, &app, &[]).unwrap();
    assert!(r.unregistered);
    assert_eq!(f.excludes(), json!({ vs: ["**/v"] }));
}

fn pending(f: &Fx, root: &str, rels: &[&str]) {
    let mut files = serde_json::Map::new();
    for r in rels {
        let p = Path::new(root).join(r);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "new\n").unwrap();
        files.insert(
            p.to_string_lossy().into_owned(),
            json!({ "originalContent": "old\n", "reviewStatus": "pending", "sessionId": "s", "capturedAt": "2026-01-01T00:00:00Z" }),
        );
    }
    let file = session_file_for(&f.cg, Path::new(root));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, json!({ "sessionId": "x", "status": "active", "files": files, "accepted": [], "rejected": {} }).to_string())
        .unwrap();
}

#[test]
fn the_view_filters_with_the_owned_exclude_and_counts_skip_hidden_rows() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    pending(&f, &app, &["src/a.rs", "gen/b.rs", "snap/c.snap"]);
    // The app's list is saved but excludes.json still holds the defaults (no sync yet).
    claudegate_desktop_lib::config::save_project_exclude(&f.cfg, &app, Some(&["gen".to_string()]), None).unwrap();
    save_extra_hide(&f.cfg, &app, &["**/*.snap".to_string()], None).unwrap();
    let known = KnownMap::default();
    let v = get_project_for(&f.cg, &known, &f.cfg, &app, &f.src);
    let rels: Vec<&str> = v.pending.iter().map(|p| p.rel.as_str()).collect();
    assert_eq!(rels, vec!["snap/c.snap", "src/a.rs"], "gen excluded; the hidden row is the UI's to hide");
    let list = list_projects_for(&f.cg, &known, &f.cfg, &f.src);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pending, 1, "the sidebar count leaves out the extra-hidden row too");
}

#[test]
fn take_back_returns_ownership_and_read_only_blocks_it() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    set_project_exclude_with(&f.cg, &f.cfg, &app, Some(vec!["**/mine".into()]), &f.src).unwrap();
    let c = f.config();
    assert_eq!(c["projects"][&app]["excludeWritten"], json!(["**/mine"]));
    assert_eq!(c["projects"][&app]["excludeWrittenPrev"], json!(defaults()));
    let mut ex = f.excludes();
    ex[&app] = json!(["**/from-vscode"]);
    fs::write(f.cg.join("excludes.json"), ex.to_string()).unwrap();
    sync_excludes_with(&f.cg, &f.cfg);
    assert_eq!(settings_view(&f.cfg, Some(&app), &f.src).exclude_owner, ExcludeOwner::ManagedByVsCode);

    save_read_only(&f.cfg, true, None).unwrap();
    let e = take_back_exclude_with(&f.cg, &f.cfg, &app, &f.src).unwrap_err();
    assert!(e.contains("Read-only"), "{e}");
    assert_eq!(f.excludes()[&app], json!(["**/from-vscode"]));
    save_read_only(&f.cfg, false, None).unwrap();

    let s = take_back_exclude_with(&f.cg, &f.cfg, &app, &f.src).unwrap();
    assert_eq!(s.exclude_owner, ExcludeOwner::Owned);
    assert_eq!(s.effective.exclude.value, vec!["**/mine".to_string()]);
    assert_eq!(f.excludes()[&app], json!(["**/mine"]));
    assert!(f.config()["projects"][&app].get("managedByVsCode").is_none());
    assert!(sync_excludes_with(&f.cg, &f.cfg).is_empty());

    // A VS Code project cannot be taken.
    let vs = f.dir("vs");
    assert!(take_back_exclude_with(&f.cg, &f.cfg, &vs, &f.src).is_err());
}

/// I2: `excludeWrittenAt` marks when `excludeWritten` last changed. Rewriting the same value after
/// a stale write-back must not restart the window in which the previous value still counts as
/// the app's, or a real takeover by a default-configured VS Code would be masked indefinitely.
#[test]
fn rewriting_the_same_value_does_not_restart_the_stale_write_back_window() {
    use claudegate_core::projects::sync_excludes_at;
    use claudegate_desktop_lib::config::{mark_added_by_app, save_project_exclude, ConfigProjects};
    let f = fx();
    let app = f.dir("app");
    let t: i64 = 1_800_000_000_000;
    let mut cfg = ConfigProjects { path: &f.cfg, guard: Some(&f.cg) };
    mark_added_by_app(&f.cfg, &app, None).unwrap();
    sync_excludes_at(&f.cg, &mut cfg, t - 120_000).unwrap();
    assert_eq!(f.excludes()[&app], json!(defaults()));
    // The user sets X at t: X is written, the defaults D become the previous value.
    let x = vec!["**/gen/**".to_string()];
    save_project_exclude(&f.cfg, &app, Some(&x), None).unwrap();
    sync_excludes_at(&f.cg, &mut cfg, t).unwrap();
    assert_eq!(f.excludes()[&app], json!(x));
    let write_back_defaults = || {
        let mut ex = f.excludes();
        ex[&app] = json!(defaults());
        fs::write(f.cg.join("excludes.json"), serde_json::to_string_pretty(&ex).unwrap()).unwrap();
    };
    // t+20 s: the extension's stale write-back of D: still ours, X is rewritten.
    write_back_defaults();
    let ch = sync_excludes_at(&f.cg, &mut cfg, t + 20_000).unwrap();
    assert_eq!(ch, vec![RootChange::Written { root: app.clone(), globs: x.clone() }]);
    assert_eq!(f.config()["projects"][&app]["excludeWrittenAt"], json!(t), "same value: not restamped");
    // t+40 s: D again, now outside the window that started when X was first written: VS Code's.
    write_back_defaults();
    let ch = sync_excludes_at(&f.cg, &mut cfg, t + 40_000).unwrap();
    assert_eq!(ch, vec![RootChange::ManagedByVsCode { root: app.clone() }]);
    assert_eq!(f.excludes()[&app], json!(defaults()), "VS Code's key is left alone");
}

/// M2: a save whose exclude sync then fails is still saved, and says the hook still uses the
/// old list, with the reason.
#[test]
fn a_failed_exclude_sync_is_surfaced_as_a_warning() {
    let f = fx();
    let app = f.dir("app");
    add_project_with(&f.cg, &f.cfg, &app).unwrap();
    // excludes.json becomes something the writer refuses to overwrite.
    fs::write(f.cg.join("excludes.json"), "[ not json").unwrap();
    let s = set_project_exclude_with(&f.cg, &f.cfg, &app, Some(vec!["**/gen/**".into()]), &f.src).unwrap();
    let w = s.warning.expect("a warning");
    assert!(w.starts_with("Saved, but the hook still uses the old list: "), "{w}");
    assert!(w.len() > "Saved, but the hook still uses the old list: ".len(), "names the reason: {w}");
    assert_eq!(f.config()["projects"][&app]["exclude"], json!(["**/gen/**"]), "the list itself is saved");
    assert_eq!(fs::read_to_string(f.cg.join("excludes.json")).unwrap(), "[ not json", "nothing overwritten");
    // A sync that works carries no warning.
    fs::remove_file(f.cg.join("excludes.json")).unwrap();
    let s = set_project_exclude_with(&f.cg, &f.cfg, &app, None, &f.src).unwrap();
    assert_eq!(s.warning, None);
    assert_eq!(f.excludes()[&app], json!(defaults()));
}
