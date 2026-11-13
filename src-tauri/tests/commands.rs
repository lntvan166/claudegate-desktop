use claudegate_core::paths::session_file_for;
use claudegate_desktop_lib::commands::{
    diff_for, get_project_with, list_projects_with, DiffKind, TextSide,
};
use claudegate_desktop_lib::watch::KnownMap;
use std::fs;
use std::path::Path;

fn text(s: &str) -> TextSide {
    TextSide::Text { text: s.into() }
}

struct Fx {
    home: tempfile::TempDir,
    ws: tempfile::TempDir,
}

fn fixture() -> Fx {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let root = ws.path();
    let f = root.join("a.txt");
    fs::write(&f, "new\n").unwrap();
    let path = f.to_string_lossy().into_owned();
    let created = root.join("created.txt");
    fs::write(&created, "hi\n").unwrap();
    let session = serde_json::json!({
        "sessionId": "s", "status": "active",
        "files": {
            path.clone(): {"originalContent": "old\n", "reviewStatus": "pending", "sessionId": "s"},
            created.to_string_lossy(): {"originalContent": null, "reviewStatus": "pending", "sessionId": "s"},
        },
        "accepted": [{"id": "2026-01-01T00:00:00Z::/x/acc.txt", "path": "/x/acc.txt",
                      "before": null, "after": "acc\n", "decidedAt": "2026-01-01T00:00:00Z"}],
        "rejected": {"/x/rej.txt": {"id": "r1", "path": "/x/rej.txt", "before": "b\n", "after": "a\n",
                                    "decidedAt": "2026-01-02T00:00:00Z"}}
    });
    let sf = session_file_for(home.path(), root);
    fs::create_dir_all(sf.parent().unwrap()).unwrap();
    fs::write(&sf, session.to_string()).unwrap();
    fs::write(home.path().join("workspace-roots.json"), serde_json::json!([root]).to_string()).unwrap();
    Fx { home, ws }
}

#[test]
fn pending_diff_uses_original_and_disk() {
    let fx = fixture();
    let scope = fx.ws.path().to_string_lossy().into_owned();
    let key = fx.ws.path().join("a.txt").to_string_lossy().into_owned();
    let d = diff_for(fx.home.path(), &scope, DiffKind::Pending, &key).unwrap();
    assert_eq!(d.before, text("old\n"));
    assert_eq!(d.after, text("new\n"));
    let key = fx.ws.path().join("created.txt").to_string_lossy().into_owned();
    let d = diff_for(fx.home.path(), &scope, DiffKind::Pending, &key).unwrap();
    assert_eq!(d.before, TextSide::Missing);
    assert_eq!(d.after, text("hi\n"));
}

#[test]
fn accepted_and_rejected_use_record_sides() {
    let fx = fixture();
    let scope = fx.ws.path().to_string_lossy().into_owned();
    let d = diff_for(fx.home.path(), &scope, DiffKind::Accepted, "2026-01-01T00:00:00Z::/x/acc.txt").unwrap();
    assert_eq!((d.before, d.after, d.path.as_str()), (TextSide::Missing, text("acc\n"), "/x/acc.txt"));
    let d = diff_for(fx.home.path(), &scope, DiffKind::Rejected, "/x/rej.txt").unwrap();
    assert_eq!((d.before, d.after), (text("b\n"), text("a\n")));
}

#[test]
fn unknown_key_and_scope_are_not_found() {
    let fx = fixture();
    let scope = fx.ws.path().to_string_lossy().into_owned();
    for k in [DiffKind::Pending, DiffKind::Accepted, DiffKind::Rejected] {
        assert_eq!(diff_for(fx.home.path(), &scope, k, "nope").unwrap_err(), "not found");
    }
    assert_eq!(
        diff_for(fx.home.path(), "/no/such/scope", DiffKind::Pending, "x").unwrap_err(),
        "not found"
    );
}

#[test]
fn text_side_serializes_as_contract() {
    assert_eq!(serde_json::to_string(&TextSide::Missing).unwrap(), r#"{"kind":"missing"}"#);
    assert_eq!(serde_json::to_string(&text("x")).unwrap(), r#"{"kind":"text","text":"x"}"#);
    let k: DiffKind = serde_json::from_str("\"pending\"").unwrap();
    assert_eq!(k, DiffKind::Pending);
}

#[test]
fn listing_populates_known_map_and_never_writes() {
    let fx = fixture();
    let known = KnownMap::default();
    let before: Vec<_> = walk(fx.home.path());
    let list = list_projects_with(fx.home.path(), &known);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pending, 2);
    let k = known.lock().unwrap().clone();
    assert_eq!(k.len(), 1);
    assert_eq!(k[0].1, session_file_for(fx.home.path(), fx.ws.path()));
    let v = get_project_with(fx.home.path(), &known, &list[0].root);
    assert_eq!((v.pending.len(), v.accepted.len(), v.rejected.len()), (2, 1, 1));
    assert_eq!(walk(fx.home.path()), before, "home must be untouched");
}

fn walk(p: &Path) -> Vec<(String, u64)> {
    let mut v = vec![];
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(d).unwrap().flatten() {
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                v.push((e.path().to_string_lossy().into_owned(), m.len()));
            }
        }
    }
    v.sort();
    v
}
