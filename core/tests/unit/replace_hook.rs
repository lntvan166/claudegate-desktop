//! "Update hook": `replace_hook` in a temp HOME, with a temp desktop home for the backups.
//! Compiled as a unit-test module of `store.rs` (`#[path]`) so it can reach the fault seam.

use crate::capture::{HookReplaceError, HookVersion, CaptureStatus, HOOK_SH_CONTENT};
use crate::store::{replace_hook, set_install_fault};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const HOOK: &str = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.17.0\nprint('new hook')\n";
const AUDIT: &str = "#!/usr/bin/env python3\nprint('audit')\n";
const EXT_HOOK: &str = "#!/usr/bin/env python3\nprint('the extension hook, older')\n";
const MARKER: &str = "v1.16.1";

struct Home {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    desk: PathBuf,
}

impl Home {
    fn new() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let desk = tmp.path().join("desk");
        fs::create_dir_all(home.join(".claudegate")).unwrap();
        fs::create_dir_all(home.join(".claude")).unwrap();
        Home { _tmp: tmp, home, desk }
    }
    fn cg(&self, f: &str) -> PathBuf {
        self.home.join(".claudegate").join(f)
    }
    fn settings(&self) -> PathBuf {
        self.home.join(".claude/settings.json")
    }
    /// The extension's setup: its unstamped hook.py, its wrapper and its settings entry.
    fn as_extension(&self) -> String {
        fs::write(self.cg("hook.py"), EXT_HOOK).unwrap();
        fs::set_permissions(self.cg("hook.py"), fs::Permissions::from_mode(0o640)).unwrap();
        fs::write(self.cg("hook.sh"), HOOK_SH_CONTENT).unwrap();
        let s = format!(
            "{{\n  \"model\": \"x\",\n  \"hooks\": {{\"PreToolUse\": [{{\"matcher\": \"x\", \"hooks\": [{{\"type\": \"command\", \"command\": \"{}\"}}]}}]}}\n}}\n",
            self.cg("hook.sh").display()
        );
        fs::write(self.settings(), &s).unwrap();
        s
    }
    fn backups(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(self.desk.join("backups/hook"))
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }
    fn run(&self) -> Result<crate::capture::HookReplaced, HookReplaceError> {
        replace_hook(&self.home, &self.desk, HOOK, AUDIT, false)
    }
}

#[test]
fn updates_the_extensions_unstamped_hook_and_nothing_else() {
    let h = Home::new();
    let settings = h.as_extension();
    fs::write(h.cg(".installed-by-desktop"), MARKER).unwrap();

    let r = h.run().unwrap();

    assert_eq!(r.from, HookVersion::Different { installed: None });
    assert_eq!(r.to, "v1.17.0");
    assert!(r.app_owns_now, "marker beside the extension's hook: stamped again, the app's now");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK, "the stamped bundled hook");
    let mode = fs::metadata(h.cg("hook.py")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640, "mode kept");
    let backup = r.backup.expect("a backup");
    assert_eq!(backup.parent().unwrap(), h.desk.join("backups/hook"));
    let name = backup.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("hook-") && name.ends_with(".py"), "{name}");
    assert_eq!(fs::read_to_string(&backup).unwrap(), EXT_HOOK);
    assert_eq!(fs::metadata(&backup).unwrap().permissions().mode() & 0o777, 0o640, "backup mode kept");
    // Never touched:
    assert_eq!(fs::read_to_string(h.cg("hook.sh")).unwrap(), HOOK_SH_CONTENT);
    assert_eq!(fs::read_to_string(h.cg(".installed-by-desktop")).unwrap(), MARKER);
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), settings);
    assert!(!h.cg("hook.py.bak").exists());
    let names: Vec<String> = fs::read_dir(h.home.join(".claudegate")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
}

