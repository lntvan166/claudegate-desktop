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
fn a_foreign_write_with_a_differently_spelled_entry_keeps_the_files() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let theirs = r#"{"hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"$HOME/.claudegate/hook.sh"}]}]}}"#;
    let report = install_with(&h, move |stage| {
        if stage == "after_settings_write" {
            fs::write(&target, theirs).unwrap();
        }
        Ok(())
    })
    .unwrap();
    assert!(report.note.as_deref().is_some_and(|n| n.contains("left as is")), "{report:?}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), theirs);
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(h.cg(f).exists(), "{f}: an entry is registered, so its files stay");
    }
}

#[test]
fn a_read_error_after_our_write_keeps_the_files_and_says_so() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let report = install_with(&h, move |stage| {
        if stage == "after_settings_write" {
            fs::remove_file(&target).unwrap();
            fs::create_dir(&target).unwrap(); // reading it now fails
        }
        Ok(())
    })
    .unwrap();
    let note = report.note.unwrap_or_default();
    assert!(note.contains("could not read it back to check it"), "{note}");
    assert!(!note.contains("another program"), "{note}");
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(h.cg(f).exists(), "{f}");
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_home_does_not_trip_the_guard_on_rollback() {
    use std::os::unix::fs::symlink;
    let h = Home::new();
    let link = h.home.parent().unwrap().join("linkhome");
    symlink(&h.home, &link).unwrap();
    set_install_corrupt(Some(Box::new(|b: &mut Vec<u8>| *b = b"[]".to_vec())));
    let r = install_capture(&link, HOOK, true, false);
    set_install_corrupt(None);
    let err = r.unwrap_err();
    assert!(matches!(err, InstallError::Failed { rollback: None, .. }), "{err:?}");
    assert!(!h.settings().exists(), "the fresh file this call wrote was removed");
    assert!(!h.home.join(".claudegate").exists());
}

#[test]
fn a_failed_restore_keeps_the_files_while_the_entry_is_registered() {
    // (a) Our write drops a top-level key but carries our entry; verify fails, and the backup is
    // gone by the time Undo needs it, so the file keeps our entry: the hook stays.
    let h = Home::new();
    h.write_settings(EXISTING);
    set_install_corrupt(Some(Box::new(|b: &mut Vec<u8>| {
        let mut v: serde_json::Value = serde_json::from_slice(b).unwrap();
        v.as_object_mut().unwrap().remove("model");
        *b = serde_json::to_vec_pretty(&v).unwrap();
    })));
    let dir = h.home.join(".claude");
    let err = install_with(&h, move |stage| {
        if stage == "after_settings_write" {
            for e in fs::read_dir(&dir).unwrap().flatten() {
                if e.file_name().to_string_lossy().ends_with(".bak") {
                    fs::remove_file(e.path()).unwrap();
                }
            }
        }
        Ok(())
    })
    .unwrap_err();
    set_install_corrupt(None);
    let InstallError::Failed { reason, rollback } = &err else { panic!("{err:?}") };
    assert!(reason.contains("top-level key \"model\" was lost"), "{reason}");
    let rollback = rollback.as_deref().unwrap_or_default();
    assert!(rollback.contains("could not read the backup"), "{rollback}");
    assert!(rollback.contains("the hook files and marker were kept"), "{rollback}");
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(h.cg(f).exists(), "{f}");
    }
}

#[test]
fn a_foreign_rewrite_before_undo_that_keeps_an_entry_keeps_the_files() {
    // (b) Verify fails on our bytes; before Undo runs, another program rewrites the file with an
    // entry. Undo leaves the file alone, and the hook stays.
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let theirs = r#"{"hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"command":"$HOME/.claudegate/hook.sh"}]}]}}"#;
    set_install_corrupt(Some(Box::new(|b: &mut Vec<u8>| *b = b"{\"model\":\"opus\"}".to_vec())));
    let err = install_with(&h, move |stage| {
        if stage == "before_undo" {
            fs::write(&target, theirs).unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    set_install_corrupt(None);
    let InstallError::Failed { rollback, .. } = &err else { panic!("{err:?}") };
    let rollback = rollback.as_deref().unwrap_or_default();
    assert!(rollback.contains("was changed by another program right after setup; it was left as is"), "{rollback}");
    assert!(rollback.contains("the hook files and marker were kept"), "{rollback}");
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), theirs);
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(h.cg(f).exists(), "{f}");
    }
}

