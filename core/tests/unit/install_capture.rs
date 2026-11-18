//! Capture setup failure paths: write → verify → restore, and rollback that removes only what this
//! call created. Compiled as a unit-test module of `store.rs` (`#[path]`) so it can reach the
//! crate-private fault seam. Temp HOME only.

use crate::capture::{InstallError, HOOK_SH_CONTENT};
use crate::store::{install_capture, set_install_corrupt, set_install_fault};
use std::fs;
use std::io;
use std::path::PathBuf;

const HOOK: &str = "#!/usr/bin/env python3\n# CLAUDEGATE_HOOK_VERSION=v1.16.1\nprint('hook')\n";
const EXISTING: &str = "{\n    \"model\": \"opus\",\n    \"hooks\": { \"Stop\": [] }\n}\n";

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
    fn write_settings(&self, text: &str) {
        fs::create_dir_all(self.home.join(".claude")).unwrap();
        fs::write(self.settings(), text).unwrap();
    }
    fn backups(&self) -> Vec<PathBuf> {
        fs::read_dir(self.home.join(".claude"))
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("settings.json.claudegate-"))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Runs `install_capture` with `fault` called at each stage; clears the seam afterwards.
fn install_with(h: &Home, fault: impl FnMut(&str) -> io::Result<()> + 'static) -> Result<crate::capture::InstallReport, InstallError> {
    set_install_fault(Some(Box::new(fault)));
    let r = install_capture(&h.home, HOOK, true, false);
    set_install_fault(None);
    r
}

/// Runs `install_capture` with our settings bytes replaced by `bad` before they are written.
fn install_corrupted(h: &Home, bad: &'static str) -> Result<crate::capture::InstallReport, InstallError> {
    set_install_corrupt(Some(Box::new(move |b: &mut Vec<u8>| *b = bad.as_bytes().to_vec())));
    let r = install_capture(&h.home, HOOK, true, false);
    set_install_corrupt(None);
    r
}

#[test]
fn forced_verification_failure_restores_the_backup_byte_for_byte() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let dir = h.home.join(".claude");
    for d in 1..=7 {
        fs::write(dir.join(format!("settings.json.claudegate-2020-01-0{d}T00-00-00.000Z.bak")), "old").unwrap();
    }
    // Our own write lands wrong content: our entry is missing.
    let err = install_corrupted(&h, "{\"model\":\"opus\",\"hooks\":{}}").unwrap_err();

    assert!(matches!(err, InstallError::Failed { .. }), "{err:?}");
    assert!(err.to_string().contains("the claudegate hook entry is missing"), "{err}");
    assert_eq!(fs::read(h.settings()).unwrap(), EXISTING.as_bytes(), "restored byte for byte");
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(!h.cg(f).exists(), "{f} left behind");
    }
    assert!(!h.home.join(".claudegate").exists(), "the folder this call created is gone");
    let mut backups = h.backups();
    backups.sort();
    assert_eq!(backups.len(), 8, "the new backup stays, and a failed setup prunes nothing");
    assert_eq!(fs::read(backups.last().unwrap()).unwrap(), EXISTING.as_bytes());
}

#[test]
fn verification_failure_on_a_fresh_install_removes_the_settings_it_created() {
    let h = Home::new();
    let err = install_corrupted(&h, "[]").unwrap_err();
    assert!(matches!(err, InstallError::Failed { rollback: None, .. }), "{err:?}");
    assert!(!h.settings().exists());
    assert!(!h.home.join(".claude").exists(), "the empty folder this call created is gone");
    assert!(!h.home.join(".claudegate").exists());
}

#[test]
fn a_foreign_write_right_after_ours_is_never_restored_over() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let theirs = "{\"model\":\"sonnet\",\"hooks\":{\"Stop\":[]}}";
    let err = install_with(&h, move |stage| {
        if stage == "after_settings_write" {
            fs::write(&target, theirs).unwrap(); // Claude Code rewrites the file, dropping our entry
        }
        Ok(())
    })
    .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("was changed by another program right after setup; it was left as is"), "{msg}");
    assert!(msg.contains("Your pre-setup copy is"), "{msg}");
    assert!(msg.contains("capture is not set up"), "{msg}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), theirs, "their write survives untouched");
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(!h.cg(f).exists(), "{f}: the entry is gone, so the files this call made go too");
    }
    assert_eq!(h.backups().len(), 1);
}

