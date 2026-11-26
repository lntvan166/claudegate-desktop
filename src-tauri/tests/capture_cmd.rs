//! Capture status, setup and the startup hook update through the app's functions, in a temp HOME
//! with a temp desktop config. The real ~/.claude, ~/.claudegate and ~/.claudegate-desktop are
//! never touched.
use claudegate_core::capture::{bundled_hook_version, version_newer, AuditRepair, CaptureStatus, HookUpdate, InstallError};
use claudegate_desktop_lib::capture::{
    capture_view_for, install_error_text, pick_python3, python3_available_with, read_status, set_up_capture_with, status_key,
    repair_audit_with, update_capture_hook_with, update_hook_with, CaptureView, HookView, BUNDLED_AUDIT, BUNDLED_HOOK,
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
    /// The installed hook.py, stamped older than the bundled one, so only the guard under test
    /// stands between it and an update.
    fn old_hook(&self) {
        fs::write(self.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    }
}

fn bundled() -> String {
    bundled_hook_version(BUNDLED_HOOK).expect("the bundled hook is stamped")
}

// ---- status ----------------------------------------------------------------------------------

#[test]
fn status_keys_cover_every_variant() {
    assert_eq!(status_key(&CaptureStatus::SetUpByExtension { python_missing: false }), "vscode");
    assert_eq!(status_key(&CaptureStatus::SetUpByApp { version: None, python_missing: true }), "app");
    assert_eq!(status_key(&CaptureStatus::NotSetUp), "not-set-up");
    assert_eq!(status_key(&CaptureStatus::SettingsUnreadable("x".into())), "settings-unreadable");
    assert_eq!(status_key(&CaptureStatus::PythonMissing), "python-missing");
}

#[test]
fn not_set_up_when_settings_absent_or_without_entry() {
    let f = fx();
    let v = capture_view_for(&f.home, true);
    assert_eq!(v, CaptureView { status: "not-set-up".into(), version: None, note: None, python_missing: false, taken_over: false, setup_supported: cfg!(unix), hook: None });
    f.write_settings(r#"{"model":"x","hooks":{"PreToolUse":[]}}"#);
    assert_eq!(capture_view_for(&f.home, true).status, "not-set-up");
}

#[test]
fn vscode_when_entry_without_marker() {
    let f = fx();
    f.as_extension();
    assert_eq!(read_status(&f.home, true), CaptureStatus::SetUpByExtension { python_missing: false });
    let v = capture_view_for(&f.home, true);
    assert_eq!((v.status.as_str(), v.taken_over), ("vscode", false));
}

#[test]
fn app_with_trimmed_marker_version() {
    let f = fx();
    f.as_app("v1.2.3\n");
    assert_eq!(
        read_status(&f.home, true),
        CaptureStatus::SetUpByApp { version: Some("v1.2.3".into()), python_missing: false }
    );
    let v = capture_view_for(&f.home, true);
    assert_eq!(
        v,
        CaptureView {
            status: "app".into(),
            version: Some("v1.2.3".into()),
            note: None,
            python_missing: false,
            taken_over: false,
            setup_supported: cfg!(unix),
            hook: Some(HookView { state: "upToDate".into(), installed: Some(bundled()), bundled: bundled(), audit_stale: false }),
        }
    );
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
fn python_missing_blocks_only_when_not_set_up() {
    let f = fx();
    assert_eq!(capture_view_for(&f.home, false).status, "python-missing");
    f.as_extension();
    let v = capture_view_for(&f.home, false);
    assert_eq!((v.status.as_str(), v.python_missing), ("vscode", true));
    let g = fx();
    g.as_app("v1.16.1");
    let v = capture_view_for(&g.home, false);
    assert_eq!((v.status.as_str(), v.python_missing), ("app", true));
    // Set up refuses either way: already set up.
    let e = set_up_capture_with(&g.home, &g.cfg, false).unwrap_err();
    assert!(e.contains("already set up"), "{e}");
}

#[test]
fn marker_with_an_unstamped_hook_is_the_extensions() {
    // The app set up, then the extension replaced hook.py (it carries no stamp).
    let f = fx();
    f.as_app("v1.16.1");
    fs::write(f.cg("hook.py"), "# the extension's hook, unstamped\n").unwrap();
    assert_eq!(read_status(&f.home, true), CaptureStatus::SetUpByExtension { python_missing: false });
    let v = capture_view_for(&f.home, true);
    assert_eq!((v.status.as_str(), v.taken_over), ("vscode", true));
    let e = set_up_capture_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("VS Code"), "{e}");
}

#[test]
fn marker_without_hook_py_stays_the_apps() {
    let f = fx();
    f.as_app("v1.16.1");
    fs::remove_file(f.cg("hook.py")).unwrap();
    assert_eq!(capture_view_for(&f.home, true).status, "app");
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
    assert_eq!(s, CaptureStatus::SetUpByExtension { python_missing: false });
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
    assert_eq!(fs::read_to_string(f.cg("audit.py")).unwrap(), BUNDLED_AUDIT);
    assert!(f.cg("audit.sh").is_file());
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
    assert!(install_error_text(&InstallError::AlreadySetUp(CaptureStatus::SetUpByExtension { python_missing: false })).contains("VS Code"));
    let t = install_error_text(&InstallError::AlreadySetUp(CaptureStatus::SetUpByApp { version: None, python_missing: false }));
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
    let audit_sh = fs::read(f.cg("audit.sh")).unwrap();
    fs::write(f.cg("audit.py"), "print('older audit')\n").unwrap();
    let r = update_hook_with(&f.home, &f.cfg, true).unwrap();
    assert_eq!(r, HookUpdate::Updated { from: "v1.0.0".into(), to: bundled() });
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), BUNDLED_HOOK);
    assert_eq!(fs::read_to_string(f.cg(".installed-by-desktop")).unwrap(), bundled());
    assert_eq!(fs::read(f.settings()).unwrap(), settings, "settings.json is never touched");
    assert_eq!(fs::read(f.cg("hook.sh")).unwrap(), sh);
    assert_eq!(fs::read_to_string(f.cg("audit.py")).unwrap(), BUNDLED_AUDIT, "audit.py refreshed alongside");
    assert_eq!(fs::read(f.cg("audit.sh")).unwrap(), audit_sh, "the audit wrapper is never the update's to write");
    // Once done, it is done.
    assert!(matches!(update_hook_with(&f.home, &f.cfg, true).unwrap(), HookUpdate::Skipped(_)));
}