#[test]
fn updates_an_older_stamped_hook() {
    let h = Home::new();
    h.as_extension();
    fs::write(h.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\nold\n").unwrap();
    let r = h.run().unwrap();
    assert_eq!(r.from, HookVersion::Older { installed: "v1.16.1".into() });
    assert!(!r.app_owns_now, "no marker");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
}

#[test]
fn a_missing_hook_py_is_created_without_a_backup() {
    let h = Home::new();
    h.as_extension();
    fs::remove_file(h.cg("hook.py")).unwrap();
    let r = h.run().unwrap();
    assert_eq!(r.from, HookVersion::Missing);
    assert_eq!(r.backup, None);
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
    assert!(h.backups().is_empty());
}

#[test]
fn a_change_between_read_and_write_refuses() {
    let h = Home::new();
    h.as_extension();
    let py = h.cg("hook.py");
    set_install_fault(Some(Box::new(move |stage| {
        if stage == "before_hook_replace" {
            fs::write(&py, "the extension synced in between\n").unwrap();
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    assert_eq!(err, HookReplaceError::Changed);
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "the extension synced in between\n");
}

#[test]
fn a_hook_py_turned_into_a_symlink_between_read_and_write_refuses() {
    let h = Home::new();
    h.as_extension();
    let elsewhere = h.home.join("elsewhere.py");
    fs::write(&elsewhere, EXT_HOOK).unwrap();
    let py = h.cg("hook.py");
    let target = elsewhere.clone();
    set_install_fault(Some(Box::new(move |stage| {
        if stage == "before_hook_replace" {
            fs::remove_file(&py).unwrap();
            std::os::unix::fs::symlink(&target, &py).unwrap();
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    assert_eq!(err, HookReplaceError::Changed);
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), EXT_HOOK, "the link's target is not written");
}

#[test]
fn a_hook_py_appearing_before_the_create_refuses() {
    let h = Home::new();
    h.as_extension();
    fs::remove_file(h.cg("hook.py")).unwrap();
    let py = h.cg("hook.py");
    set_install_fault(Some(Box::new(move |stage| {
        if stage == "before_hook_replace" {
            fs::write(&py, "someone else's\n").unwrap();
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    assert_eq!(err, HookReplaceError::Changed);
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "someone else's\n");
}

#[test]
fn a_symlinked_hook_py_refuses() {
    let h = Home::new();
    h.as_extension();
    let real = h.home.join("real-hook.py");
    fs::write(&real, EXT_HOOK).unwrap();
    fs::remove_file(h.cg("hook.py")).unwrap();
    std::os::unix::fs::symlink(&real, h.cg("hook.py")).unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::NotRegularFile);
    assert_eq!(fs::read_to_string(&real).unwrap(), EXT_HOOK);
    assert!(fs::symlink_metadata(h.cg("hook.py")).unwrap().file_type().is_symlink());
    assert!(h.backups().is_empty());
}

#[test]
fn a_folder_named_hook_py_refuses() {
    let h = Home::new();
    h.as_extension();
    fs::remove_file(h.cg("hook.py")).unwrap();
    fs::create_dir(h.cg("hook.py")).unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::NotRegularFile);
}

#[test]
fn read_only_refuses() {
    let h = Home::new();
    h.as_extension();
    assert_eq!(replace_hook(&h.home, &h.desk, HOOK, AUDIT, true).unwrap_err(), HookReplaceError::ReadOnly);
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), EXT_HOOK);
    assert!(h.backups().is_empty());
}

#[test]
fn not_registered_refuses() {
    let h = Home::new();
    h.as_extension();
    fs::write(h.settings(), "{\"hooks\": {\"PreToolUse\": []}}").unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::NotRegistered(CaptureStatus::NotSetUp));
    fs::remove_file(h.settings()).unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::NotRegistered(CaptureStatus::NotSetUp));
    fs::write(h.settings(), "{ not json").unwrap();
    assert!(matches!(h.run().unwrap_err(), HookReplaceError::NotRegistered(CaptureStatus::SettingsUnreadable(_))));
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), EXT_HOOK);
    assert!(h.backups().is_empty());
}

#[test]
fn an_up_to_date_hook_is_not_rewritten() {
    let h = Home::new();
    h.as_extension();
    fs::write(h.cg("hook.py"), HOOK).unwrap();
    fs::write(h.cg("audit.py"), AUDIT).unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::UpToDate);
    // The extension's copy at the same tag (the stamp line removed) is up to date too.
    fs::write(h.cg("hook.py"), "#!/usr/bin/env python3\nprint('new hook')\n").unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::UpToDate);
    assert!(h.backups().is_empty());
}

#[test]
fn an_unstamped_bundled_hook_refuses() {
    let h = Home::new();
    h.as_extension();
    assert_eq!(replace_hook(&h.home, &h.desk, "print('x')\n", AUDIT, false).unwrap_err(), HookReplaceError::BundledHook);
}

#[test]
fn a_failed_backup_refuses_the_write() {
    let h = Home::new();
    h.as_extension();
    fs::create_dir_all(&h.desk).unwrap();
    fs::write(h.desk.join("backups"), "a file where the folder goes").unwrap();
    assert!(matches!(h.run().unwrap_err(), HookReplaceError::Failed(r) if r.contains("back up")));
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), EXT_HOOK);
}