#[test]
fn a_foreign_write_that_keeps_our_entry_is_a_successful_setup() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let t2 = target.clone();
    let report = install_with(&h, move |stage| {
        if stage == "after_settings_write" {
            let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&t2).unwrap()).unwrap();
            v["model"] = "sonnet".into();
            fs::write(&t2, serde_json::to_string(&v).unwrap()).unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert!(report.note.as_deref().is_some_and(|n| n.contains("left as is")), "{report:?}");
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
    assert_eq!(v["model"], "sonnet", "their change kept");
    assert!(h.cg("hook.py").exists() && h.cg("hook.sh").exists() && h.cg(".installed-by-desktop").exists());
}

#[test]
fn a_change_just_before_our_write_is_refused_and_kept() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let err = install_with(&h, move |stage| {
        if stage == "before_write" {
            fs::write(&target, "{\"model\":\"changed\"}").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { rollback: None, .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), "{\"model\":\"changed\"}");
    assert!(!h.cg("hook.py").exists());
}

#[test]
fn rollback_never_removes_files_it_did_not_create() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate/sessions")).unwrap();
    fs::write(h.cg("hook.sh"), HOOK_SH_CONTENT).unwrap(); // the extension's wrapper, identical
    fs::write(h.cg("sessions/a.json"), "{}").unwrap();

    let err = install_with(&h, |stage| {
        if stage == "after_hooks" {
            return Err(io::Error::other("injected"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { .. }), "{err:?}");

    assert_eq!(fs::read_to_string(h.cg("hook.sh")).unwrap(), HOOK_SH_CONTENT, "pre-existing hook.sh kept");
    assert!(h.cg("sessions/a.json").exists());
    assert!(!h.cg("hook.py").exists(), "the hook.py this call wrote is removed");
    assert!(!h.cg(".installed-by-desktop").exists());
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING);
    assert!(h.backups().is_empty(), "failed before the backup");
}

#[test]
fn a_hook_py_written_by_someone_else_mid_install_is_never_deleted() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let py = h.cg("hook.py");
    let err = install_with(&h, move |stage| {
        if stage == "before_hook_py" {
            fs::write(&py, "the extension's hook").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::HookFilesExist | InstallError::Failed { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "the extension's hook");
    assert!(!h.cg("hook.sh").exists());
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING);
}

#[test]
fn a_hook_py_appearing_after_ours_is_not_ours_to_delete() {
    // We wrote hook.py, then someone replaced it; rollback still removes only bytes we wrote.
    let h = Home::new();
    h.write_settings(EXISTING);
    let py = h.cg("hook.py");
    let err = install_with(&h, move |stage| {
        if stage == "after_hooks" {
            fs::write(&py, "replaced by the extension").unwrap();
            return Err(io::Error::other("injected"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "replaced by the extension");
}

#[test]
fn a_stale_marker_is_restored_on_rollback() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg(".installed-by-desktop"), "v0.0.1").unwrap();
    let err = install_with(&h, |stage| {
        if stage == "after_hooks" {
            return Err(io::Error::other("injected"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg(".installed-by-desktop")).unwrap(), "v0.0.1");
}

#[test]
fn settings_changed_during_install_is_refused_and_kept() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let err = install_with(&h, move |stage| {
        if stage == "before_backup" {
            fs::write(&target, "{\"model\":\"changed by Claude Code\"}").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), "{\"model\":\"changed by Claude Code\"}");
    assert!(!h.cg("hook.py").exists());
    assert!(h.backups().is_empty());
}

#[test]
fn guards_refuse_paths_outside_their_files() {
    use crate::store::{guard_hook_file, guard_settings_target};
    let h = Home::new();
    let cg = h.home.join(".claudegate");
    assert!(guard_hook_file(&cg, &cg.join("hook.py")).is_ok());
    assert!(guard_hook_file(&cg, &cg.join("hook.sh")).is_ok());
    assert!(guard_hook_file(&cg, &cg.join(".installed-by-desktop")).is_ok());
    assert!(guard_hook_file(&cg, &cg.join("sessions/x.json")).is_err());
    assert!(guard_hook_file(&cg, &cg.join("excludes.json")).is_err());
    assert!(guard_hook_file(&cg, &h.home.join("hook.py")).is_err());

    assert!(guard_settings_target(&h.home, &h.settings()).is_ok(), "absent: the literal path");
    assert!(guard_settings_target(&h.home, &h.home.join(".claude/settings.local.json")).is_err());
    assert!(guard_settings_target(&h.home, &h.home.join("settings.json")).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let real = h.home.join("real.json");
        fs::write(&real, "{}").unwrap();
        fs::create_dir_all(h.home.join(".claude")).unwrap();
        symlink(&real, h.settings()).unwrap();
        assert!(guard_settings_target(&h.home, &fs::canonicalize(&real).unwrap()).is_ok());
        assert!(guard_settings_target(&h.home, &h.settings()).is_err(), "never the link itself");
    }
}