fn assert_untouched(f: &Fx, run: impl FnOnce() -> Result<HookUpdate, String>) {
    let snap = |p: PathBuf| fs::read(p).ok();
    let before = (snap(f.cg("hook.py")), snap(f.cg("audit.py")), snap(f.cg(".installed-by-desktop")), snap(f.settings()));
    let r = run();
    assert!(matches!(r, Ok(HookUpdate::Skipped(_))), "{r:?}");
    let after = (snap(f.cg("hook.py")), snap(f.cg("audit.py")), snap(f.cg(".installed-by-desktop")), snap(f.settings()));
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
    // The installed hook's own stamp decides: newer or unparsable is never replaced.
    for stamp in ["v99.0.0", "not a version"] {
        let k = fx();
        k.as_app("v1.0.0");
        fs::write(k.cg("hook.py"), format!("#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION={stamp}\n")).unwrap();
        assert_untouched(&k, || update_hook_with(&k.home, &k.cfg, true));
    }
}

#[test]
fn update_never_touches_the_extensions_hook() {
    let f = fx();
    f.as_extension();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), "# the extension's hook, unstamped\n");
}

#[test]
fn update_rewrites_a_stamped_hook_with_no_marker() {
    // The extension registered the entry; Update hook stamped hook.py; there is no marker. The
    // stamp alone makes it this app's to keep current at startup.
    let f = fx();
    f.as_extension();
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    let settings = fs::read(f.settings()).unwrap();
    let r = update_hook_with(&f.home, &f.cfg, true).unwrap();
    assert_eq!(r, HookUpdate::Updated { from: "v1.0.0".into(), to: bundled() });
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), BUNDLED_HOOK);
    assert!(!f.cg(".installed-by-desktop").exists(), "the marker is never created");
    assert_eq!(fs::read(f.settings()).unwrap(), settings);
    assert_eq!(capture_view_for(&f.home, true).status, "vscode", "still registered by the extension");
}

