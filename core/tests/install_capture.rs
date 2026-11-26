//! Capture setup: the extension's settings protocol (`hookInstaller.ts` v1.16.1) and the guarded
//! installer. `fixtures/capture/*.json` were produced by the extension's own `computeSettingsPatch`,
//! `backupsToPrune` and `verifySettingsContent`; `xcheck/run.ts` feeds them back through that
//! TypeScript, so both sides answer every case the same way.
//!
//! Every install runs against a temp HOME. Nothing here touches the real ~/.claude or ~/.claudegate.

use claudegate_core::capture::{
    adoptable_hook, audit_command_shape, backups_to_prune, bundled_hook_version, compute_settings_patch, set_up_capture,
    verify_settings_content, CaptureStatus, InstallError, AUDIT_EVENTS, AUDIT_SH_CONTENT, HOOK_SH_CONTENT,
    POST_TOOL_MATCHER, PRE_TOOL_MATCHER, SETTINGS_BACKUP_RETENTION,
};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const HOOK: &str = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\nprint('hook')\n";
const AUDIT: &str = "#!/usr/bin/env python3\nprint('audit')\n";

fn fixture(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/capture").join(name);
    serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
}

// ---- the ported pure functions, against the extension's own answers ----

#[test]
fn settings_patch_matches_the_extension() {
    let fx = fixture("settings_patch.json");
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() >= 35);
    assert!(cases.iter().filter(|c| c.get("auditCommand").is_some()).count() >= 15, "the audit cases are there");
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let p = compute_settings_patch(c["raw"].as_str().unwrap(), c["hookCommand"].as_str().unwrap(), c["auditCommand"].as_str());
        assert_eq!(p.parse_error, c["expected"]["parseError"].as_bool().unwrap(), "{name}: parseError");
        assert_eq!(p.changed, c["expected"]["changed"].as_bool().unwrap(), "{name}: changed");
        assert!(p.shape_error.is_none(), "{name}: {:?}", p.shape_error);
        assert_eq!(p.content, c["expected"]["content"].as_str().unwrap(), "{name}: content");
    }
}

#[test]
fn settings_patch_refuses_the_shapes_the_extension_refuses() {
    let fx = fixture("settings_patch.json");
    for c in fx["refuse"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let raw = c["raw"].as_str().unwrap();
        let p = compute_settings_patch(raw, c["hookCommand"].as_str().unwrap(), c["auditCommand"].as_str());
        assert!(p.shape_error.is_some(), "{name}");
        assert!(p.parse_error, "{name}: parseError too, as the extension");
        assert!(!p.changed, "{name}");
        assert_eq!(p.content, raw, "{name}: the original bytes are kept");
    }
}

#[test]
fn audit_command_shape_matches_the_extension() {
    let fx = fixture("audit_command_shape.json");
    let cases = fx["cases"].as_array().unwrap();
    assert!(cases.len() >= 20);
    for c in cases {
        let cmd = c["command"].as_str().unwrap();
        assert_eq!(audit_command_shape(cmd), c["ours"].as_bool().unwrap(), "{cmd:?}");
    }
}

#[test]
fn backups_to_prune_matches_the_extension() {
    let fx = fixture("backups_to_prune.json");
    for c in fx["cases"].as_array().unwrap() {
        let names: Vec<String> = serde_json::from_value(c["names"].clone()).unwrap();
        let keep = c.get("keep").and_then(Value::as_u64).map_or(SETTINGS_BACKUP_RETENTION, |k| k as usize);
        let want: Vec<String> = serde_json::from_value(c["expected"].clone()).unwrap();
        assert_eq!(backups_to_prune(&names, c["baseName"].as_str().unwrap(), keep), want, "{}", c["name"]);
    }
}

#[test]
fn verify_settings_matches_the_extension() {
    let fx = fixture("verify_settings.json");
    for c in fx["cases"].as_array().unwrap() {
        let got = verify_settings_content(
            c["after"].as_str().unwrap(),
            c["previous"].as_str().unwrap(),
            c["hookCommand"].as_str().unwrap(),
            c["auditCommand"].as_str(),
        );
        assert_eq!(got.as_deref(), c["expected"].as_str(), "{}", c["name"]);
    }
}