#[test]
fn a_backup_folder_inside_claudegate_refuses() {
    let h = Home::new();
    h.as_extension();
    let inside = h.home.join(".claudegate/desk");
    assert!(matches!(replace_hook(&h.home, &inside, HOOK, AUDIT, false).unwrap_err(), HookReplaceError::Failed(_)));
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), EXT_HOOK);
}

#[test]
fn keeps_the_newest_ten_backups() {
    let h = Home::new();
    h.as_extension();
    let dir = h.desk.join("backups/hook");
    fs::create_dir_all(&dir).unwrap();
    for d in 1..=12 {
        fs::write(dir.join(format!("hook-200001{d:02}T000000.000Z.py")), "old").unwrap();
    }
    fs::write(dir.join("notes.txt"), "not a backup").unwrap();
    let r = h.run().unwrap();
    let names: Vec<String> = h.backups().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("hook-")).count(), 10, "{names:?}");
    assert!(names.contains(&"notes.txt".to_owned()));
    assert!(r.backup.unwrap().exists(), "the new backup is among the kept");
    assert!(!names.contains(&"hook-20000101T000000.000Z.py".to_owned()));
    assert!(!names.contains(&"hook-20000103T000000.000Z.py".to_owned()));
    assert!(names.contains(&"hook-20000104T000000.000Z.py".to_owned()));
}