#[test]
fn update_never_downgrades_a_newer_stamped_hook_with_no_marker() {
    let f = fx();
    f.as_extension();
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v99.0.0\nnewer\n").unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

#[test]
fn update_skips_a_stamped_hook_when_nothing_is_registered() {
    let f = fx();
    fs::create_dir_all(f.home.join(".claudegate")).unwrap();
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
    f.write_settings("{}");
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

#[test]
fn update_skips_a_symlinked_stamped_hook() {
    let f = fx();
    f.as_extension();
    let real = f.home.join("real-hook.py");
    fs::write(&real, "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    fs::remove_file(f.cg("hook.py")).unwrap();
    std::os::unix::fs::symlink(&real, f.cg("hook.py")).unwrap();
    let r = update_hook_with(&f.home, &f.cfg, true);
    assert!(matches!(r, Ok(HookUpdate::Skipped(_))), "{r:?}");
    assert!(fs::read_to_string(&real).unwrap().contains("v1.0.0"));
    assert!(fs::symlink_metadata(f.cg("hook.py")).unwrap().file_type().is_symlink());
}

#[test]
fn update_skips_when_the_entry_is_gone() {
    // Marker left behind, settings entry removed: the status is NotSetUp, not ours to update.
    let f = fx();
    f.as_app("v1.0.0");
    f.old_hook();
    f.write_settings("{}");
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

#[test]
fn update_compares_with_the_installed_stamp_not_the_marker() {
    // Marker says old, but hook.py is already the bundled version: nothing to do.
    let f = fx();
    f.as_app("v1.0.0");
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
    // Marker says current, but hook.py is stamped older: update it.
    let g = fx();
    g.as_app(&bundled());
    fs::write(g.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.2.0\n").unwrap();
    let r = update_hook_with(&g.home, &g.cfg, true).unwrap();
    assert_eq!(r, HookUpdate::Updated { from: "v1.2.0".into(), to: bundled() });
    assert_eq!(fs::read_to_string(g.cg("hook.py")).unwrap(), BUNDLED_HOOK);
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
    f.old_hook();
    save_read_only(&f.cfg, true, None).unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));

    let g = fx();
    g.as_app("v1.0.0");
    g.old_hook();
    fs::create_dir_all(g.cfg.parent().unwrap()).unwrap();
    fs::write(&g.cfg, "{ nope").unwrap();
    assert_untouched(&g, || update_hook_with(&g.home, &g.cfg, true));
}

#[test]
fn update_skips_when_python_is_missing() {
    let f = fx();
    f.as_app("v1.0.0");
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n").unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, false));
}

#[test]
fn update_keeps_hook_py_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = fx();
    f.as_app("v1.0.0");
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\n").unwrap();
    fs::set_permissions(f.cg("hook.py"), fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(update_hook_with(&f.home, &f.cfg, true).unwrap(), HookUpdate::Updated { .. }));
    assert_eq!(fs::metadata(f.cg("hook.py")).unwrap().permissions().mode() & 0o777, 0o640);
}

// ---- python3 resolution ----------------------------------------------------------------------

fn exec_set(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
    move |p: &Path| paths.iter().any(|x| Path::new(x) == p)
}

#[test]
fn python3_is_found_on_path_then_in_the_fallback_dirs() {
    let path = std::ffi::OsString::from("/opt/py/bin:/usr/bin");
    let home = Path::new("/home/u");
    let found = pick_python3(Some(&path), Some(home), &exec_set(&["/opt/py/bin/python3", "/usr/bin/python3"]), false, &|| true);
    assert_eq!(found.as_deref(), Some(Path::new("/opt/py/bin/python3")));
    // A GUI PATH without it: the usual install dirs are searched after PATH.
    let bare = std::ffi::OsString::from("/usr/sbin");
    let found = pick_python3(Some(&bare), Some(home), &exec_set(&["/home/u/.local/bin/python3"]), false, &|| true);
    assert_eq!(found.as_deref(), Some(Path::new("/home/u/.local/bin/python3")));
    let found = pick_python3(Some(&bare), Some(home), &exec_set(&["/opt/homebrew/bin/python3"]), false, &|| true);
    assert_eq!(found.as_deref(), Some(Path::new("/opt/homebrew/bin/python3")));
    assert_eq!(pick_python3(None, None, &exec_set(&[]), false, &|| true), None);
}

#[test]
fn macos_usr_bin_python3_needs_the_command_line_tools() {
    let path = std::ffi::OsString::from("/usr/bin:/bin");
    let only_stub = exec_set(&["/usr/bin/python3"]);
    let asked = std::cell::Cell::new(0);
    let no_clt = || {
        asked.set(asked.get() + 1);
        false
    };
    assert_eq!(pick_python3(Some(&path), None, &only_stub, true, &no_clt), None, "the stub would open an install dialog");
    assert!(asked.get() >= 1);
    assert_eq!(
        pick_python3(Some(&path), None, &only_stub, true, &|| true).as_deref(),
        Some(Path::new("/usr/bin/python3"))
    );
    // Not macOS: /usr/bin/python3 is a plain python, and the check is never asked.
    let never = || panic!("xcode-select asked off macOS");
    assert_eq!(pick_python3(Some(&path), None, &only_stub, false, &never).as_deref(), Some(Path::new("/usr/bin/python3")));
    // A brew python later on the list is still found on macOS.
    let both = exec_set(&["/usr/bin/python3", "/opt/homebrew/bin/python3"]);
    assert_eq!(
        pick_python3(Some(&path), None, &both, true, &|| false).as_deref(),
        Some(Path::new("/opt/homebrew/bin/python3"))
    );
}

#[test]
fn the_view_says_whether_setup_is_supported_here() {
    let f = fx();
    assert_eq!(capture_view_for(&f.home, true).setup_supported, cfg!(unix));
    let json = serde_json::to_value(capture_view_for(&f.home, true)).unwrap();
    assert_eq!(json["setupSupported"], serde_json::Value::Bool(cfg!(unix)));
}

// ---- Update hook ------------------------------------------------------------------------------

fn hook_state(f: &Fx) -> Option<HookView> {
    capture_view_for(&f.home, true).hook
}

#[test]
fn the_view_reports_the_hooks_version_state() {
    let f = fx();
    assert_eq!(hook_state(&f), None, "not set up: no hook state");
    f.as_extension();
    assert_eq!(hook_state(&f), Some(HookView { state: "different".into(), installed: None, bundled: bundled(), audit_stale: true }));
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    assert_eq!(hook_state(&f), Some(HookView { state: "older".into(), installed: Some("v1.0.0".into()), bundled: bundled(), audit_stale: true }));
    fs::write(f.cg("hook.py"), BUNDLED_HOOK).unwrap();
    assert_eq!(hook_state(&f), Some(HookView { state: "upToDate".into(), installed: Some(bundled()), bundled: bundled(), audit_stale: true }));
    // The extension's copy at the bundled tag (stamp line removed) is up to date, and unstamped.
    let mut lines: Vec<&str> = BUNDLED_HOOK.split_inclusive('\n').collect();
    lines.remove(1);
    fs::write(f.cg("hook.py"), lines.concat()).unwrap();
    assert_eq!(hook_state(&f), Some(HookView { state: "upToDate".into(), installed: None, bundled: bundled(), audit_stale: true }));
    // audit.py is the other half: missing or different is stale, the bundled copy is not, a link is
    // left alone.
    fs::write(f.cg("audit.py"), BUNDLED_AUDIT).unwrap();
    assert!(!hook_state(&f).unwrap().audit_stale);
    fs::write(f.cg("audit.py"), "print('older audit')\n").unwrap();
    assert!(hook_state(&f).unwrap().audit_stale);
    fs::remove_file(f.cg("audit.py")).unwrap();
    std::os::unix::fs::symlink(f.home.join("elsewhere.py"), f.cg("audit.py")).unwrap();
    assert!(!hook_state(&f).unwrap().audit_stale);
    fs::remove_file(f.cg("audit.py")).unwrap();
    fs::write(f.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v99.0.0\nnewer\n").unwrap();
    assert_eq!(hook_state(&f), Some(HookView { state: "newer".into(), installed: Some("v99.0.0".into()), bundled: bundled(), audit_stale: true }));
    fs::remove_file(f.cg("hook.py")).unwrap();
    assert_eq!(hook_state(&f).unwrap().state, "missing");
    let json = serde_json::to_value(capture_view_for(&f.home, true)).unwrap();
    assert_eq!(json["hook"]["state"], "missing");
    assert_eq!(json["hook"]["bundled"], serde_json::Value::String(bundled()));
}

#[test]
fn update_hook_replaces_the_extensions_hook_and_backs_it_up_beside_the_config() {
    let f = fx();
    f.as_extension();
    fs::write(f.cg("hook.sh"), "#!/usr/bin/env bash\npython3 \"$HOME/.claudegate/hook.py\"\n").unwrap();
    let settings = fs::read(f.settings()).unwrap();
    let sh = fs::read(f.cg("hook.sh")).unwrap();
    let r = update_capture_hook_with(&f.home, &f.cfg, true).unwrap();
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), BUNDLED_HOOK);
    let backup = PathBuf::from(r.backup.expect("backed up"));
    assert_eq!(backup.parent().unwrap(), f.cfg.parent().unwrap().join("backups/hook"));
    assert_eq!(fs::read_to_string(&backup).unwrap(), "# the extension's hook, unstamped\n");
    assert_eq!(r.from, HookView { state: "different".into(), installed: None, bundled: bundled(), audit_stale: true });
    assert_eq!(r.to, bundled());
    assert_eq!(r.capture.status, "vscode", "no marker: still the extension's setup");
    assert_eq!(r.capture.hook.unwrap().state, "upToDate");
    assert_eq!(fs::read(f.settings()).unwrap(), settings);
    assert_eq!(fs::read(f.cg("hook.sh")).unwrap(), sh);
    assert!(!f.cg(".installed-by-desktop").exists(), "no marker written");
    assert!(!r.app_owns_now);
    // Done once: a second press refuses rather than backing up again.
    assert!(update_capture_hook_with(&f.home, &f.cfg, true).unwrap_err().contains("already"));
}

#[test]
fn update_hook_is_blocked_by_read_only_and_an_unreadable_config() {
    let f = fx();
    f.as_extension();
    save_read_only(&f.cfg, true, None).unwrap();
    let e = update_capture_hook_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("update the hook"), "{e}");
    fs::write(&f.cfg, "{ not json").unwrap();
    assert!(update_capture_hook_with(&f.home, &f.cfg, true).is_err());
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), "# the extension's hook, unstamped\n");
    assert!(!f.cfg.parent().unwrap().join("backups").exists());
}

