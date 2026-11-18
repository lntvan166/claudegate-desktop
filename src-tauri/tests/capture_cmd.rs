//! Capture status, setup and the startup hook update through the app's functions, in a temp HOME
//! with a temp desktop config. The real ~/.claude, ~/.claudegate and ~/.claudegate-desktop are
//! never touched.
use claudegate_core::capture::{bundled_hook_version, version_newer, CaptureStatus, HookUpdate, InstallError};
use claudegate_desktop_lib::capture::{
    capture_view_for, install_error_text, python3_available_with, read_status, set_up_capture_with, status_key,
    update_hook_with, CaptureView, BUNDLED_HOOK,
};
use claudegate_desktop_lib::config::{config_path_in, save_read_only};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Fx {
    _t: tempfile::TempDir,
    home: PathBuf,
    cfg: PathBuf,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    fs::create_dir_all(home.join(".claude")).unwrap();
    let cfg = config_path_in(&t.path().join("desk"));
    Fx { home, cfg, _t: t }
}

impl Fx {
    fn settings(&self) -> PathBuf {
        self.home.join(".claude/settings.json")
    }
    fn cg(&self, f: &str) -> PathBuf {
        self.home.join(".claudegate").join(f)
    }
    fn write_settings(&self, s: &str) {
        fs::write(self.settings(), s).unwrap();
    }
    /// The extension's registration: its hook files and its settings entry, no marker.
    fn as_extension(&self) {
        fs::create_dir_all(self.home.join(".claudegate")).unwrap();
        fs::write(self.cg("hook.py"), "# the extension's hook, unstamped\n").unwrap();
        let cmd = self.cg("hook.sh");
        self.write_settings(&format!(
            r#"{{"hooks":{{"PreToolUse":[{{"matcher":"x","hooks":[{{"type":"command","command":"{}"}}]}}]}}}}"#,
            cmd.display()
        ));
    }
    /// Set up by this app, then the marker rewritten to `marker` (no trailing newline).
    fn as_app(&self, marker: &str) {
        set_up_capture_with(&self.home, &self.cfg, true).unwrap();
        fs::write(self.cg(".installed-by-desktop"), marker).unwrap();
    }
}

fn bundled() -> String {
    bundled_hook_version(BUNDLED_HOOK).expect("the bundled hook is stamped")
}

// ---- status ----------------------------------------------------------------------------------

#[test]
fn status_keys_cover_every_variant() {
    assert_eq!(status_key(&CaptureStatus::SetUpByExtension), "vscode");
    assert_eq!(status_key(&CaptureStatus::SetUpByApp { version: None }), "app");
    assert_eq!(status_key(&CaptureStatus::NotSetUp), "not-set-up");
    assert_eq!(status_key(&CaptureStatus::SettingsUnreadable("x".into())), "settings-unreadable");
    assert_eq!(status_key(&CaptureStatus::PythonMissing), "python-missing");
}