#[test]
fn constants_are_the_extensions() {
    assert_eq!(PRE_TOOL_MATCHER, "^(Write|Edit|MultiEdit|Bash)$");
    assert_eq!(SETTINGS_BACKUP_RETENTION, 5);
    assert_eq!(HOOK_SH_CONTENT, "#!/usr/bin/env bash\npython3 \"$HOME/.claudegate/hook.py\"\n");
    assert_eq!(POST_TOOL_MATCHER, "^Bash$");
    assert_eq!(AUDIT_EVENTS, ["PostToolUse", "PostToolUseFailure"]);
    assert_eq!(
        AUDIT_SH_CONTENT,
        "#!/usr/bin/env bash\n[ -f \"$HOME/.claudegate/audit.py\" ] || exit 0\npython3 \"$HOME/.claudegate/audit.py\"\n"
    );
}

#[test]
fn bundled_version_comes_from_the_stamp() {
    assert_eq!(bundled_hook_version(HOOK).as_deref(), Some("v1.16.1"));
    let real = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src-tauri/resources/hook.py")).unwrap();
    assert!(bundled_hook_version(&real).is_some_and(|v| v.starts_with('v')));
    assert_eq!(bundled_hook_version("print(1)\n"), None);
    assert_eq!(bundled_hook_version("# CLAUDEGATE_HOOK_VERSION=\n"), None);
    // Only line 2 is the stamp, the line `adoptable_hook` removes.
    assert_eq!(bundled_hook_version("# CLAUDEGATE_HOOK_VERSION=v1.0.0\nx\n"), None);
    assert_eq!(bundled_hook_version("#!/usr/bin/env python3\nx\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n"), None);
    assert_eq!(bundled_hook_version("#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0"), Some("v1.0.0".into()));
}

// ---- the installer, in a temp HOME ----

struct Home {
    _tmp: tempfile::TempDir,
    home: PathBuf,
}

impl Home {
    fn new() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        fs::create_dir(&home).unwrap();
        Home { _tmp: tmp, home }
    }
    fn settings(&self) -> PathBuf {
        self.home.join(".claude/settings.json")
    }
    fn cg(&self, f: &str) -> PathBuf {
        self.home.join(".claudegate").join(f)
    }
    fn cmd(&self) -> String {
        self.cg("hook.sh").to_str().unwrap().to_owned()
    }
    fn audit_cmd(&self) -> String {
        self.cg("audit.sh").to_str().unwrap().to_owned()
    }
    fn write_settings(&self, text: &str) {
        fs::create_dir_all(self.home.join(".claude")).unwrap();
        fs::write(self.settings(), text).unwrap();
    }
    fn backups(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(self.home.join(".claude"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.starts_with("settings.json.claudegate-") && n.ends_with(".bak"))
            .collect();
        v.sort();
        v
    }
    fn install(&self) -> Result<claudegate_core::capture::InstallReport, InstallError> {
        set_up_capture(&self.home, HOOK, AUDIT, true, false)
    }
}

fn our_entry(cmd: &str) -> Value {
    json!({ "matcher": PRE_TOOL_MATCHER, "hooks": [{ "type": "command", "command": cmd }] })
}

/// What a fresh install writes: the capture entry, then the audit pair.
fn fresh_hooks(h: &Home) -> Value {
    let audit = json!({ "matcher": POST_TOOL_MATCHER, "hooks": [{ "type": "command", "command": h.audit_cmd() }] });
    json!({ "hooks": { "PreToolUse": [our_entry(&h.cmd())], "PostToolUse": [audit], "PostToolUseFailure": [audit] } })
}