#[test]
fn update_hook_refuses_when_capture_is_not_set_up() {
    let f = fx();
    fs::create_dir_all(f.home.join(".claudegate")).unwrap();
    fs::write(f.cg("hook.py"), "leftover\n").unwrap();
    let e = update_capture_hook_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("not set up"), "{e}");
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), "leftover\n");
}

#[test]
fn update_hook_says_when_it_takes_a_taken_over_hook_back() {
    let f = fx();
    f.as_extension();
    fs::write(f.cg(".installed-by-desktop"), "v1.0.0").unwrap();
    assert!(capture_view_for(&f.home, true).taken_over);
    let r = update_capture_hook_with(&f.home, &f.cfg, true).unwrap();
    assert!(r.app_owns_now);
    assert_eq!(r.capture.status, "app");
    assert_eq!(fs::read_to_string(f.cg(".installed-by-desktop")).unwrap(), "v1.0.0", "marker untouched");
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["appOwnsNow"], serde_json::Value::Bool(true));
}

#[test]
fn update_hook_refuses_a_newer_hook() {
    let f = fx();
    f.as_extension();
    let newer = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v99.0.0\nnewer\n";
    fs::write(f.cg("hook.py"), newer).unwrap();
    let e = update_capture_hook_with(&f.home, &f.cfg, true).unwrap_err();
    assert!(e.contains("newer"), "{e}");
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), newer);
}

