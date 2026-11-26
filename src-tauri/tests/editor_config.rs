//! Config and editor tests. Every file touched lives in a temp dir; the real
//! ~/.claudegate and ~/.claudegate-desktop are never read or written.
use claudegate_desktop_lib::config::{
    allowed_template, config_path_in, load_or_create, save_editor, Config, DEFAULT_EDITOR, VSCODE, ZED,
};
use claudegate_desktop_lib::editor::{
    check_target, render_editor_command, resolve_program, resolve_with_bundles,
};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn render_splits_template_before_substituting() {
    assert_eq!(
        render_editor_command("zed {path}:{line}", "/a b/c.ts", 12, 1),
        vec!["zed".to_string(), "/a b/c.ts:12".to_string()]
    );
    assert_eq!(
        render_editor_command("code -g {path}:{line}", "/a b/c.ts", 12, 1),
        vec!["code".to_string(), "-g".to_string(), "/a b/c.ts:12".to_string()]
    );
}

#[test]
fn render_appends_path_when_template_has_no_placeholder() {
    assert_eq!(
        render_editor_command("subl", "/x y.rs", 3, 1),
        vec!["subl".to_string(), "/x y.rs".to_string()]
    );
}

#[test]
fn render_does_not_reexpand_placeholders_inside_the_path() {
    assert_eq!(
        render_editor_command("zed {path}:{line}", "/w/{line}.ts", 7, 1),
        vec!["zed".to_string(), "/w/{line}.ts:7".to_string()]
    );
}

#[test]
fn missing_config_gives_default_and_creates_it() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    assert_eq!(p, home.path().join(".claudegate-desktop").join("config.json"));
    let c = load_or_create(&p);
    assert_eq!(c, Config { editor: DEFAULT_EDITOR.into(), custom_editor: None, read_only: false, workspace: None, app_roots: vec![] });
    assert_eq!(DEFAULT_EDITOR, "zed {path}:{line}:{col}");
    assert!(p.is_file(), "defaults written to the app's own dir");
    assert_eq!(load_or_create(&p), c);
}

#[test]
fn malformed_config_reads_as_default_and_is_not_overwritten_on_load() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, "{ nope").unwrap();
    assert_eq!(load_or_create(&p).editor, DEFAULT_EDITOR);
    assert_eq!(fs::read_to_string(&p).unwrap(), "{ nope");
    assert!(save_editor(&p, VSCODE, None).is_err(), "never clobber a file we cannot parse");
}

#[test]
fn custom_editor_is_read_and_preserved_by_set_editor() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{"editor":"zed {path}:{line}","customEditor":"nvim +{line} {path}","other":1}"#).unwrap();
    let c = load_or_create(&p);
    assert_eq!(c.custom_editor.as_deref(), Some("nvim +{line} {path}"));
    let saved = save_editor(&p, VSCODE, None).unwrap();
    assert_eq!(saved.editor, VSCODE);
    assert_eq!(saved.custom_editor.as_deref(), Some("nvim +{line} {path}"));
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(v["other"], 1, "unknown keys survive");
    assert_eq!(load_or_create(&p).editor, VSCODE);
}

#[test]
fn set_editor_creates_dir_and_file_and_rejects_empty() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    assert!(save_editor(&p, "   ", None).is_err());
    let c = save_editor(&p, VSCODE, None).unwrap();
    assert_eq!(c.editor, VSCODE);
    assert!(p.is_file());
}

#[test]
fn set_editor_refuses_paths_under_the_claudegate_home() {
    let home = tempfile::tempdir().unwrap();
    let cg = home.path().join(".claudegate");
    let p = cg.join("config.json");
    assert!(save_editor(&p, VSCODE, Some(&cg)).is_err());
    assert!(!cg.exists());
}

fn fake(set: Vec<PathBuf>) -> impl Fn(&Path) -> bool {
    move |p: &Path| set.iter().any(|s| s == p)
}

