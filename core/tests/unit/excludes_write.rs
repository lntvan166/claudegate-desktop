//! `excludes_set` / `excludes_remove`: the app writes only its own key of the shared
//! `excludes.json`. Compiled as a unit module of `store.rs` (`#[path]`) because the writers and the
//! race seam are crate-private. Temp homes only.

use crate::store::{excludes_remove, excludes_set, set_roots_race_hook, EXCLUDES_FILE};
use std::fs;
use std::path::{Path, PathBuf};

fn file(home: &Path) -> PathBuf {
    home.join(EXCLUDES_FILE)
}

fn read(home: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(file(home)).unwrap()).unwrap()
}

fn globs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn no_tmp(home: &Path) {
    let left: Vec<_> = fs::read_dir(home)
        .unwrap()
        .flatten()
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "tmp left behind: {left:?}");
}

#[test]
fn set_writes_only_its_key_and_keeps_every_other_value() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    // A root that no longer exists, a non-list value and odd glob types: the extension would prune
    // or filter these; the app keeps every key it does not own exactly as it found it.
    let others = serde_json::json!({
        "/gone/root": ["**/a", 7, null],
        "/ws/vscode": ["**/b"],
        "/ws/weird": { "not": "a list" },
    });
    fs::write(file(home), serde_json::to_string_pretty(&others).unwrap()).unwrap();
    excludes_set(home, Path::new("/ws/app"), &globs(&["**/x", "**/y"])).unwrap();
    let v = read(home);
    for (k, val) in others.as_object().unwrap() {
        assert_eq!(&v[k], val, "{k} kept as is");
    }
    assert_eq!(v["/ws/app"], serde_json::json!(["**/x", "**/y"]));
    assert_eq!(v.as_object().unwrap().len(), 4);
    no_tmp(home);
}

#[test]
fn a_missing_file_starts_from_an_empty_object() {
    let t = tempfile::tempdir().unwrap();
    excludes_set(t.path(), Path::new("/ws/app"), &globs(&["**/x"])).unwrap();
    assert_eq!(read(t.path()), serde_json::json!({ "/ws/app": ["**/x"] }));
    // Two-space pretty JSON, like the extension's JSON.stringify(_, null, 2).
    assert!(fs::read_to_string(file(t.path())).unwrap().contains("\n  \"/ws/app\""));
}

#[test]
fn the_key_is_the_resolved_root() {
    let t = tempfile::tempdir().unwrap();
    excludes_set(t.path(), Path::new("/ws/./x/../app/"), &globs(&["**/x"])).unwrap();
    assert_eq!(read(t.path()), serde_json::json!({ "/ws/app": ["**/x"] }));
}

#[test]
fn malformed_or_non_object_files_are_refused_and_left_alone() {
    for raw in ["{ not json", "[\"/ws/a\"]", "\"text\"", "null"] {
        let t = tempfile::tempdir().unwrap();
        fs::write(file(t.path()), raw).unwrap();
        assert!(excludes_set(t.path(), Path::new("/ws/app"), &globs(&["**/x"])).is_err(), "{raw}");
        assert!(excludes_remove(t.path(), Path::new("/ws/app")).is_err(), "{raw}");
        assert_eq!(fs::read_to_string(file(t.path())).unwrap(), raw, "{raw} untouched");
        no_tmp(t.path());
    }
}

#[test]
fn an_unchanged_value_writes_nothing() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    // Not our formatting: a rewrite would change the bytes.
    let raw = r#"{"/ws/vscode":["**/b"],"/ws/app":["**/x"]}"#;
    fs::write(file(home), raw).unwrap();
    excludes_set(home, Path::new("/ws/app"), &globs(&["**/x"])).unwrap();
    assert_eq!(fs::read_to_string(file(home)).unwrap(), raw);
    // Removing a key that is not there writes nothing either.
    excludes_remove(home, Path::new("/ws/other")).unwrap();
    assert_eq!(fs::read_to_string(file(home)).unwrap(), raw);
    // And a missing file stays missing.
    let t2 = tempfile::tempdir().unwrap();
    excludes_remove(t2.path(), Path::new("/ws/app")).unwrap();
    assert!(!file(t2.path()).exists());
}

#[test]
fn remove_deletes_only_its_own_key() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    fs::write(file(home), r#"{"/ws/vscode":["**/b"],"/ws/app":["**/x"],"/ws/app-two":["**/z"]}"#).unwrap();
    excludes_remove(home, Path::new("/ws/app/")).unwrap();
    assert_eq!(read(home), serde_json::json!({ "/ws/vscode": ["**/b"], "/ws/app-two": ["**/z"] }));
}

#[test]
fn only_an_absolute_home_is_written() {
    assert!(excludes_set(Path::new(""), Path::new("/ws/app"), &globs(&["**/x"])).is_err());
    assert!(excludes_set(Path::new("rel/home"), Path::new("/ws/app"), &globs(&["**/x"])).is_err());
    assert!(!Path::new("rel/home").exists());
}

#[test]
fn a_rewrite_between_read_and_rename_is_merged_not_lost() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().to_path_buf();
    fs::write(file(&home), r#"{"/ws/a":["**/a"]}"#).unwrap();
    let h = home.clone();
    let mut fired = 0;
    set_roots_race_hook(Some(Box::new(move || {
        fired += 1;
        if fired == 1 {
            // Another window persists its own exclude after our read, before our rename.
            fs::write(file(&h), r#"{"/ws/a":["**/a"],"/ws/vscode":["**/v"]}"#).unwrap();
        }
    })));
    let r = excludes_set(&home, Path::new("/ws/app"), &globs(&["**/x"]));
    set_roots_race_hook(None);
    r.unwrap();
    assert_eq!(
        read(&home),
        serde_json::json!({ "/ws/a": ["**/a"], "/ws/vscode": ["**/v"], "/ws/app": ["**/x"] })
    );
}