const EXISTING: &str = r#"{
  "model": "opus",
  "permissions": { "allow": ["Bash(ls)"] },
  "hooks": {
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "/x/start.sh" }] }],
    "PreToolUse": [
      { "matcher": "^Bash$", "hooks": [{ "type": "command", "command": "/usr/bin/guard", "timeout": 5 }] },
      { "matcher": "Write|Edit", "hooks": [{ "type": "command", "command": "prettier --write" }] }
    ],
    "PostToolUse": [{ "matcher": "*", "hooks": [{ "type": "command", "command": "/x/post.sh" }] }]
  },
  "enabledPlugins": { "b@m": true, "a@m": false }
}"#;

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
#[test]
fn fresh_install_appends_our_entry_and_keeps_everything_else() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::set_permissions(h.settings(), fs::Permissions::from_mode(0o600)).unwrap();

    let report = h.install().unwrap();

    let before: Value = serde_json::from_str(EXISTING).unwrap();
    let after: Value = serde_json::from_str(&fs::read_to_string(h.settings()).unwrap()).unwrap();
    // Every key but hooks.PreToolUse is unchanged as a value.
    for (k, v) in before.as_object().unwrap() {
        if k != "hooks" {
            assert_eq!(&after[k], v, "{k}");
        }
    }
    assert_eq!(after["hooks"]["SessionStart"], before["hooks"]["SessionStart"]);
    // The audit entries: after any foreign PostToolUse group, and a PostToolUseFailure of their own.
    let audit = json!({ "matcher": POST_TOOL_MATCHER, "hooks": [{ "type": "command", "command": h.audit_cmd() }] });
    assert_eq!(after["hooks"]["PostToolUse"], json!([before["hooks"]["PostToolUse"][0], audit]));
    assert_eq!(after["hooks"]["PostToolUseFailure"], json!([audit]));
    let pre = after["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 3);
    assert_eq!(pre[..2], before["hooks"]["PreToolUse"].as_array().unwrap()[..], "other matchers survive in order");
    assert_eq!(pre[2], our_entry(&h.cmd()));
    // The bytes are the extension's: JSON.stringify(_, null, 2), key order kept.
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), compute_settings_patch(EXISTING, &h.cmd(), Some(&h.audit_cmd())).content);
    assert_eq!(mode(&h.settings()), 0o600, "the file's mode is kept");

    // Hook files and the marker.
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
    assert_eq!(fs::read_to_string(h.cg("hook.sh")).unwrap(), HOOK_SH_CONTENT);
    assert_eq!(mode(&h.cg("hook.sh")), 0o755);
    assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), AUDIT);
    assert_eq!(fs::read_to_string(h.cg("audit.sh")).unwrap(), AUDIT_SH_CONTENT);
    assert_eq!(mode(&h.cg("audit.sh")), 0o755);
    assert_eq!(fs::read_to_string(h.cg(".installed-by-desktop")).unwrap(), "v1.16.1");
    let created: Vec<PathBuf> = ["hook.py", "hook.sh", "audit.py", "audit.sh"].iter().map(|f| h.cg(f)).collect();
    assert_eq!(report.created, created);

    // One backup, the original bytes, beside the settings file, private like the original.
    let backups = h.backups();
    assert_eq!(backups.len(), 1);
    let bak = h.home.join(".claude").join(&backups[0]);
    assert_eq!(fs::read_to_string(&bak).unwrap(), EXISTING);
    assert_eq!(mode(&bak), 0o600);
    assert_eq!(report.backup.as_deref(), Some(bak.as_path()));
    assert_eq!(report.version, "v1.16.1");
    assert_eq!(report.note, None);

    // The status now reads as set up by this app.
    let status = claudegate_core::capture::capture_status(
        Some(&fs::read_to_string(h.settings()).unwrap()),
        h.cg(".installed-by-desktop").exists(),
        true,
    );
    assert_eq!(status, CaptureStatus::SetUpByApp { version: None, python_missing: false });
}

