use claudegate_core::jsonc::parse_jsonc;
use claudegate_core::settings::*;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/settings").join(name)
}

struct Env {
    dir: tempfile::TempDir,
}
impl Env {
    fn new() -> Self {
        Env { dir: tempfile::tempdir().unwrap() }
    }
    fn p(&self, n: &str) -> PathBuf {
        self.dir.path().join(n)
    }
    fn sources(&self) -> ExtSources {
        ExtSources {
            vscode_user: None,
            cursor_user: None,
            excludes_json: self.p("excludes.json"),
            hooklog_sentinel: self.p("hooklog.enabled"),
        }
    }
}

fn app_with(k: &str, v: serde_json::Value) -> AppSettings {
    let mut s = HashMap::new();
    s.insert(k.to_string(), v);
    AppSettings { settings: s, projects: HashMap::new() }
}

#[test]
fn defaults_when_nothing_set() {
    let e = Env::new();
    let r = effective(&AppSettings::default(), None, &e.sources());
    assert_eq!(r.auto_advance.value, true);
    assert_eq!(r.auto_advance.source, Source::Default);
    assert_eq!(r.group_by_session.value, false);
    assert_eq!(r.pending_age_min_days.value, 0);
    assert_eq!(r.max_attached.value, 256);
    assert_eq!(r.hook_log.value, false);
    assert_eq!(r.hook_log.source, Source::Default);
    assert_eq!(r.exclude.source, Source::Default);
    assert!(r.exclude.value.contains(&"**/go.sum".to_string()));
    assert_eq!(r.protected.value.len(), 10);
}

#[test]
fn precedence_app_workspace_vscode_cursor_default() {
    let e = Env::new();
    let root = e.p("proj");
    fs::create_dir_all(root.join(".vscode")).unwrap();
    let mut src = e.sources();
    src.vscode_user = Some(fixture("vscode-user.jsonc"));
    src.cursor_user = Some(fixture("cursor-user.jsonc"));

    // vscode beats cursor
    let r = effective(&AppSettings::default(), None, &src);
    assert_eq!((r.auto_advance.value, r.auto_advance.source), (false, Source::VsCode));
    assert_eq!(r.pending_age_min_days.value, 3);
    assert_eq!(r.pending_age_min_days.source, Source::VsCode);
    // only cursor has these
    assert_eq!((r.group_by_session.value, r.group_by_session.source), (true, Source::Cursor));
    assert_eq!((r.max_attached.value, r.max_attached.source), (8, Source::Cursor));

    // workspace beats vscode
    fs::write(root.join(".vscode/settings.json"), r#"{ // c
        "claudegate.autoAdvance": true, }"#).unwrap();
    let r = effective(&AppSettings::default(), Some(&root), &src);
    assert_eq!((r.auto_advance.value, r.auto_advance.source), (true, Source::Workspace));

    // app beats workspace
    let r = effective(&app_with("autoAdvance", json!(false)), Some(&root), &src);
    assert_eq!((r.auto_advance.value, r.auto_advance.source), (false, Source::App));
    let r = effective(&app_with("pendingAge.minDays", json!(7)), Some(&root), &src);
    assert_eq!((r.pending_age_min_days.value, r.pending_age_min_days.source), (7, Source::App));
}

#[test]
fn wrongly_typed_values_fall_through() {
    let e = Env::new();
    let mut src = e.sources();
    src.vscode_user = Some(fixture("vscode-user.jsonc"));
    let r = effective(&app_with("autoAdvance", json!("yes")), None, &src);
    assert_eq!(r.auto_advance.source, Source::VsCode);
    let r = effective(&app_with("worktrees.maxAttached", json!(0)), None, &src);
    assert_eq!(r.max_attached.source, Source::Default);
}