#[test]
fn an_injected_fault_before_the_write_leaves_hook_py_alone() {
    let h = Home::new();
    h.as_extension();
    set_install_fault(Some(Box::new(|stage| {
        if stage == "before_hook_replace" {
            return Err(io::Error::other("boom"));
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    assert!(matches!(err, HookReplaceError::Failed(_)), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), EXT_HOOK);
}

#[test]
fn a_newer_stamped_hook_is_never_downgraded() {
    let h = Home::new();
    h.as_extension();
    let newer = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v9.0.0\nnewer\n";
    fs::write(h.cg("hook.py"), newer).unwrap();
    assert_eq!(h.run().unwrap_err(), HookReplaceError::Newer("v9.0.0".into()));
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), newer);
    assert!(h.backups().is_empty());
}

#[test]
fn a_modified_copy_of_the_bundled_version_is_replaced() {
    let h = Home::new();
    h.as_extension();
    fs::write(h.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.17.0\nhand edited\n").unwrap();
    let r = h.run().unwrap();
    assert_eq!(r.from, HookVersion::Different { installed: Some("v1.17.0".into()) });
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
}

#[test]
fn an_app_owned_older_hook_is_not_reported_as_taken_back() {
    let h = Home::new();
    h.as_extension();
    fs::write(h.cg(".installed-by-desktop"), MARKER).unwrap();
    fs::write(h.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\nold\n").unwrap();
    assert!(!h.run().unwrap().app_owns_now);
}

fn ten_old_backups(h: &Home) {
    let dir = h.desk.join("backups/hook");
    fs::create_dir_all(&dir).unwrap();
    for d in 1..=10 {
        fs::write(dir.join(format!("hook-200001{d:02}T000000.000Z.py")), "old").unwrap();
    }
}

#[test]
fn a_refused_write_removes_the_backup_it_made() {
    let h = Home::new();
    h.as_extension();
    ten_old_backups(&h);
    let before = h.backups();
    let py = h.cg("hook.py");
    set_install_fault(Some(Box::new(move |stage| {
        if stage == "before_hook_replace" {
            fs::write(&py, "changed\n").unwrap();
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    assert_eq!(err, HookReplaceError::Changed);
    assert_eq!(h.backups(), before, "still the same ten");
}

#[test]
fn a_failed_write_removes_the_backup_it_made() {
    let h = Home::new();
    h.as_extension();
    ten_old_backups(&h);
    let before = h.backups();
    set_install_fault(Some(Box::new(|stage| {
        if stage == "before_hook_replace" {
            return Err(io::Error::other("boom"));
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    assert!(matches!(err, HookReplaceError::Failed(_)), "{err:?}");
    assert_eq!(h.backups(), before, "still the same ten");
}

// ---- audit.py, refreshed alongside ----

#[test]
fn a_stale_audit_py_is_replaced_with_a_backup_and_its_mode_kept() {
    let h = Home::new();
    let settings = h.as_extension();
    fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
    fs::set_permissions(h.cg("audit.py"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(h.cg("audit.sh"), crate::capture::AUDIT_SH_CONTENT).unwrap();

    h.run().unwrap();

    assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), AUDIT);
    assert_eq!(fs::metadata(h.cg("audit.py")).unwrap().permissions().mode() & 0o777, 0o600, "mode kept");
    let audit_backups: Vec<PathBuf> = h
        .backups()
        .into_iter()
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("audit-"))
        .collect();
    assert_eq!(audit_backups.len(), 1);
    assert_eq!(fs::read_to_string(&audit_backups[0]).unwrap(), "print('older audit')\n");
    assert_eq!(fs::read_to_string(h.cg("audit.sh")).unwrap(), crate::capture::AUDIT_SH_CONTENT, "never written");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), settings, "never written");
}

#[test]
fn a_missing_audit_py_is_created_and_a_current_one_left_alone() {
    let h = Home::new();
    h.as_extension();
    h.run().unwrap();
    assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), AUDIT);
    assert!(!h.cg("audit.sh").exists(), "the wrapper is setup's, not Update hook's");
    assert_eq!(h.backups().len(), 1, "only hook.py's");

    let g = Home::new();
    g.as_extension();
    fs::write(g.cg("audit.py"), AUDIT).unwrap();
    let before = fs::metadata(g.cg("audit.py")).unwrap().modified().unwrap();
    g.run().unwrap();
    assert_eq!(fs::metadata(g.cg("audit.py")).unwrap().modified().unwrap(), before, "not rewritten");
    assert_eq!(g.backups().len(), 1);
}

#[test]
fn a_symlinked_audit_py_is_left_alone() {
    let h = Home::new();
    h.as_extension();
    let elsewhere = h.home.join("mine.py");
    fs::write(&elsewhere, "mine\n").unwrap();
    std::os::unix::fs::symlink(&elsewhere, h.cg("audit.py")).unwrap();
    h.run().unwrap();
    assert!(fs::symlink_metadata(h.cg("audit.py")).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "mine\n");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK, "hook.py still updated");
}

#[test]
fn a_failed_hook_py_write_puts_audit_py_back() {
    for stale in [true, false] {
        let h = Home::new();
        h.as_extension();
        if stale {
            fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
        }
        let py = h.cg("hook.py");
        set_install_fault(Some(Box::new(move |stage| {
            if stage == "after_audit_replace" {
                fs::write(&py, "the extension synced in between\n").unwrap();
            }
            Ok(())
        })));
        let err = h.run().unwrap_err();
        set_install_fault(None);
        assert_eq!(err, HookReplaceError::Changed);
        if stale {
            assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), "print('older audit')\n", "put back");
        } else {
            assert!(!h.cg("audit.py").exists(), "the one it created is gone");
        }
        assert!(h.backups().is_empty(), "the backups it made go too");
    }
}

#[test]
fn the_startup_update_puts_audit_py_back_when_hook_py_changed_under_it() {
    for stale in [true, false] {
        let h = Home::new();
        h.as_extension();
        fs::write(h.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
        if stale {
            fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
        }
        let py = h.cg("hook.py");
        set_install_fault(Some(Box::new(move |stage| {
            if stage == "after_audit_update" {
                fs::write(&py, "the extension synced in between\n").unwrap();
            }
            Ok(())
        })));
        let r = crate::store::update_app_hook(&h.home, HOOK, AUDIT, true, false);
        set_install_fault(None);
        assert!(r.is_err(), "{r:?}");
        assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "the extension synced in between\n");
        if stale {
            assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), "print('older audit')\n", "put back");
        } else {
            assert!(!h.cg("audit.py").exists(), "the one it created is gone");
        }
    }
}

#[test]
fn the_startup_update_refreshes_audit_py_with_hook_py() {
    let h = Home::new();
    let settings = h.as_extension();
    fs::write(h.cg("hook.py"), "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.0.0\nold\n").unwrap();
    fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
    let r = crate::store::update_app_hook(&h.home, HOOK, AUDIT, true, false).unwrap();
    assert!(matches!(r, crate::capture::HookUpdate::Updated { .. }), "{r:?}");
    assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), AUDIT);
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
    assert!(!h.cg("audit.sh").exists());
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), settings);
}

