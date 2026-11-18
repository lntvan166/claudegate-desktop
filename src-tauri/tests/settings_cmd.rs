//! Settings commands: the app's overrides in config.json, the extension fallback, the hook-log
//! sentinel and maxAttached. Every file touched lives in a temp dir; the real ~/.claudegate,
//! ~/.claudegate-desktop and the editors' settings are never read or written.
use claudegate_core::settings::{ExtSources, Source};
use claudegate_desktop_lib::commands::{get_project_max, settings_view};
use claudegate_desktop_lib::config::{
    config_path_in, load_or_create, save_extra_hide, save_hidden_projects, save_read_only, save_setting,
};
use claudegate_desktop_lib::watch::KnownMap;
use serde_json::json;
use std::fs;
use std::path::Path;

fn sources(dir: &Path) -> ExtSources {
    ExtSources {
        vscode_user: Some(dir.join("Code/User/settings.json")),
        cursor_user: Some(dir.join("Cursor/User/settings.json")),
        excludes_json: dir.join("cg/excludes.json"),
        hooklog_sentinel: dir.join("cg/hooklog.enabled"),
    }
}

fn read(p: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
fn set_setting_round_trips_and_keeps_everything_else() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(
        &p,
        json!({
            "editor": "code -g {path}:{line}:{col}",
            "customEditor": "hx {path}",
            "readOnly": true,
            "hiddenProjects": ["/a"],
            "somethingNew": {"x": 1},
            "settings": {"futureKey": 7},
        })
        .to_string(),
    )
    .unwrap();

    save_setting(&p, "autoAdvance", json!(false), None).unwrap();
    save_setting(&p, "groupBySession", json!(true), None).unwrap();
    save_setting(&p, "pendingAgeMinDays", json!(3), None).unwrap();
    save_setting(&p, "maxAttached", json!(12), None).unwrap();
    save_setting(&p, "protected", json!(["**/.env", "**/*.key"]), None).unwrap();

    let v = read(&p);
    assert_eq!(v["somethingNew"], json!({"x": 1}));
    assert_eq!(v["settings"]["futureKey"], json!(7));
    assert_eq!(v["settings"]["autoAdvance"], json!(false));
    assert_eq!(v["settings"]["groupBySession"], json!(true));
    assert_eq!(v["settings"]["pendingAge.minDays"], json!(3));
    assert_eq!(v["settings"]["worktrees.maxAttached"], json!(12));
    assert_eq!(v["settings"]["protected"], json!(["**/.env", "**/*.key"]));
    let c = load_or_create(&p);
    assert_eq!(c.editor, "code -g {path}:{line}:{col}");
    assert_eq!(c.custom_editor.as_deref(), Some("hx {path}"));
    assert!(c.read_only);
    assert_eq!(c.hidden_projects, vec!["/a".to_string()]);

    let s = settings_view(&p, None, &sources(home.path()));
    assert_eq!((s.effective.auto_advance.value, s.effective.auto_advance.source), (false, Source::App));
    assert_eq!(s.effective.group_by_session.value, true);
    assert_eq!(s.effective.pending_age_min_days.value, 3);
    assert_eq!(s.effective.max_attached.value, 12);
    assert_eq!(s.effective.protected.value, vec!["**/.env".to_string(), "**/*.key".to_string()]);
    assert_eq!(s.effective.protected.source, Source::App);

    // null drops the override: the value falls back to the default.
    save_setting(&p, "autoAdvance", serde_json::Value::Null, None).unwrap();
    let v = read(&p);
    assert!(v["settings"].get("autoAdvance").is_none());
    assert_eq!(v["settings"]["futureKey"], json!(7));
    let s = settings_view(&p, None, &sources(home.path()));
    assert_eq!((s.effective.auto_advance.value, s.effective.auto_advance.source), (true, Source::Default));

    // The other writers keep the settings map.
    save_read_only(&p, false, None).unwrap();
    save_hidden_projects(&p, &["/b".into()], None).unwrap();
    assert_eq!(read(&p)["settings"]["groupBySession"], json!(true));
}