#[test]
fn exclude_order() {
    let e = Env::new();
    let root = e.p("proj");
    fs::create_dir_all(root.join(".vscode")).unwrap();
    let mut src = e.sources();
    src.vscode_user = Some(fixture("vscode-user.jsonc"));

    // settings file map, only true keys
    let r = effective(&AppSettings::default(), Some(&root), &src);
    assert_eq!(r.exclude.source, Source::VsCode);
    assert_eq!(r.exclude.value, vec!["**/*.log".to_string()]);

    // excludes.json[root] beats settings files
    let key = claudegate_core::paths::resolve_root(&root).to_string_lossy().into_owned();
    fs::write(e.p("excludes.json"), json!({ key.clone(): ["**/a", "**/b"], "/other": ["x"] }).to_string()).unwrap();
    let r = effective(&AppSettings::default(), Some(&root), &src);
    assert_eq!((r.exclude.value.clone(), r.exclude.source), (vec!["**/a".to_string(), "**/b".to_string()], Source::VsCode));

    // app project beats excludes.json
    let mut app = AppSettings::default();
    app.projects.insert(key, ProjectSettings { exclude: Some(vec!["**/mine".into()]) });
    let r = effective(&app, Some(&root), &src);
    assert_eq!((r.exclude.value, r.exclude.source), (vec!["**/mine".to_string()], Source::App));
}

#[test]
fn exclude_without_root_ignores_project_sources() {
    let e = Env::new();
    fs::write(e.p("excludes.json"), r#"{"/x":["a"]}"#).unwrap();
    let r = effective(&AppSettings::default(), None, &e.sources());
    assert_eq!(r.exclude.source, Source::Default);
}

#[test]
fn hook_log_reads_sentinel() {
    let e = Env::new();
    fs::write(e.p("hooklog.enabled"), "").unwrap();
    let r = effective(&AppSettings::default(), None, &e.sources());
    assert_eq!(r.hook_log.value, true);
    assert_ne!(r.hook_log.source, Source::Default);
}

#[test]
fn bad_files_fall_back() {
    let e = Env::new();
    let root = e.p("proj");
    fs::create_dir_all(root.join(".vscode")).unwrap();
    fs::write(e.p("u.json"), "{ not json").unwrap();
    fs::write(e.p("arr.json"), "[1,2]").unwrap();
    fs::write(root.join(".vscode/settings.json"), "\"str\"").unwrap();
    fs::write(e.p("excludes.json"), "garbage").unwrap();
    for u in [e.p("u.json"), e.p("arr.json"), e.p("missing.json")] {
        let mut src = e.sources();
        src.vscode_user = Some(u.clone());
        src.cursor_user = Some(u);
        let r = effective(&AppSettings::default(), Some(&root), &src);
        assert_eq!(r.auto_advance.source, Source::Default);
        assert_eq!(r.exclude.source, Source::Default);
    }
}

#[test]
fn default_ext_sources_paths() {
    let s = default_ext_sources(Path::new("/h"));
    assert_eq!(s.excludes_json, Path::new("/h/.claudegate/excludes.json"));
    assert_eq!(s.hooklog_sentinel, Path::new("/h/.claudegate/hooklog.enabled"));
    assert!(s.vscode_user.is_some() && s.cursor_user.is_some());
}

#[test]
fn app_settings_deserializes() {
    let a: AppSettings = serde_json::from_str(
        r#"{"settings":{"autoAdvance":false},"projects":{"/r":{"exclude":["a"],"addedByApp":true}}}"#,
    ).unwrap();
    assert_eq!(a.projects["/r"].exclude.as_deref(), Some(&["a".to_string()][..]));
    let a: AppSettings = serde_json::from_str("{}").unwrap();
    assert!(a.settings.is_empty());
}

#[test]
fn jsonc_cases() {
    assert_eq!(parse_jsonc(r#"{"a":1, // x
        "b":[1,2,],/* y */ "c":{"d":"//not /* a comment */","e":"q\"//z",},}"#).unwrap(),
        json!({"a":1,"b":[1,2],"c":{"d":"//not /* a comment */","e":"q\"//z"}}));
    assert_eq!(parse_jsonc("\u{feff}{\"a\":1}").unwrap(), json!({"a":1}));
    assert_eq!(parse_jsonc(r#"{"a":"x,}"}"#).unwrap(), json!({"a":"x,}"}));
    assert!(parse_jsonc("{ nope").is_none());
    assert!(parse_jsonc("{\"a\":1 /* unterminated").is_none());
    assert!(parse_jsonc("").is_none());
}
