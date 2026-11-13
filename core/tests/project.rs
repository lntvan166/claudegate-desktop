use claudegate_core::paths::session_file_for;
use claudegate_core::project::{list_projects, load_project};
use claudegate_core::textfile::{read_text, TextFile};
use std::fs;
use std::path::Path;

fn write_session(home: &Path, root: &Path, json: &str) {
    let f = session_file_for(home, root);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(f, json).unwrap();
}

fn entry(orig: Option<&str>) -> serde_json::Value {
    serde_json::json!({"originalContent": orig, "reviewStatus":"pending","sessionId":"s","capturedAt":"2026-01-01T00:00:00Z"})
}

fn rec(path: &str, at: &str) -> serde_json::Value {
    serde_json::json!({"id": format!("{at}::{path}"), "path": path, "before":"a\n","after":"a\nb\n","decidedAt":at,"sessionId":"s"})
}

fn session(files: serde_json::Value, accepted: serde_json::Value, rejected: serde_json::Value) -> String {
    serde_json::json!({"sessionId":"x","status":"active","files":files,"accepted":accepted,"rejected":rejected}).to_string()
}

#[test]
fn pending_real_change_counts_and_unchanged_hidden() {
    let h = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    let root = w.path();
    let same = root.join("same.txt");
    let chg = root.join("chg.txt");
    fs::write(&same, "x\n").unwrap();
    fs::write(&chg, "a\nB\nc\nd\n").unwrap();
    let mut files = serde_json::Map::new();
    files.insert(same.to_string_lossy().into(), entry(Some("x\n")));
    files.insert(chg.to_string_lossy().into(), entry(Some("a\nb\nc\n")));
    write_session(h.path(), root, &session(files.into(), serde_json::json!([]), serde_json::json!({})));
    let v = load_project(h.path(), root);
    assert_eq!(v.pending.len(), 1);
    assert_eq!(v.pending[0].rel, "chg.txt");
    assert_eq!((v.pending[0].added, v.pending[0].removed), (2, 1));
    assert!(!v.missing);
}

#[test]
fn excluded_hidden() {
    let h = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(w.path()).unwrap();
    fs::create_dir_all(root.join("gen")).unwrap();
    let f = root.join("gen/a.txt");
    fs::write(&f, "new").unwrap();
    let mut files = serde_json::Map::new();
    files.insert(f.to_string_lossy().into(), entry(Some("old")));
    write_session(h.path(), &root, &session(files.into(), serde_json::json!([]), serde_json::json!({})));
    fs::write(
        h.path().join("excludes.json"),
        serde_json::json!({ root.to_string_lossy(): ["gen"] }).to_string(),
    )
    .unwrap();
    assert!(load_project(h.path(), &root).pending.is_empty());
}

#[test]
fn worktree_scope_sorted_and_error_isolated() {
    let h = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    let root = w.path();
    let wt = root.join("wt");
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), "gitdir: /x/.git/worktrees/wt\n").unwrap();
    let a = root.join("z.txt");
    let b = wt.join("a.txt");
    fs::write(&a, "1").unwrap();
    fs::write(&b, "1").unwrap();
    let mut f1 = serde_json::Map::new();
    f1.insert(a.to_string_lossy().into(), entry(Some("0")));
    let mut f2 = serde_json::Map::new();
    f2.insert(b.to_string_lossy().into(), entry(Some("0")));
    let acc = serde_json::json!([rec("/p/old", "2026-01-01"), rec("/p/new", "2026-02-01")]);
    let rej = serde_json::json!({"/p/z": rec("/p/z", "2026-01-01"), "/p/b": rec("/p/b", "2026-01-01")});
    write_session(h.path(), root, &session(f1.into(), acc, rej));
    write_session(h.path(), &wt, &session(f2.into(), serde_json::json!([]), serde_json::json!({})));
    let v = load_project(h.path(), root);
    assert_eq!(v.scopes.len(), 2);
    assert!(v.scopes[1].is_worktree);
    assert_eq!(v.pending.len(), 2);
    assert!(v.pending[0].path < v.pending[1].path);
    assert!(v.pending.iter().any(|p| p.scope == wt.to_string_lossy()));
    assert_eq!(v.accepted[0].path, "/p/new");
    assert_eq!(v.rejected[0].path, "/p/b");
    assert_eq!((v.accepted[0].added, v.accepted[0].removed), (1, 0));

    write_session(h.path(), &wt, "{");
    let v = load_project(h.path(), root);
    assert!(v.scopes[1].error.is_some());
    assert!(v.scopes[0].error.is_none());
    assert_eq!(v.pending.len(), 1);
}

#[test]
fn missing_root_and_list_projects() {
    let h = tempfile::tempdir().unwrap();
    let w = tempfile::tempdir().unwrap();
    let gone = w.path().join("gone");
    let v = load_project(h.path(), &gone);
    assert!(v.missing && v.pending.is_empty() && v.scopes.is_empty());

    let root = fs::canonicalize(w.path()).unwrap();
    let f = root.join("a.txt");
    fs::write(&f, "n").unwrap();
    let mut files = serde_json::Map::new();
    files.insert(f.to_string_lossy().into(), entry(Some("o")));
    write_session(h.path(), &root, &session(files.into(), serde_json::json!([]), serde_json::json!({})));
    fs::write(
        h.path().join("workspace-roots.json"),
        serde_json::json!([root.to_string_lossy(), gone.to_string_lossy()]).to_string(),
    )
    .unwrap();
    let l = list_projects(h.path());
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].pending, 1);
    assert_eq!(l[0].name, root.file_name().unwrap().to_string_lossy());
    assert!(l[1].missing);
}

#[test]
fn read_text_kinds() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(read_text(&t.path().join("no")), TextFile::Missing);
    fs::write(t.path().join("b"), [0xffu8, 0xfe, 0x00]).unwrap();
    assert_eq!(read_text(&t.path().join("b")), TextFile::Binary);
    fs::write(t.path().join("t"), "hé").unwrap();
    assert_eq!(read_text(&t.path().join("t")), TextFile::Text("hé".into()));
}