#[test]
fn settings_deleted_after_our_write_is_not_set_up() {
    let h = Home::new();
    h.write_settings(EXISTING);
    let target = h.settings();
    let err = install_with(&h, move |stage| {
        if stage == "after_settings_write" {
            fs::remove_file(&target).unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    let InstallError::Failed { reason, rollback } = &err else { panic!("{err:?}") };
    assert!(reason.contains("removed by another program"), "{reason}");
    assert_eq!(rollback, &None);
    assert!(!h.settings().exists(), "not recreated");
    for f in ["hook.py", "hook.sh", ".installed-by-desktop"] {
        assert!(!h.cg(f).exists(), "{f}");
    }
    assert_eq!(h.backups().len(), 1);
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

/// `HOOK` without its stamp line: the extension's hook.py at the pinned tag.
const UNSTAMPED: &str = "#!/usr/bin/env python3\nprint('hook')\n";

#[test]
fn a_failure_after_adopting_restores_the_leftover_hook_py() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), UNSTAMPED).unwrap();
    for stage in ["after_hooks", "before_write", "after_settings_write"] {
        let err = install_with(&h, move |s| if s == stage { Err(io::Error::other("injected")) } else { Ok(()) }).unwrap_err();
        assert!(matches!(err, InstallError::Failed { rollback: None, .. }), "{stage}: {err:?}");
        assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), UNSTAMPED, "{stage}: the leftover is back, not deleted");
        assert!(!h.cg("hook.sh").exists(), "{stage}");
        assert!(!h.cg(".installed-by-desktop").exists(), "{stage}");
        assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING, "{stage}");
    }
}

#[test]
fn an_adopted_hook_py_rewritten_before_rollback_is_left_alone() {
    // Rollback restores the leftover only while hook.py still holds the bytes we wrote.
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), UNSTAMPED).unwrap();
    let py = h.cg("hook.py");
    let err = install_with(&h, move |stage| {
        if stage == "after_hooks" {
            fs::write(&py, "the extension reinstalled").unwrap();
            return Err(io::Error::other("injected"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "the extension reinstalled");
}

#[test]
fn a_leftover_that_changes_before_the_replace_is_refused() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), UNSTAMPED).unwrap();
    let py = h.cg("hook.py");
    let err = install_with(&h, move |stage| {
        if stage == "before_hook_py" {
            fs::write(&py, "changed mid-install").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::HookFilesExist), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), "changed mid-install");
    assert!(!h.cg("hook.sh").exists());
    assert!(!h.cg(".installed-by-desktop").exists());
    assert_eq!(fs::read_to_string(h.settings()).unwrap(), EXISTING);
}

#[test]
fn a_hook_py_that_turns_into_the_bundled_one_mid_install_is_refused() {
    // Byte-compare just before the write: any change refuses, even to an adoptable form.
    let h = Home::new();
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg("hook.py"), UNSTAMPED).unwrap();
    let py = h.cg("hook.py");
    let err = install_with(&h, move |stage| {
        if stage == "before_hook_py" {
            fs::write(&py, HOOK).unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::HookFilesExist), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg("hook.py")).unwrap(), HOOK);
    assert!(!h.settings().exists());
}

#[test]
fn a_marker_rewritten_by_someone_else_before_rollback_is_left_alone() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg(".installed-by-desktop"), "v0.0.1").unwrap();
    let marker = h.cg(".installed-by-desktop");
    let err = install_with(&h, move |stage| {
        if stage == "after_hooks" {
            fs::write(&marker, "v9.9.9").unwrap();
            return Err(io::Error::other("injected"));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Failed { rollback: None, .. }), "{err:?}");
    assert_eq!(fs::read_to_string(h.cg(".installed-by-desktop")).unwrap(), "v9.9.9");
}

#[test]
fn a_replaced_file_that_cannot_be_read_at_rollback_is_a_rollback_problem() {
    let h = Home::new();
    h.write_settings(EXISTING);
    fs::create_dir_all(h.home.join(".claudegate")).unwrap();
    fs::write(h.cg(".installed-by-desktop"), "v0.0.1").unwrap();
    let marker = h.cg(".installed-by-desktop");
    let err = install_with(&h, move |stage| {
        if stage == "after_hooks" {
            fs::remove_file(&marker).unwrap();
            fs::create_dir(&marker).unwrap(); // stat works, read fails
            return Err(io::Error::other("injected"));
        }
        Ok(())
    })
    .unwrap_err();
    match err {
        InstallError::Failed { rollback: Some(r), .. } => assert!(r.contains(".installed-by-desktop"), "{r}"),
        other => panic!("{other:?}"),
    }
    assert!(h.cg(".installed-by-desktop").is_dir(), "left as found");
}