#[test]
fn resolver_prefers_path_then_extra_dirs() {
    let path_var = std::env::join_paths(["/p1", "/p2"]).unwrap();
    let extra = vec![PathBuf::from("/home/u/.local/bin"), PathBuf::from("/usr/local/bin")];
    let exists = fake(vec![PathBuf::from("/p2/zed"), PathBuf::from("/home/u/.local/bin/zed")]);
    assert_eq!(
        resolve_program("zed", Some(&path_var), &extra, &exists),
        Some(PathBuf::from("/p2/zed"))
    );
    // A desktop launch often has a bare PATH: fall back to the install dirs.
    assert_eq!(
        resolve_program("zed", None, &extra, &exists),
        Some(PathBuf::from("/home/u/.local/bin/zed"))
    );
    assert_eq!(resolve_program("code", Some(&path_var), &extra, &exists), None);
}

#[test]
fn resolver_takes_explicit_paths_as_given() {
    let exists = fake(vec![PathBuf::from("/opt/ed/bin/ed")]);
    assert_eq!(
        resolve_program("/opt/ed/bin/ed", None, &[], &exists),
        Some(PathBuf::from("/opt/ed/bin/ed"))
    );
    assert_eq!(resolve_program("/nope/ed", None, &[], &exists), None);
    assert_eq!(resolve_program("", None, &[], &exists), None);
}

#[test]
fn resolver_falls_back_to_an_app_bundled_cli() {
    let cli = PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli");
    let exists = fake(vec![cli.clone()]);
    let extra = vec![PathBuf::from("/usr/local/bin")];
    assert_eq!(resolve_with_bundles("zed", None, &extra, &[cli.clone()], &exists), Some(cli.clone()));
    let on_path = fake(vec![PathBuf::from("/usr/local/bin/zed"), cli.clone()]);
    assert_eq!(
        resolve_with_bundles("zed", None, &extra, &[cli], &on_path),
        Some(PathBuf::from("/usr/local/bin/zed"))
    );
}

#[test]
fn only_presets_the_custom_editor_or_the_hand_set_editor_are_allowed() {
    let c = Config { editor: "subl {path}:{line}".into(), custom_editor: Some("nvim +{line} {path}".into()), read_only: false, workspace: None, app_roots: vec![] };
    assert_eq!(allowed_template(ZED, &c).unwrap(), ZED);
    assert_eq!(allowed_template("  code   -g {path}:{line}:{col} ", &c).unwrap(), VSCODE);
    assert!(allowed_template("nvim +{line}  {path}", &c).is_ok());
    assert!(allowed_template("subl {path}:{line}", &c).is_ok(), "the editor the user wrote by hand");
    for bad in ["sh -c {path}", "rm -rf {path}", "zed --dev-server-token x {path}", ""] {
        assert!(allowed_template(bad, &c).is_err(), "{bad}");
    }
    assert!(allowed_template("nvim +{line} {path}", &Config::default()).is_err());
}

#[test]
fn set_editor_rejects_an_unknown_template_and_leaves_the_file_alone() {
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    assert!(save_editor(&p, "sh -c 'curl x | sh' {path}", None).is_err());
    assert!(!p.exists());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{"editor":"zed {path}:{line}","customEditor":"hx {path}:{line}"}"#).unwrap();
    assert_eq!(save_editor(&p, "hx {path}:{line}", None).unwrap().editor, "hx {path}:{line}");
    assert!(save_editor(&p, "evil {path}", None).is_err());
    assert_eq!(load_or_create(&p).editor, "hx {path}:{line}");
}

#[test]
fn target_must_be_an_absolute_path() {
    assert!(check_target("/w/a.ts").is_ok());
    assert!(check_target("-r").is_err());
    assert!(check_target("--new-window").is_err());
    assert!(check_target("rel/a.ts").is_err());
    assert!(check_target("").is_err());
}

// UX wave 9: open at the exact line and column (Ctrl/Cmd+click, F12).

#[test]
fn presets_carry_the_column() {
    assert_eq!(ZED, "zed {path}:{line}:{col}");
    assert_eq!(VSCODE, "code -g {path}:{line}:{col}");
}

