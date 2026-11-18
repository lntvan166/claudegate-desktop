//! `roots_add` / `roots_remove` against a concurrent rewrite by the extension. Compiled as a unit
//! module of `store.rs` (`#[path]`) because the writers and the test seam are crate-private.

use crate::store::{roots_add, roots_remove, set_roots_race_hook};
use std::fs;
use std::path::{Path, PathBuf};

fn file(home: &Path) -> PathBuf {
    home.join("workspace-roots.json")
}

fn roots(home: &Path) -> Vec<String> {
    serde_json::from_str(&fs::read_to_string(file(home)).unwrap()).unwrap()
}

#[test]
fn a_rewrite_between_read_and_rename_is_merged_not_lost() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    fs::write(file(&home), r#"["/ws/a"]"#).unwrap();
    // The extension's window activates after our read and before our rename, and appends its
    // own root (its write is plain and unlocked).
    let h = home.clone();
    let mut fired = 0;
    set_roots_race_hook(Some(Box::new(move || {
        fired += 1;
        if fired == 1 {
            fs::write(file(&h), "[\n  \"/ws/a\",\n  \"/ws/vscode\"\n]").unwrap();
        }
    })));
    let r = roots_add(&home, Path::new("/ws/app"));
    set_roots_race_hook(None);
    r.unwrap();
    assert_eq!(roots(&home), vec!["/ws/a", "/ws/vscode", "/ws/app"]);
}

#[test]
fn remove_also_merges_a_concurrent_rewrite() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    fs::write(file(&home), r#"["/ws/a", "/ws/app"]"#).unwrap();
    let h = home.clone();
    let mut fired = 0;
    set_roots_race_hook(Some(Box::new(move || {
        fired += 1;
        if fired == 1 {
            fs::write(file(&h), r#"["/ws/a", "/ws/app", "/ws/vscode"]"#).unwrap();
        }
    })));
    let r = roots_remove(&home, Path::new("/ws/app"));
    set_roots_race_hook(None);
    r.unwrap();
    assert_eq!(roots(&home), vec!["/ws/a", "/ws/vscode"]);
}

#[test]
fn a_writer_that_never_settles_is_refused_rather_than_clobbered() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    fs::write(file(&home), "[]").unwrap();
    let h = home.clone();
    let mut n = 0;
    set_roots_race_hook(Some(Box::new(move || {
        n += 1;
        fs::write(file(&h), format!("[\"/ws/{n}\"]")).unwrap();
    })));
    let r = roots_add(&home, Path::new("/ws/app"));
    set_roots_race_hook(None);
    assert!(r.is_err());
    assert!(!roots(&home).contains(&"/ws/app".to_string()));
}

#[test]
fn only_the_roots_file_in_the_given_home_is_written() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    roots_add(&home, Path::new("/ws/app")).unwrap();
    let names: Vec<_> = fs::read_dir(&home).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("workspace-roots.json")], "no temp file left");
    // Appending twice never duplicates.
    roots_add(&home, Path::new("/ws/app/")).unwrap();
    assert_eq!(roots(&home), vec!["/ws/app"]);
    // Removing a root that is not there writes nothing.
    let before = fs::metadata(file(&home)).unwrap().modified().unwrap();
    roots_remove(&home, Path::new("/ws/other")).unwrap();
    assert_eq!(fs::metadata(file(&home)).unwrap().modified().unwrap(), before);
}

#[test]
fn the_nesting_guard_sees_a_root_written_between_read_and_rename() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    fs::write(file(&home), "[]").unwrap();
    let h = home.clone();
    let mut fired = 0;
    set_roots_race_hook(Some(Box::new(move || {
        fired += 1;
        if fired == 1 {
            fs::write(file(&h), r#"["/ws"]"#).unwrap();
        }
    })));
    let r = roots_add(&home, Path::new("/ws/app"));
    set_roots_race_hook(None);
    assert!(matches!(r, Err(crate::roots::RootsError::Nested { ref existing, .. }) if existing == "/ws"), "{r:?}");
    assert_eq!(roots(&home), vec!["/ws"]);
}