#[test]
fn not_set_up_when_settings_absent_or_without_entry() {
    let f = fx();
    let v = capture_view_for(&f.home, true);
    assert_eq!(v, CaptureView { status: "not-set-up".into(), version: None, note: None });
    f.write_settings(r#"{"model":"x","hooks":{"PreToolUse":[]}}"#);
    assert_eq!(capture_view_for(&f.home, true).status, "not-set-up");
}

#[test]
fn vscode_when_entry_without_marker() {
    let f = fx();
    f.as_extension();
    assert_eq!(read_status(&f.home, true), CaptureStatus::SetUpByExtension);
    assert_eq!(capture_view_for(&f.home, true).status, "vscode");
}

#[test]
fn app_with_trimmed_marker_version() {
    let f = fx();
    f.as_app("v1.2.3\n");
    assert_eq!(read_status(&f.home, true), CaptureStatus::SetUpByApp { version: Some("v1.2.3".into()) });
    let v = capture_view_for(&f.home, true);
    assert_eq!(v, CaptureView { status: "app".into(), version: Some("v1.2.3".into()), note: None });
}

#[test]
fn unreadable_settings_carries_the_reason() {
    let f = fx();
    f.write_settings("{ // comment\n}");
    let v = capture_view_for(&f.home, true);
    assert_eq!(v.status, "settings-unreadable");
    assert!(v.note.as_deref().is_some_and(|n| !n.is_empty()), "{v:?}");
}

#[test]
fn dangling_settings_link_is_unreadable_not_absent() {
    let f = fx();
    std::os::unix::fs::symlink(f.home.join("nowhere.json"), f.settings()).unwrap();
    assert_eq!(capture_view_for(&f.home, true).status, "settings-unreadable");
}

#[test]
fn python_missing_wins() {
    let f = fx();
    f.as_extension();
    assert_eq!(capture_view_for(&f.home, false).status, "python-missing");
}

#[test]
fn status_reads_home_not_claudegate_home() {
    // CLAUDEGATE_HOME is ignored: a marker elsewhere does not make the extension's setup ours.
    let f = fx();
    f.as_extension();
    let other = f.home.join("elsewhere");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join(".installed-by-desktop"), "v1.16.1").unwrap();
    std::env::set_var("CLAUDEGATE_HOME", &other);
    let s = read_status(&f.home, true);
    std::env::remove_var("CLAUDEGATE_HOME");
    assert_eq!(s, CaptureStatus::SetUpByExtension);
}

// ---- python3 detection -----------------------------------------------------------------------

#[test]
fn python_detection_runs_without_a_shell() {
    assert!(!python3_available_with("claudegate-no-such-program-xyz", Duration::from_secs(2)));
    // A shell would expand this; exec looks for a program literally named so.
    assert!(!python3_available_with("python3; true", Duration::from_secs(2)));
}

#[test]
fn python_detection_finds_python3_here() {
    assert!(python3_available_with("python3", Duration::from_secs(5)));
}

#[test]
fn python_detection_times_out() {
    let t = tempfile::tempdir().unwrap();
    let slow = t.path().join("slow-python");
    fs::write(&slow, "#!/bin/sh\nsleep 10\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&slow, fs::Permissions::from_mode(0o755)).unwrap();
    let start = Instant::now();
    assert!(!python3_available_with(slow.to_str().unwrap(), Duration::from_millis(300)));
    assert!(start.elapsed() < Duration::from_secs(3), "{:?}", start.elapsed());
}

// ---- set up ----------------------------------------------------------------------------------

#[test]
fn set_up_from_not_set_up_reports_backup_and_new_status() {
    let f = fx();
    f.write_settings("{\n  \"model\": \"opus\"\n}\n");
    let v = set_up_capture_with(&f.home, &f.cfg, true).unwrap();
    assert_eq!(v.capture.status, "app");
    assert_eq!(v.capture.version.as_deref(), Some(bundled().as_str()));
    assert_eq!(v.version, bundled());
    assert!(v.note.is_none());
    let backup = v.backup.expect("a settings file existed, so a backup is kept");
    assert!(Path::new(&backup).is_file(), "{backup}");
    assert!(backup.contains("settings.json.claudegate-"), "{backup}");
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), BUNDLED_HOOK);
    assert!(f.cg("hook.sh").is_file());
    assert_eq!(fs::read_to_string(f.cg(".installed-by-desktop")).unwrap(), bundled());
    let s = fs::read_to_string(f.settings()).unwrap();
    assert!(s.contains("\"model\": \"opus\"") && s.contains(".claudegate/hook.sh"), "{s}");
}