#[test]
fn a_current_hook_py_with_a_stale_or_missing_audit_py_refreshes_audit_py_only() {
    for stale in [true, false] {
        let h = Home::new();
        let settings = h.as_extension();
        fs::write(h.cg("hook.py"), HOOK).unwrap();
        if stale {
            fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
        }
        let hook_before = fs::metadata(h.cg("hook.py")).unwrap().modified().unwrap();
        let r = h.run().unwrap();
        assert_eq!(r.from, HookVersion::UpToDate);
        assert!(r.audit_refreshed);
        assert_eq!(r.backup, None, "hook.py was not replaced, so not backed up");
        assert!(!r.app_owns_now);
        assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), AUDIT);
        assert_eq!(fs::metadata(h.cg("hook.py")).unwrap().modified().unwrap(), hook_before, "hook.py untouched");
        assert_eq!(fs::read_to_string(h.settings()).unwrap(), settings);
        match (stale, &r.audit_backup) {
            (true, Some(b)) => assert_eq!(fs::read_to_string(b).unwrap(), "print('older audit')\n"),
            (false, None) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(h.run().unwrap_err(), HookReplaceError::UpToDate, "then both are current");
    }
}

#[test]
fn a_failed_put_back_of_audit_py_is_reported() {
    let h = Home::new();
    h.as_extension();
    fs::write(h.cg("audit.py"), "print('older audit')\n").unwrap();
    let (py, audit) = (h.cg("hook.py"), h.cg("audit.py"));
    set_install_fault(Some(Box::new(move |stage| {
        if stage == "after_audit_replace" {
            fs::write(&audit, "someone else's audit.py\n").unwrap();
            fs::write(&py, "the extension synced in between\n").unwrap();
        }
        Ok(())
    })));
    let err = h.run().unwrap_err();
    set_install_fault(None);
    let HookReplaceError::Failed(msg) = &err else { panic!("{err:?}") };
    assert!(msg.contains("audit.py was updated but could not be put back"), "{msg}");
    assert_eq!(fs::read_to_string(h.cg("audit.py")).unwrap(), "someone else's audit.py\n", "theirs is left");
}