#[test]
fn backup_name_is_the_extensions() {
    let h = Home::new();
    h.write_settings("{}");
    h.install().unwrap();
    let b = &h.backups()[0];
    // settings.json.claudegate-<ISO with ':' replaced by '-'>.bak, e.g. 2026-11-18T10-20-30.123Z
    let stamp = b.strip_prefix("settings.json.claudegate-").unwrap().strip_suffix(".bak").unwrap();
    assert_eq!(stamp.len(), 24, "{stamp}");
    let bytes = stamp.as_bytes();
    assert_eq!((bytes[4], bytes[7], bytes[10], bytes[13], bytes[16], bytes[19], bytes[23]), (b'-', b'-', b'T', b'-', b'-', b'.', b'Z'));
}

#[cfg(unix)]
#[test]
fn symlinked_settings_keep_the_link_and_update_the_target() {
    use std::os::unix::fs::symlink;
    let h = Home::new();
    let dotfiles = h.home.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    fs::create_dir_all(h.home.join(".claude")).unwrap();
    let real = dotfiles.join("claude-settings.json");
    fs::write(&real, EXISTING).unwrap();
    symlink(&real, h.settings()).unwrap();

    let report = h.install().unwrap();

    assert!(fs::symlink_metadata(h.settings()).unwrap().file_type().is_symlink(), "the link survives");
    let after: Value = serde_json::from_str(&fs::read_to_string(&real).unwrap()).unwrap();
    assert_eq!(after["hooks"]["PreToolUse"][2], our_entry(&h.cmd()));
    assert_eq!(report.settings, fs::canonicalize(&real).unwrap());
    // The backup sits beside the link (in ~/.claude), not in the dotfiles repo.
    assert_eq!(h.backups().len(), 1);
    let in_dotfiles: Vec<_> = fs::read_dir(&dotfiles).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(in_dotfiles.len(), 1, "{in_dotfiles:?}");
}

#[cfg(unix)]
#[test]
fn dangling_settings_symlink_is_refused() {
    use std::os::unix::fs::symlink;
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claude")).unwrap();
    symlink(h.home.join("nowhere.json"), h.settings()).unwrap();
    assert!(h.install().is_err());
    assert!(fs::symlink_metadata(h.settings()).unwrap().file_type().is_symlink());
    assert!(!h.home.join("nowhere.json").exists());
    assert!(!h.cg("hook.py").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_home_installs() {
    use std::os::unix::fs::symlink;
    let h = Home::new();
    let link = h.home.parent().unwrap().join("linkhome");
    symlink(&h.home, &link).unwrap();
    let report = set_up_capture(&link, HOOK, AUDIT, true, false).unwrap();
    assert_eq!(report.settings, fs::canonicalize(h.settings()).unwrap());
    assert!(fs::read_to_string(h.settings()).unwrap().contains("hook.sh"));
}

#[test]
fn missing_settings_gets_a_file_with_only_the_hooks_block() {
    let h = Home::new();
    let report = h.install().unwrap();
    let after: Value = serde_json::from_str(&fs::read_to_string(h.settings()).unwrap()).unwrap();
    assert_eq!(after, fresh_hooks(&h));
    assert!(report.backup.is_none(), "nothing to back up");
    assert!(h.backups().is_empty());
}

#[test]
fn empty_settings_is_a_fresh_install_but_still_backed_up() {
    let h = Home::new();
    h.write_settings("  \n");
    h.install().unwrap();
    let after: Value = serde_json::from_str(&fs::read_to_string(h.settings()).unwrap()).unwrap();
    assert_eq!(after, fresh_hooks(&h));
    assert_eq!(h.backups().len(), 1);
}

#[test]
fn second_install_is_refused_and_writes_nothing() {
    let h = Home::new();
    h.write_settings(EXISTING);
    h.install().unwrap();
    let settings = fs::read(h.settings()).unwrap();
    let backups = h.backups();

    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::AlreadySetUp(CaptureStatus::SetUpByApp { .. })), "{err:?}");
    assert_eq!(fs::read(h.settings()).unwrap(), settings);
    assert_eq!(h.backups(), backups, "no new backup");
}