#[test]
fn update_refreshes_a_stale_audit_py_only_alongside_hook_py() {
    let f = fx();
    f.as_app("v1.0.0");
    f.old_hook();
    fs::write(f.cg("audit.py"), "print('older audit')\n").unwrap();
    update_hook_with(&f.home, &f.cfg, true).unwrap();
    assert_eq!(fs::read_to_string(f.cg("audit.py")).unwrap(), BUNDLED_AUDIT);
    // hook.py current: nothing is written, a stale audit.py included.
    fs::write(f.cg("audit.py"), "print('older audit')\n").unwrap();
    assert_untouched(&f, || update_hook_with(&f.home, &f.cfg, true));
}

// ---- the startup repair of the audit registration ----

fn audit_entries(f: &Fx) -> Vec<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(f.settings()).unwrap()).unwrap();
    ["PostToolUse", "PostToolUseFailure"].iter().map(|e| v["hooks"][e].clone()).collect()
}

#[test]
fn repair_adds_the_audit_entries_to_an_extension_setup_and_creates_the_files_first() {
    let f = fx();
    f.as_extension();
    let pre_before = {
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(f.settings()).unwrap()).unwrap();
        v["hooks"]["PreToolUse"].clone()
    };
    assert!(!f.cg("audit.sh").exists() && !f.cg("audit.py").exists());
    let r = repair_audit_with(&f.home, &f.cfg, true).unwrap();
    let AuditRepair::Repaired { created, backup, note } = r else { panic!("{r:?}") };
    assert_eq!(created, vec![f.cg("audit.py"), f.cg("audit.sh")]);
    assert!(backup.is_file() && note.is_none());
    assert_eq!(fs::read_to_string(f.cg("audit.py")).unwrap(), BUNDLED_AUDIT);
    assert_eq!(fs::read_to_string(f.cg("audit.sh")).unwrap(), claudegate_core::capture::AUDIT_SH_CONTENT);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(fs::metadata(f.cg("audit.sh")).unwrap().permissions().mode() & 0o777, 0o755);
    let cmd = f.cg("audit.sh").display().to_string();
    let want = serde_json::json!([{ "matcher": "^Bash$", "hooks": [{ "type": "command", "command": cmd }] }]);
    assert_eq!(audit_entries(&f), vec![want.clone(), want]);
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(f.settings()).unwrap()).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"], pre_before, "the capture entry is never touched");
    assert_eq!(fs::read_to_string(f.cg("hook.py")).unwrap(), "# the extension's hook, unstamped\n", "hook.py untouched");
    assert!(!f.cg(".installed-by-desktop").exists());
    assert_eq!(capture_view_for(&f.home, true).status, "vscode");
    // A second launch finds nothing to do and writes nothing.
    let bytes = fs::read(f.settings()).unwrap();
    assert!(matches!(repair_audit_with(&f.home, &f.cfg, true).unwrap(), AuditRepair::Skipped(_)));
    assert_eq!(fs::read(f.settings()).unwrap(), bytes);
}