#[test]
fn set_up_is_refused_unless_not_set_up() {
    // Set up by VS Code.
    let f = fx();
    f.as_extension();
    let before = fs::read(f.settings()).unwrap();
    let e = set_up_capture_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("already set up") && e.contains("VS Code"), "{e}");
    assert_eq!(fs::read(f.settings()).unwrap(), before);

    // Set up by this app.
    let g = fx();
    g.as_app(&bundled());
    let e = set_up_capture_with(&g.home, &g.cfg, true).unwrap_err();
    assert!(e.contains("already set up") && e.contains("this app"), "{e}");

    // Settings unreadable.
    let h = fx();
    h.write_settings("{ // no\n}");
    let e = set_up_capture_with(&h.home, &h.cfg, true).unwrap_err();
    assert!(e.contains("settings.json"), "{e}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), "{ // no\n}");
    assert!(!h.cg("hook.py").exists());

    // python3 missing.
    let p = fx();
    let e = set_up_capture_with(&p.home, &p.cfg, false).unwrap_err();
    assert!(e.contains("Python 3"), "{e}");
    assert!(!p.cg("hook.py").exists() && !p.settings().exists());
}

#[test]
fn set_up_is_blocked_by_read_only_and_by_an_unreadable_config() {
    let f = fx();
    save_read_only(&f.cfg, true, None).unwrap();
    let e = set_up_capture_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("Read-only mode is on") && e.contains("set up capture"), "{e}");
    assert!(!f.settings().exists() && !f.cg("hook.py").exists());

    let g = fx();
    fs::create_dir_all(g.cfg.parent().unwrap()).unwrap();
    fs::write(&g.cfg, "{ nope").unwrap();
    let e = set_up_capture_with(&g.home, &g.cfg, true).unwrap_err();
    assert!(e.contains("couldn't read its settings file"), "{e}");
    assert!(!g.settings().exists() && !g.cg("hook.py").exists());
}

#[test]
fn leftover_hook_files_get_clear_messages() {
    let f = fx();
    fs::create_dir_all(f.home.join(".claudegate")).unwrap();
    fs::write(f.cg("hook.py"), "x").unwrap();
    let e = set_up_capture_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("hook.py") && e.contains("already exist"), "{e}");
    assert_eq!(e, install_error_text(&InstallError::HookFilesExist));

    fs::write(f.cg(".installed-by-desktop"), "v1.0.0").unwrap();
    let e = set_up_capture_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("earlier setup in this app"), "{e}");
    assert_eq!(e, install_error_text(&InstallError::HookFilesFromApp));
}

#[test]
fn already_set_up_text_names_who() {
    assert!(install_error_text(&InstallError::AlreadySetUp(CaptureStatus::SetUpByExtension)).contains("VS Code"));
    let t = install_error_text(&InstallError::AlreadySetUp(CaptureStatus::SetUpByApp { version: None }));
    assert!(t.contains("this app") && !t.contains("SetUpByApp"), "{t}");
}

// ---- the startup update ----------------------------------------------------------------------

#[test]
fn version_compare_is_semver_ish() {
    assert!(version_newer("v1.16.1", "v1.16.0"));
    assert!(version_newer("v1.16.1", "v1.9.9"));
    assert!(version_newer("v2.0.0", "v1.99.99"));
    assert!(version_newer("1.16.1", "v1.16.0"));
    assert!(!version_newer("v1.16.1", "v1.16.1"));
    assert!(!version_newer("v1.16.0", "v1.16.1"));
    assert!(!version_newer("v1.16.1", "garbage"));
    assert!(!version_newer("v1.16", "v1.15.0"));
    assert!(!version_newer("", "v1.0.0"));
}