#[test]
fn extension_registration_is_refused() {
    let h = Home::new();
    let text = serde_json::to_string(&json!({ "hooks": { "PreToolUse": [our_entry("/elsewhere/.claudegate/hook.sh")] } })).unwrap();
    h.write_settings(&text);
    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::AlreadySetUp(CaptureStatus::SetUpByExtension { .. })), "{err:?}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), text);
    assert!(!h.home.join(".claudegate").exists());
}

#[test]
fn backups_are_pruned_to_the_extensions_keep_count() {
    let h = Home::new();
    h.write_settings("{}");
    let dir = h.home.join(".claude");
    for d in 1..=7 {
        fs::write(dir.join(format!("settings.json.claudegate-2020-01-0{d}T00-00-00.000Z.bak")), "old").unwrap();
    }
    fs::write(dir.join("settings.json.claudegate.bak"), "legacy").unwrap();
    fs::write(dir.join("settings.local.json.claudegate-2019-01-01T00-00-00.000Z.bak"), "other").unwrap();

    h.install().unwrap();

    let left = h.backups();
    assert_eq!(left.len(), SETTINGS_BACKUP_RETENTION, "{left:?}");
    assert!(left[0].contains("2020-01-04"), "the oldest went first: {left:?}");
    assert!(!left.last().unwrap().contains("2020-"), "the new backup is kept: {left:?}");
    assert!(dir.join("settings.json.claudegate.bak").exists(), "the legacy backup is never pruned");
    assert!(dir.join("settings.local.json.claudegate-2019-01-01T00-00-00.000Z.bak").exists());
}

fn assert_untouched(h: &Home, text: Option<&str>) {
    match text {
        Some(t) => assert_eq!(fs::read_to_string(h.settings()).unwrap(), t),
        None => assert!(!h.settings().exists()),
    }
    for f in ["hook.py", "hook.sh", "audit.py", "audit.sh", ".installed-by-desktop"] {
        assert!(!h.cg(f).exists(), "{f}");
    }
    if h.home.join(".claude").exists() {
        assert!(h.backups().is_empty());
    }
}

#[test]
fn settings_with_comments_or_not_an_object_are_refused() {
    for text in ["{\n  // mine\n  \"model\": \"x\"\n}", "{/* c */}", "[]", "null", "{ broken"] {
        let h = Home::new();
        h.write_settings(text);
        let err = h.install().unwrap_err();
        assert!(matches!(err, InstallError::SettingsUnreadable(_)), "{text:?}: {err:?}");
        assert_untouched(&h, Some(text));
    }
}

#[test]
fn hooks_of_the_wrong_shape_are_refused() {
    for text in [r#"{"hooks":"x"}"#, r#"{"hooks":[]}"#, r#"{"hooks":{"PreToolUse":{}}}"#] {
        let h = Home::new();
        h.write_settings(text);
        assert!(h.install().is_err(), "{text}");
        assert_untouched(&h, Some(text));
    }
}

#[test]
fn python_missing_is_refused() {
    let h = Home::new();
    h.write_settings("{}");
    let err = set_up_capture(&h.home, HOOK, AUDIT, false, false).unwrap_err();
    assert!(matches!(err, InstallError::PythonMissing), "{err:?}");
    assert_untouched(&h, Some("{}"));
}

#[test]
fn read_only_is_refused() {
    let h = Home::new();
    h.write_settings("{}");
    let err = set_up_capture(&h.home, HOOK, AUDIT, true, true).unwrap_err();
    assert!(matches!(err, InstallError::ReadOnly), "{err:?}");
    assert_untouched(&h, Some("{}"));
}

#[test]
fn bundled_hook_without_a_version_is_refused() {
    let h = Home::new();
    let err = set_up_capture(&h.home, "print(1)\n", AUDIT, true, false).unwrap_err();
    assert!(matches!(err, InstallError::BundledHook), "{err:?}");
    assert_untouched(&h, None);
}