#[test]
fn repair_writes_nothing_for_a_correct_setup_or_none_at_all() {
    let f = fx();
    set_up_capture_with(&f.home, &f.cfg, true).unwrap();
    let snap = |f: &Fx| (fs::read(f.settings()).ok(), fs::read(f.cg("audit.sh")).ok(), fs::read(f.cg("audit.py")).ok());
    let before = snap(&f);
    assert!(matches!(repair_audit_with(&f.home, &f.cfg, true).unwrap(), AuditRepair::Skipped(_)));
    assert_eq!(snap(&f), before);
    // Not set up: repair only, never a first-time install.
    let g = fx();
    g.write_settings("{}");
    assert!(matches!(repair_audit_with(&g.home, &g.cfg, true).unwrap(), AuditRepair::Skipped(_)));
    assert_eq!(fs::read_to_string(g.settings()).unwrap(), "{}");
    assert!(!g.cg("audit.sh").exists());
}

#[test]
fn repair_is_blocked_by_read_only_and_a_missing_python() {
    let f = fx();
    f.as_extension();
    let before = fs::read(f.settings()).unwrap();
    assert!(matches!(repair_audit_with(&f.home, &f.cfg, false).unwrap(), AuditRepair::Skipped(_)));
    save_read_only(&f.cfg, true, None).unwrap();
    assert!(matches!(repair_audit_with(&f.home, &f.cfg, true).unwrap(), AuditRepair::Skipped(_)));
    assert_eq!(fs::read(f.settings()).unwrap(), before);
    assert!(!f.cg("audit.sh").exists() && !f.cg("audit.py").exists());
}

#[test]
fn repair_refuses_malformed_settings_and_writes_nothing() {
    for post in [r#""x""#, "{}", "false"] {
        let f = fx();
        f.as_extension();
        let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(f.settings()).unwrap()).unwrap();
        v["hooks"]["PostToolUseFailure"] = serde_json::from_str(post).unwrap();
        let text = serde_json::to_string(&v).unwrap();
        f.write_settings(&text);
        let err = repair_audit_with(&f.home, &f.cfg, true).unwrap_err();
        assert!(err.contains("can't be updated safely"), "{post}: {err}");
        assert_eq!(fs::read_to_string(f.settings()).unwrap(), text, "{post}");
        assert!(!f.cg("audit.sh").exists(), "{post}: no file written before the refusal");
    }
}

#[test]
fn repair_keeps_an_existing_wrapper_and_audit_py() {
    let f = fx();
    f.as_extension();
    fs::write(f.cg("audit.sh"), "#!/usr/bin/env bash\npython \"$HOME/.claudegate/audit.py\"\n").unwrap();
    fs::write(f.cg("audit.py"), "print('older audit')\n").unwrap();
    let r = repair_audit_with(&f.home, &f.cfg, true).unwrap();
    let AuditRepair::Repaired { created, .. } = r else { panic!("{r:?}") };
    assert!(created.is_empty());
    assert_eq!(fs::read_to_string(f.cg("audit.py")).unwrap(), "print('older audit')\n", "Update hook's job, not repair's");
}