#[test]
fn wrong_types_and_unknown_keys_are_refused_without_writing() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    save_setting(&p, "autoAdvance", json!(true), None).unwrap();
    let before = fs::read_to_string(&p).unwrap();
    for (k, v) in [
        ("autoAdvance", json!("yes")),
        ("groupBySession", json!(1)),
        ("pendingAgeMinDays", json!(-1)),
        ("pendingAgeMinDays", json!(1.5)),
        ("maxAttached", json!(0)),
        ("maxAttached", json!("256")),
        ("protected", json!("**/.env")),
        ("protected", json!([1, 2])),
        ("nope", json!(true)),
        ("exclude", json!(["x"])),
        ("hookLog", json!(true)),
    ] {
        assert!(save_setting(&p, k, v.clone(), None).is_err(), "{k} = {v} should be refused");
    }
    assert_eq!(fs::read_to_string(&p).unwrap(), before);
}

#[test]
fn refuses_config_inside_claudegate_home() {
    let home = tempfile::tempdir().unwrap();
    let cg = home.path().join(".claudegate");
    let p = cg.join("config.json");
    assert!(save_setting(&p, "autoAdvance", json!(true), Some(&cg)).is_err());
    assert!(save_extra_hide(&p, "/r", &["x".into()], Some(&cg)).is_err());
    assert!(!p.exists());
}

#[test]
fn extension_values_show_their_source_and_override_copies_them() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    let src = sources(home.path());
    fs::create_dir_all(home.path().join("Code/User")).unwrap();
    fs::write(
        src.vscode_user.as_ref().unwrap(),
        "{\n // jsonc\n \"claudegate.autoAdvance\": false, \"claudegate.pendingAge.minDays\": 5,\n}",
    )
    .unwrap();
    let s = settings_view(&p, None, &src);
    assert_eq!((s.effective.auto_advance.value, s.effective.auto_advance.source), (false, Source::VsCode));
    assert_eq!(s.effective.pending_age_min_days.value, 5);
    // Override = copy the effective value into the app.
    save_setting(&p, "pendingAgeMinDays", json!(5), None).unwrap();
    let s = settings_view(&p, None, &src);
    assert_eq!((s.effective.pending_age_min_days.value, s.effective.pending_age_min_days.source), (5, Source::App));
    // The extension's file is untouched.
    assert!(fs::read_to_string(src.vscode_user.as_ref().unwrap()).unwrap().contains("// jsonc"));
}

#[test]
fn extra_hide_is_per_project_and_kept_by_other_writes() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, json!({"editor": "zed {path}:{line}:{col}", "projects": {"/r": {"addedByApp": true}}}).to_string())
        .unwrap();
    save_extra_hide(&p, "/r", &[" dist/** ".into(), "".into(), "dist/**".into(), "*.snap".into()], None).unwrap();
    let v = read(&p);
    assert_eq!(v["projects"]["/r"]["addedByApp"], json!(true));
    assert_eq!(v["projects"]["/r"]["extraHide"], json!(["dist/**", "*.snap"]));
    let s = settings_view(&p, Some("/r"), &sources(home.path()));
    assert_eq!(s.extra_hide, vec!["dist/**".to_string(), "*.snap".to_string()]);
    assert!(settings_view(&p, Some("/other"), &sources(home.path())).extra_hide.is_empty());
    save_setting(&p, "groupBySession", json!(true), None).unwrap();
    assert_eq!(read(&p)["projects"]["/r"]["extraHide"], json!(["dist/**", "*.snap"]));
}

#[test]
fn settings_view_serializes_flat_camel_case() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    let v = serde_json::to_value(settings_view(&p, None, &sources(home.path()))).unwrap();
    assert_eq!(v["autoAdvance"], json!({"value": true, "source": "default"}));
    assert_eq!(v["pendingAgeMinDays"]["value"], json!(0));
    assert_eq!(v["maxAttached"]["value"], json!(256));
    assert_eq!(v["hookLog"]["value"], json!(false));
    assert_eq!(v["extraHide"], json!([]));
    assert!(v["exclude"]["value"].as_array().unwrap().len() > 3);
}

fn wt(dir: &Path, n: usize) {
    for i in 0..n {
        let d = dir.join(format!("w{i}"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(".git"), format!("gitdir: /x/.git/worktrees/w{i}\n")).unwrap();
    }
}

#[test]
fn max_attached_limits_the_worktree_scan() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    wt(ws.path(), 3);
    let known = KnownMap::default();
    let root = ws.path().to_string_lossy().into_owned();
    assert_eq!(get_project_max(home.path(), &known, &root, 256).scopes.len(), 4);
    assert_eq!(get_project_max(home.path(), &known, &root, 1).scopes.len(), 2);
    // Only the scopes it loaded are watched.
    assert_eq!(known.lock().unwrap().len(), 2);
}