#[test]
fn existing_hook_py_is_refused_and_left_alone() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), "the extension's hook").unwrap();

    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::HookFilesExist), "{err:?}");
    assert_eq!(
        err.to_string(),
        "ClaudeGate's hook files already exist (likely from the VS Code extension). \
         Run \"Claude Gate: Setup Hook\" in VS Code, or remove ~/.claudegate/hook.py and audit.py if the extension is gone."
    );
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "the extension's hook");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING);
    assert!(!h.cg("hook.sh").exists());
    assert!(!h.cg(".installed-by-desktop").exists());
    assert!(h.backups().is_empty());
}

#[test]
fn leftover_hook_files_from_this_app_get_their_own_message() {
    // An older stamped hook is neither form setup adopts, so it is still refused.
    const OLDER: &str = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.15.0\nprint('hook')\n";
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), OLDER).unwrap();
    fs::write(h.cg(".installed-by-desktop"), "v1.15.0").unwrap();
    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::HookFilesFromApp), "{err:?}");
    assert_eq!(
        err.to_string(),
        "ClaudeGate's hook files from an earlier setup in this app already exist. \
         Remove ~/.claudegate/hook.py, hook.sh, audit.py, audit.sh and .installed-by-desktop to set up again."
    );
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), OLDER);
    assert!(!h.settings().exists());
}

/// `HOOK` without its stamp line: the extension's hook.py at the pinned tag.
const UNSTAMPED: &str = "#!/usr/bin/env python3\nprint('hook')\n";

#[test]
fn a_leftover_extension_hook_py_is_adopted() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), UNSTAMPED).unwrap();

    let report = h.install().unwrap();
    assert!(report.adopted);
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK, "replaced with the stamped bundled hook");
    assert!(!report.created.contains(&h.cg("hook.py")), "{:?}", report.created);
    assert!(report.created.contains(&h.cg("hook.sh")));
    assert_eq!(fs::read_to_string(h.cg(".installed-by-desktop")).unwrap(), "v1.16.1");
    let v: Value = serde_json::from_str(&fs::read_to_string(h.settings()).unwrap()).unwrap();
    assert!(v["hooks"]["PreToolUse"].as_array().unwrap().contains(&our_entry(&h.cmd())));
}

#[test]
fn the_apps_own_leftover_hook_py_is_adopted() {
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), HOOK).unwrap();
    fs::write(h.cg("hook.sh"), HOOK_SH_CONTENT).unwrap();
    fs::write(h.cg(".installed-by-desktop"), "v1.16.1").unwrap();

    let report = h.install().unwrap();
    assert!(report.adopted);
    assert_eq!(report.created, vec![h.cg("audit.py"), h.cg("audit.sh")], "only the audit files are new");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
    assert!(h.settings().exists());
}

#[test]
fn a_hook_py_one_byte_off_is_refused_and_untouched() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    let near = UNSTAMPED.replace("hook", "hooK");
    fs::write(h.cg("hook.py"), &near).unwrap();

    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::HookFilesExist), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), near);
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING);
    assert!(!h.cg("hook.sh").exists());
    assert!(!h.cg(".installed-by-desktop").exists());
    assert!(h.backups().is_empty());
}

#[cfg(unix)]
#[test]
fn a_symlinked_leftover_hook_py_is_not_adopted() {
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    let real = h.home.join("real-hook.py");
    fs::write(&real, UNSTAMPED).unwrap();
    std::os::unix::fs::symlink(&real, h.cg("hook.py")).unwrap();

    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::HookFilesExist), "{err:?}");
    assert_eq!(fs::read_to_string(&real).unwrap(), UNSTAMPED);
    assert!(fs::symlink_metadata(h.cg("hook.py")).unwrap().file_type().is_symlink());
}

#[test]
fn a_fresh_install_adopts_nothing() {
    let h = Home::new();
    let report = h.install().unwrap();
    assert!(!report.adopted);
    assert!(report.created.contains(&h.cg("hook.py")));
}