#[test]
fn update_rewrites_the_apps_older_hook_and_the_marker_only() {
    let f = fx();
    f.as_app("v1.0.0");
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    let settings = fs::read(f.settings()).unwrap();
    let sh = fs::read(f.cg("hook.sh")).unwrap();
    let r = update_hook_with(&f.home, &f.cfg, true).unwrap();
    assert_eq!(r, HookUpdate::Updated { from: "v1.0.0".into(), to: bundled() });
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), BUNDLED_HOOK);
    assert_eq!(fs::read_to_string(f.cg(".installed-by-desktop")).unwrap(), bundled());
    assert_eq!(fs::read(f.settings()).unwrap(), settings, "settings.json is never touched");
    assert_eq!(fs::read(f.cg("hook.sh")).unwrap(), sh);
    // Once done, it is done.
    assert!(matches!(update_hook_with(&f.home, &f.cfg, true).unwrap(), HookUpdate::Skipped(_)));
}

fn assert_untouched(f: &Fx, run: impl FnOnce() -> Result<HookUpdate, String>) {
    let snap = |p: PathBuf| fs::read(p).ok();
    let before = (snap(f.cg("hook.py")), snap(f.cg(".installed-by-desktop")), snap(f.settings()));
    let r = run();
    assert!(matches!(r, Ok(HookUpdate::Skipped(_))), "{r:?}");
    let after = (snap(f.cg("hook.py")), snap(f.cg(".installed-by-desktop")), snap(f.settings()));
    assert_eq!(before, after);
}

#[test]
fn update_skips_when_not_newer() {
    let f = fx();
    f.as_app(&bundled());
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
    let g = fx();
    g.as_app("v99.0.0");
    assert_untouched(&g, || update_hook_with(&g.home, &g.cfg, true));
    let h = fx();
    h.as_app("not a version");
    assert_untouched(&h, || update_hook_with(&h.home, &h.cfg, true));
}

#[test]
fn update_never_touches_the_extensions_hook() {
    let f = fx();
    f.as_extension();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), "# the extension's hook, unstamped\n");
}

#[test]
fn update_refuses_without_the_marker_even_if_the_hook_is_stamped() {
    let f = fx();
    f.as_app("v1.0.0");
    fs::remove_file(f.cg(".installed-by-desktop")).unwrap();
    fs::write(f.cg("hook.py"), "# CLAUDEGATE_HOOK_VERSION=v1.0.0\n").unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

#[test]
fn update_skips_when_the_entry_is_gone() {
    // Marker left behind, settings entry removed: the status is NotSetUp, not ours to update.
    let f = fx();
    f.as_app("v1.0.0");
    f.write_settings("{}");
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

#[test]
fn update_skips_a_hook_the_extension_took_over() {
    // The marker says this app, but hook.py has no stamp: the extension rewrote it since.
    let f = fx();
    f.as_app("v1.0.0");
    fs::write(f.cg("hook.py"), "# the extension's hook, unstamped\n").unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

#[test]
fn update_skips_a_missing_hook_py() {
    let f = fx();
    f.as_app("v1.0.0");
    fs::remove_file(f.cg("hook.py")).unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
    assert!(!f.cg("hook.py").exists());
}

#[test]
fn update_is_blocked_by_read_only_and_an_unreadable_config() {
    let f = fx();
    f.as_app("v1.0.0");
    save_read_only(&f.cfg, true, None).unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));

    let g = fx();
    g.as_app("v1.0.0");
    fs::create_dir_all(g.cfg.parent().unwrap()).unwrap();
    fs::write(&g.cfg, "{ nope").unwrap();
    assert_untouched(&g, || update_hook_with(&g.home, &g.cfg, true));
}

#[test]
fn update_skips_when_python_is_missing() {
    let f = fx();
    f.as_app("v1.0.0");
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, false));
}

#[test]
fn update_keeps_hook_py_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = fx();
    f.as_app("v1.0.0");
    fs::write(f.cg("hook.py"), "# CLAUDEGATE_HOOK_VERSION=v1.0.0\n").unwrap();
    fs::set_permissions(f.cg("hook.py"), fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(update_hook_with(&f.home, &f.cfg, true).unwrap(), HookUpdate::Updated { .. }));
    assert_eq!(fs::metadata(f.cg("hook.py")).unwrap().permissions().mode() & 0o777, 0o640);
}