#[test]
fn render_substitutes_the_column() {
    assert_eq!(render_editor_command(ZED, "/a b/c.ts", 12, 5), vec!["zed".to_string(), "/a b/c.ts:12:5".to_string()]);
    assert_eq!(
        render_editor_command(VSCODE, "/a/c.ts", 3, 9),
        vec!["code".to_string(), "-g".to_string(), "/a/c.ts:3:9".to_string()]
    );
    // Column 0 means "no column": 1. A `{col}` inside the path is never expanded.
    assert_eq!(render_editor_command(ZED, "/w/{col}.ts", 7, 0), vec!["zed".to_string(), "/w/{col}.ts:7:1".to_string()]);
    // A template without `{col}` ignores it.
    assert_eq!(render_editor_command("hx {path}:{line}", "/f", 2, 8), vec!["hx".to_string(), "/f:2".to_string()]);
}

#[test]
fn an_old_preset_is_upgraded_on_load_and_only_the_new_one_is_allowed() {
    for (old, new) in [("code -g {path}:{line}", VSCODE), ("zed {path}:{line}", ZED), ("  code  -g {path}:{line} ", VSCODE)] {
        let home = tempfile::tempdir().unwrap();
        let p = config_path_in(home.path());
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let text = format!(r#"{{"editor":{},"readOnly":true,"mine":{{"k":1}}}}"#, serde_json::to_string(old).unwrap());
        fs::write(&p, &text).unwrap();
        let c = load_or_create(&p);
        assert_eq!(c.editor, new, "{old}");
        assert!(c.read_only);
        // Loading never rewrites the user's file.
        assert_eq!(fs::read_to_string(&p).unwrap(), text);
        // The allow-list is unchanged in kind: presets, customEditor, the stored editor (upgraded).
        assert!(allowed_template(new, &c).is_ok());
        assert!(allowed_template("sh -c {path}", &c).is_err());
    }
    // A hand-written editor that merely resembles a preset is kept as written.
    let home = tempfile::tempdir().unwrap();
    let p = config_path_in(home.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{"editor":"code {path}:{line}"}"#).unwrap();
    assert_eq!(load_or_create(&p).editor, "code {path}:{line}");
}

// The workspace lives in the app's own config, written through the same guarded writer.

#[test]
fn the_workspace_ignores_a_malformed_value_and_a_bad_file_is_never_clobbered() {
    use claudegate_desktop_lib::config::add_to_workspace;
    let home = tempfile::tempdir().unwrap();
    let cg = home.path().join(".claudegate");
    let p = config_path_in(home.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, r#"{"workspace":"nope"}"#).unwrap();
    assert!(load_or_create(&p).workspace.is_none());
    fs::write(&p, "{not json").unwrap();
    assert!(add_to_workspace(&p, &cg, "/w/a", None).is_err());
    assert_eq!(fs::read_to_string(&p).unwrap(), "{not json");
    // A config path inside the ClaudeGate home is refused outright.
    fs::create_dir_all(&cg).unwrap();
    assert!(add_to_workspace(&cg.join("config.json"), &cg, "/w/a", Some(&cg)).is_err());
    assert!(!cg.join("config.json").exists());
}

/// M1: every config.json write is a read-modify-write of the whole file. Two threads writing
/// different keys at once must not lose each other's writes (nor fail on a shared temp file).
#[test]
fn concurrent_writes_of_different_keys_both_survive() {
    use claudegate_desktop_lib::config::{save_extra_hide, save_setting};
    let dir = tempfile::tempdir().unwrap();
    let p = config_path_in(dir.path());
    const N: usize = 200;
    let (pa, pb) = (p.clone(), p.clone());
    let a = std::thread::spawn(move || {
        for i in 0..N {
            save_setting(&pa, "pendingAgeMinDays", serde_json::json!(i), None).unwrap();
        }
    });
    let b = std::thread::spawn(move || {
        for i in 0..N {
            save_extra_hide(&pb, "/w/proj", &[format!("**/b{i}/**")], None).unwrap();
        }
    });
    a.join().unwrap();
    b.join().unwrap();
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    assert_eq!(v["settings"]["pendingAge.minDays"], serde_json::json!(N - 1), "thread A's last write survives: {v}");
    assert_eq!(v["projects"]["/w/proj"]["extraHide"], serde_json::json!([format!("**/b{}/**", N - 1)]), "thread B's last write survives: {v}");
}