#[test]
fn foreign_hook_sh_is_refused_and_left_alone() {
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.sh"), "#!/bin/sh\necho mine\n").unwrap();
    assert!(h.install().is_err());
    assert_eq!(fs::read_to_string(h.cg("hook.sh")).unwrap(), "#!/bin/sh\necho mine\n");
    assert!(!h.cg("hook.py").exists());
    assert!(!h.settings().exists());
}

#[test]
fn identical_hook_sh_is_reused() {
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.sh"), HOOK_SH_CONTENT).unwrap();
    let report = h.install().unwrap();
    assert!(!report.created.contains(&h.cg("hook.sh")), "{:?}", report.created);
    assert!(report.created.contains(&h.cg("hook.py")));
}

#[test]
fn the_real_bundled_hook_adopts_itself_and_its_unstamped_form_only() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src-tauri/resources/hook.py");
    let bundled = fs::read_to_string(p).unwrap();
    let mut lines: Vec<&str> = bundled.split_inclusive('\n').collect();
    assert!(lines[1].starts_with("# CLAUDEGATE_HOOK_VERSION="), "the stamp is line 2");
    assert!(adoptable_hook(bundled.as_bytes(), &bundled));
    lines.remove(1);
    let unstamped = lines.concat();
    assert!(adoptable_hook(unstamped.as_bytes(), &bundled));
    assert!(!adoptable_hook(format!("{unstamped}\n").as_bytes(), &bundled));
    assert!(!adoptable_hook(unstamped.trim_end().as_bytes(), &bundled));
    assert!(!adoptable_hook(b"", &bundled));
    assert!(!adoptable_hook(b"x", "no stamp here\n"), "an unstamped bundle adopts nothing but itself");
}

#[cfg(unix)]
#[test]
fn a_symlinked_marker_is_refused_before_anything_is_written() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    let real = h.home.join("real-marker");
    fs::write(&real, "v0.0.1").unwrap();
    std::os::unix::fs::symlink(&real, h.cg(".installed-by-desktop")).unwrap();

    let err = h.install().unwrap_err();
    assert!(matches!(err, InstallError::MarkerNotFile), "{err:?}");
    assert!(err.to_string().contains("~/.claudegate/.installed-by-desktop is not a regular file"), "{err}");
    assert_eq!(fs::read_to_string(&real).unwrap(), "v0.0.1");
    assert!(fs::symlink_metadata(h.cg(".installed-by-desktop")).unwrap().file_type().is_symlink());
    assert!(!h.cg("hook.py").exists());
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING);
    assert!(h.backups().is_empty());
}

#[test]
fn the_stamp_is_removed_only_from_line_two() {
    // Line 1 or line 3 stamps: only the bundled hook itself is adoptable.
    for bundled in [
        "# CLAUDEGATE_HOOK_VERSION=v1.16.1\n#!/usr/bin/env python3\nprint('hook')\n",
        "#!/usr/bin/env python3\nprint('hook')\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\n",
    ] {
        assert!(adoptable_hook(bundled.as_bytes(), bundled));
        assert!(!adoptable_hook(b"#!/usr/bin/env python3\nprint('hook')\n", bundled), "{bundled:?}");
    }
    let line2 = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\nprint('hook')\n";
    assert!(adoptable_hook(b"#!/usr/bin/env python3\nprint('hook')\n", line2));
    // A stamp without its newline (the last line) is not "that line".
    assert!(!adoptable_hook(b"#!/usr/bin/env python3\n", "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1"));
}

// ---- the capture-audit hook ----

#[cfg(unix)]
#[test]
fn the_audit_wrapper_exits_quietly_without_audit_py() {
    let h = Home::new();
    h.install().unwrap();
    fs::remove_file(h.cg("audit.py")).unwrap();
    let out = std::process::Command::new(h.cg("audit.sh")).env("HOME", &h.home).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
}

#[test]
fn an_identical_audit_py_and_wrapper_are_kept_not_refused() {
    let h = Home::new();
    fs::create_dir_all(h.cg("")).unwrap();
    fs::write(h.cg("audit.py"), AUDIT).unwrap();
    fs::write(h.cg("audit.sh"), AUDIT_SH_CONTENT).unwrap();
    let report = h.install().unwrap();
    assert_eq!(report.created, vec![h.cg("hook.py"), h.cg("hook.sh")], "existing identical files are not ours to list");
    let after: Value = serde_json::from_str(&fs::read_to_string(h.settings()).unwrap()).unwrap();
    for ev in AUDIT_EVENTS {
        assert_eq!(after["hooks"][ev][0]["hooks"][0]["command"], json!(h.audit_cmd()), "{ev}");
    }
}

#[test]
fn a_different_audit_py_or_wrapper_refuses_setup_and_writes_nothing() {
    let h = Home::new();
    h.write_settings("{}");
    fs::create_dir_all(h.cg("")).unwrap();
    fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
    assert!(matches!(h.install().unwrap_err(), InstallError::HookFilesExist));
    fs::remove_file(h.cg("audit.py")).unwrap();
    fs::write(h.cg("audit.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    assert!(matches!(h.install().unwrap_err(), InstallError::ForeignWrapper));
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), "{}");
    assert!(!h.cg("hook.py").exists() && !h.cg("audit.py").exists());
    assert!(h.backups().is_empty());
}

#[test]
fn a_post_event_of_the_wrong_shape_refuses_setup() {
    for raw in [r#"{"hooks":{"PostToolUse":{}}}"#, r#"{"hooks":{"PostToolUseFailure":"x"}}"#, r#"{"hooks":false}"#] {
        let h = Home::new();
        h.write_settings(raw);
        let err = h.install().unwrap_err();
        assert!(matches!(err, InstallError::SettingsUnreadable(_)), "{raw}: {err:?}");
        assert_eq!(fs::read_to_string(h.settings()).unwrap(), raw);
        assert!(!h.cg("hook.py").exists() && !h.cg("audit.sh").exists(), "{raw}");
    }
}

#[test]
fn audit_entries_alone_do_not_count_as_set_up() {
    let h = Home::new();
    let only_audit = json!({ "hooks": {
        "PostToolUse": [{ "matcher": "^Bash$", "hooks": [{ "type": "command", "command": h.audit_cmd() }] }],
        "PostToolUseFailure": [{ "matcher": "^Bash$", "hooks": [{ "type": "command", "command": h.audit_cmd() }] }],
    }});
    let text = only_audit.to_string();
    for marker in [false, true] {
        assert_eq!(claudegate_core::capture::capture_status(Some(&text), marker, true), CaptureStatus::NotSetUp);
    }
    // And with the capture entry, the status is what it was without post entries.
    let mut both = only_audit.clone();
    both["hooks"]["PreToolUse"] = json!([our_entry(&h.cmd())]);
    let plain = json!({ "hooks": { "PreToolUse": [our_entry(&h.cmd())] } });
    for marker in [false, true] {
        assert_eq!(
            claudegate_core::capture::capture_status(Some(&both.to_string()), marker, true),
            claudegate_core::capture::capture_status(Some(&plain.to_string()), marker, true),
        );
    }
    // Setup over audit-only entries keeps them as they are and adds the capture entry.
    h.write_settings(&text);
    h.install().unwrap();
    let after: Value = serde_json::from_str(&fs::read_to_string(h.settings()).unwrap()).unwrap();
    for ev in AUDIT_EVENTS {
        assert_eq!(after["hooks"][ev], only_audit["hooks"][ev], "{ev}");
    }
    assert_eq!(after["hooks"]["PreToolUse"], json!([our_entry(&h.cmd())]));
}

#[test]
fn audit_command_shape_spot_checks() {
    assert!(audit_command_shape("/home/u/.claudegate/audit.sh"));
    assert!(audit_command_shape("C:\\Users\\u\\.claudegate\\audit.bat"));
    assert!(!audit_command_shape("/x/claudegate/lint.sh"));
    assert!(!audit_command_shape("/home/u/.claudegate/audit.sh --flag"));
    assert!(!audit_command_shape(".claudegate/audit.sh"));
}
