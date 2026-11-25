//! A registered root nested inside another one is shown as a scope of the outermost containing
//! root's project: its own session, its own exclude, decisions routed to its own session file.
//! Temp dirs only.

use claudegate_core::decide::{decide, Action, DecideError, DecidePaths, Target};
use claudegate_core::paths::session_file_for;
use claudegate_core::project::{
    fold_parent, list_projects, listed_roots, load_project, nested_roots_in, review_scopes_in, ScopeKind,
};
use claudegate_core::projects::ProjectRecord;
use claudegate_core::upkeep::{upkeep, UpkeepOpts};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const CAP: &str = "2026-11-16T10:00:00.000Z";
const NOW: &str = "2026-11-16T12:00:00.000Z";

struct Fx {
    _tmp: tempfile::TempDir,
    paths: DecidePaths,
    /// The umbrella root (`umbrella`).
    parent: PathBuf,
    /// A registered root inside it (`umbrella/umbrella-orders`).
    nested: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let home = base.join("cg");
        let desktop_home = base.join("cgd");
        let parent = base.join("umbrella");
        let nested = parent.join("umbrella-orders");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::create_dir_all(nested.join(".git")).unwrap();
        let fx = Fx { paths: DecidePaths { home, desktop_home }, parent, nested, _tmp: tmp };
        fx.roots(&[fx.parent.clone(), fx.nested.clone()]);
        fx
    }

    fn roots<P: AsRef<Path>>(&self, roots: &[P]) {
        let list: Vec<&Path> = roots.iter().map(|r| r.as_ref()).collect();
        fs::write(self.paths.home.join("workspace-roots.json"), json!(list).to_string()).unwrap();
    }

    fn file(&self, p: &Path, content: &str) -> String {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn session_file(&self, scope: &Path) -> PathBuf {
        session_file_for(&self.paths.home, scope)
    }

    fn write_session(&self, scope: &Path, files: Value) {
        let v = json!({ "sessionId": "x", "status": "active", "files": files, "accepted": [], "rejected": {} });
        fs::write(self.session_file(scope), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }

    fn session(&self, scope: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(self.session_file(scope)).unwrap()).unwrap()
    }

    fn excludes(&self, v: Value) {
        fs::write(self.paths.home.join("excludes.json"), v.to_string()).unwrap();
    }

    /// One real edit in each root's session: `(parent file, nested file)`.
    fn two_sessions(&self) -> (String, String) {
        let a = self.file(&self.parent.join("README.md"), "new\n");
        let b = self.file(&self.nested.join("src/x.go"), "new\n");
        self.write_session(&self.parent, json!({ &a: pending(Some("old\n")) }));
        self.write_session(&self.nested, json!({ &b: pending(Some("old\n")) }));
        (a, b)
    }
}

fn pending(orig: Option<&str>) -> Value {
    json!({ "originalContent": orig, "reviewStatus": "pending", "sessionId": "s1", "capturedAt": CAP })
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn nesting_matches_whole_segments_only() {
    let roots = vec![PathBuf::from("/a/foo"), PathBuf::from("/a/foobar"), PathBuf::from("/a/foo/bar"), PathBuf::from("/a/foo/./bar/")];
    assert_eq!(nested_roots_in(&roots, Path::new("/a/foo")), vec![PathBuf::from("/a/foo/bar")]);
    assert!(nested_roots_in(&roots, Path::new("/a/foobar")).is_empty());
    let all_dirs = |_: &Path| true;
    assert_eq!(fold_parent(&roots, Path::new("/a/foobar"), &all_dirs), None);
    assert_eq!(fold_parent(&roots, Path::new("/a/foo"), &all_dirs), None);
    assert_eq!(fold_parent(&roots, Path::new("/a/foo/bar"), &all_dirs), Some(PathBuf::from("/a/foo")));
}

#[test]
fn a_root_folds_into_the_outermost_existing_container() {
    let roots = vec![PathBuf::from("/a"), PathBuf::from("/a/b"), PathBuf::from("/a/b/c")];
    let all = |_: &Path| true;
    assert_eq!(fold_parent(&roots, Path::new("/a/b/c"), &all), Some(PathBuf::from("/a")));
    // The outermost is gone from disk: the next one in holds it, and with no container left the
    // root lists on its own.
    let no_a = |p: &Path| p != Path::new("/a");
    assert_eq!(fold_parent(&roots, Path::new("/a/b/c"), &no_a), Some(PathBuf::from("/a/b")));
    assert_eq!(fold_parent(&roots, Path::new("/a/b"), &no_a), None);
}

#[test]
fn the_nested_root_is_a_scope_of_its_parent_and_owns_its_worktrees() {
    let fx = Fx::new();
    // A worktree inside the nested repo, and one directly in the parent.
    for (dir, name) in [(&fx.nested, "wt-n"), (&fx.parent, "wt-p")] {
        let wt = dir.join(name);
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}/.git/worktrees/{name}\n", dir.display())).unwrap();
    }
    let roots = vec![fx.parent.clone(), fx.nested.clone()];
    let scopes = review_scopes_in(&roots, &fx.parent, 256);
    let got: Vec<(PathBuf, ScopeKind, PathBuf)> = scopes.into_iter().map(|s| (s.root, s.kind, s.owner)).collect();
    assert_eq!(
        got,
        vec![
            (fx.parent.clone(), ScopeKind::Primary, fx.parent.clone()),
            (fx.parent.join("wt-p"), ScopeKind::Worktree, fx.parent.clone()),
            (fx.nested.clone(), ScopeKind::Nested, fx.nested.clone()),
            (fx.nested.join("wt-n"), ScopeKind::Worktree, fx.nested.clone()),
        ]
    );
    // A nested root does not count against `worktrees.maxAttached`.
    let capped = review_scopes_in(&roots, &fx.parent, 0);
    assert_eq!(capped.iter().map(|s| s.kind).collect::<Vec<_>>(), vec![ScopeKind::Primary, ScopeKind::Nested]);
}

#[test]
fn the_view_shows_both_sessions_and_labels_the_nested_scope() {
    let fx = Fx::new();
    let (a, b) = fx.two_sessions();
    let v = load_project(&fx.paths.home, &fx.parent);
    let scopes: Vec<(String, bool, bool)> = v.scopes.iter().map(|s| (s.root.clone(), s.is_worktree, s.nested)).collect();
    assert_eq!(scopes, vec![(s(&fx.parent), false, false), (s(&fx.nested), false, true)]);
    let rows: Vec<(String, String)> = v.pending.iter().map(|p| (p.path.clone(), p.scope.clone())).collect();
    assert_eq!(rows, vec![(a, s(&fx.parent)), (b, s(&fx.nested))]);
}

#[test]
fn the_nested_scope_is_filtered_by_its_own_exclude_not_the_parents() {
    let fx = Fx::new();
    let a = fx.file(&fx.parent.join("gen/a.txt"), "new");
    let b = fx.file(&fx.nested.join("gen/b.txt"), "new");
    let c = fx.file(&fx.nested.join("vendor2/c.txt"), "new");
    fx.write_session(&fx.parent, json!({ &a: pending(Some("old")) }));
    fx.write_session(&fx.nested, json!({ &b: pending(Some("old")), &c: pending(Some("old")) }));
    // The parent excludes `gen`; the nested root excludes only `vendor2`.
    fx.excludes(json!({ s(&fx.parent): ["gen"], s(&fx.nested): ["vendor2"] }));
    let v = load_project(&fx.paths.home, &fx.parent);
    let paths: Vec<&str> = v.pending.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths, vec![b.as_str()]);
}

#[test]
fn the_list_folds_the_nested_root_into_its_parent() {
    let fx = Fx::new();
    fx.two_sessions();
    let list = list_projects(&fx.paths.home);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].root, s(&fx.parent));
    assert_eq!(list[0].pending, 2);
    assert_eq!(list[0].nested, vec![s(&fx.nested)]);
}

#[test]
fn a_sibling_with_a_common_prefix_is_its_own_project() {
    let fx = Fx::new();
    let sib = fx.parent.with_file_name("umbrella-other");
    let sib2 = fx.parent.with_file_name("umbrellax");
    fs::create_dir_all(&sib).unwrap();
    fs::create_dir_all(&sib2).unwrap();
    fx.roots(&[&fx.parent, &sib, &sib2]);
    let listed = listed_roots(&[fx.parent.clone(), sib.clone(), sib2.clone()]);
    assert_eq!(listed, vec![fx.parent.clone(), sib.clone(), sib2.clone()]);
    let v = load_project(&fx.paths.home, &fx.parent);
    assert_eq!(v.scopes.len(), 1);
}

#[test]
fn a_nested_root_missing_from_disk_stays_listed_as_missing() {
    let fx = Fx::new();
    let gone = fx.parent.join("gone");
    fx.roots(&[&fx.parent, &gone]);
    assert_eq!(listed_roots(&[fx.parent.clone(), gone.clone()]), vec![fx.parent.clone(), gone]);
    assert_eq!(load_project(&fx.paths.home, &fx.parent).scopes.len(), 1);
}

#[test]
fn a_decision_on_a_nested_row_writes_only_the_nested_session() {
    let fx = Fx::new();
    let (a, b) = fx.two_sessions();
    let parent_file = fx.session_file(&fx.parent);
    let before = fs::read(&parent_file).unwrap();
    let mtime = fs::metadata(&parent_file).unwrap().modified().unwrap();

    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::File { scope: s(&fx.nested), key: b.clone() }, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    let n = fx.session(&fx.nested);
    assert!(n["files"].as_object().unwrap().is_empty());
    assert_eq!(n["accepted"][0]["path"], b.as_str());
    // The parent's session is untouched, byte for byte, and was not even rewritten.
    assert_eq!(fs::read(&parent_file).unwrap(), before);
    assert_eq!(fs::metadata(&parent_file).unwrap().modified().unwrap(), mtime);
    assert!(fx.session(&fx.parent)["files"].get(&a).is_some());
    // No backup was taken of the parent's session either.
    let hash = parent_file.file_stem().unwrap().to_owned();
    assert!(!fx.paths.desktop_home.join("backups").join(hash).exists());
}

#[test]
fn a_nested_rows_key_is_not_found_in_the_parent_scope() {
    let fx = Fx::new();
    let (_, b) = fx.two_sessions();
    let parent_before = fs::read(fx.session_file(&fx.parent)).unwrap();
    let nested_before = fs::read(fx.session_file(&fx.nested)).unwrap();
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::File { scope: s(&fx.parent), key: b }, false, NOW);
    assert!(matches!(r, Err(DecideError::NotFound)));
    assert_eq!(fs::read(fx.session_file(&fx.parent)).unwrap(), parent_before);
    assert_eq!(fs::read(fx.session_file(&fx.nested)).unwrap(), nested_before);
}

#[test]
fn a_nested_reject_restores_the_file_and_writes_only_the_nested_session() {
    let fx = Fx::new();
    let (_, b) = fx.two_sessions();
    let parent_before = fs::read(fx.session_file(&fx.parent)).unwrap();
    decide(&fx.paths, &fx.parent, Action::Reject, Target::File { scope: s(&fx.nested), key: b.clone() }, false, NOW).unwrap();
    assert_eq!(fs::read_to_string(&b).unwrap(), "old\n");
    assert!(fx.session(&fx.nested)["rejected"].get(&b).is_some());
    assert_eq!(fs::read(fx.session_file(&fx.parent)).unwrap(), parent_before);
}

#[test]
fn a_folder_action_in_the_nested_scope_stays_inside_it() {
    let fx = Fx::new();
    let (a, b) = fx.two_sessions();
    let parent_before = fs::read(fx.session_file(&fx.parent)).unwrap();
    let prefix = s(&fx.nested.join("src"));
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::Folder { scope: s(&fx.nested), prefix }, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(fx.session(&fx.nested)["accepted"][0]["path"], b.as_str());
    assert_eq!(fs::read(fx.session_file(&fx.parent)).unwrap(), parent_before);
    assert!(fx.session(&fx.parent)["files"].get(&a).is_some());
}

#[test]
fn accept_all_spans_the_parent_and_the_nested_session() {
    let fx = Fx::new();
    let (a, b) = fx.two_sessions();
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::All, false, NOW).unwrap();
    assert_eq!(r.done, 2);
    assert_eq!(fx.session(&fx.parent)["accepted"][0]["path"], a.as_str());
    assert_eq!(fx.session(&fx.nested)["accepted"][0]["path"], b.as_str());
}

#[test]
fn bulk_actions_in_the_nested_scope_use_its_own_exclude() {
    let fx = Fx::new();
    let b = fx.file(&fx.nested.join("gen/b.txt"), "new");
    let c = fx.file(&fx.nested.join("vendor2/c.txt"), "new");
    fx.write_session(&fx.nested, json!({ &b: pending(Some("old")), &c: pending(Some("old")) }));
    fx.excludes(json!({ s(&fx.parent): ["gen"], s(&fx.nested): ["vendor2"] }));
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::All, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    let n = fx.session(&fx.nested);
    assert_eq!(n["accepted"][0]["path"], b.as_str());
    assert!(n["files"].get(&c).is_some());
}

#[test]
fn an_unknown_shape_in_the_nested_session_refuses_all_but_not_the_parents_file() {
    let fx = Fx::new();
    let (a, b) = fx.two_sessions();
    let mut n = fx.session(&fx.nested);
    n["reviewNotes"] = json!({});
    fs::write(fx.session_file(&fx.nested), n.to_string()).unwrap();
    let nested_before = fs::read(fx.session_file(&fx.nested)).unwrap();
    let parent_before = fs::read(fx.session_file(&fx.parent)).unwrap();
    // The worktree rule: an All that reaches the locked scope is refused as a whole.
    assert!(matches!(decide(&fx.paths, &fx.parent, Action::Accept, Target::All, false, NOW), Err(DecideError::ReadOnly(_))));
    assert_eq!(fs::read(fx.session_file(&fx.parent)).unwrap(), parent_before);
    // A row in the locked scope is refused; a row in the parent is not.
    let on_nested = Target::File { scope: s(&fx.nested), key: b };
    assert!(matches!(decide(&fx.paths, &fx.parent, Action::Accept, on_nested, false, NOW), Err(DecideError::ReadOnly(_))));
    decide(&fx.paths, &fx.parent, Action::Accept, Target::File { scope: s(&fx.parent), key: a }, false, NOW).unwrap();
    assert_eq!(fs::read(fx.session_file(&fx.nested)).unwrap(), nested_before);
}

#[test]
fn the_parents_upkeep_never_touches_the_nested_session() {
    let fx = Fx::new();
    // A settled no-op in each session: the parent's upkeep prunes only its own.
    let a = fx.file(&fx.parent.join("same.txt"), "same");
    let b = fx.file(&fx.nested.join("same.txt"), "same");
    fx.write_session(&fx.parent, json!({ &a: pending(Some("same")) }));
    fx.write_session(&fx.nested, json!({ &b: pending(Some("same")) }));
    let nested_before = fs::read(fx.session_file(&fx.nested)).unwrap();
    let record = ProjectRecord { added_by_app: true, ..Default::default() };
    let now = claudegate_core::time::now_ms();
    let r = upkeep(&fx.paths, &fx.parent, &UpkeepOpts { record, read_only: false, max_attached: 256 }, now);
    assert!(r.wrote());
    assert!(fx.session(&fx.parent)["files"].as_object().unwrap().is_empty());
    assert_eq!(fs::read(fx.session_file(&fx.nested)).unwrap(), nested_before);
}

#[test]
fn settled_no_ops_in_the_nested_scope_are_not_shown_or_counted() {
    let fx = Fx::new();
    let same = fx.file(&fx.nested.join("same.txt"), "same");
    let gone = s(&fx.nested.join("never-written.txt"));
    let real = fx.file(&fx.nested.join("real.txt"), "new");
    fx.write_session(
        &fx.nested,
        json!({ &same: pending(Some("same")), &gone: pending(None), &real: pending(Some("old")) }),
    );
    let v = load_project(&fx.paths.home, &fx.parent);
    assert_eq!(v.pending.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(), vec![real.as_str()]);
    assert_eq!(list_projects(&fx.paths.home)[0].pending, 1);
    // Display only: nothing was written.
    let n = fx.session(&fx.nested);
    assert_eq!(n["files"].as_object().unwrap().len(), 3);
    // A bulk accept leaves the no-ops alone.
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::All, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    let n = fx.session(&fx.nested);
    let mut left: Vec<&String> = n["files"].as_object().unwrap().keys().collect();
    left.sort();
    let mut want = vec![&gone, &same];
    want.sort();
    assert_eq!(left, want);
}

/// The same path pending in the parent's session (a stale capture from before the nested root
/// was registered) and in the nested root's: one row, owned by the most specific scope.
fn same_path_in_both(fx: &Fx) -> String {
    let f = fx.file(&fx.nested.join("src/dup.go"), "claude\n");
    fx.write_session(&fx.parent, json!({ &f: pending(Some("parent-baseline\n")) }));
    fx.write_session(&fx.nested, json!({ &f: pending(Some("nested-baseline\n")) }));
    f
}

#[test]
fn a_path_pending_in_both_sessions_shows_once_in_the_nested_scope() {
    let fx = Fx::new();
    let f = same_path_in_both(&fx);
    let v = load_project(&fx.paths.home, &fx.parent);
    let rows: Vec<(String, String)> = v.pending.iter().map(|p| (p.path.clone(), p.scope.clone())).collect();
    assert_eq!(rows, vec![(f, s(&fx.nested))]);
    assert_eq!(list_projects(&fx.paths.home)[0].pending, 1);
}

#[test]
fn reject_all_with_a_path_in_both_sessions_touches_only_the_nested_entry() {
    let fx = Fx::new();
    let f = same_path_in_both(&fx);
    let parent_before = fs::read(fx.session_file(&fx.parent)).unwrap();
    let r = decide(&fx.paths, &fx.parent, Action::Reject, Target::All, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    // Restored to the nested baseline, never the parent's stale one.
    assert_eq!(fs::read_to_string(&f).unwrap(), "nested-baseline\n");
    assert!(fx.session(&fx.nested)["rejected"].get(&f).is_some());
    assert_eq!(fs::read(fx.session_file(&fx.parent)).unwrap(), parent_before);
}

#[test]
fn a_folder_action_in_the_parent_skips_a_path_the_nested_scope_owns() {
    let fx = Fx::new();
    let _ = same_path_in_both(&fx);
    let parent_before = fs::read(fx.session_file(&fx.parent)).unwrap();
    let prefix = s(&fx.nested);
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::Folder { scope: s(&fx.parent), prefix }, false, NOW).unwrap();
    assert_eq!(r.done, 0);
    assert_eq!(fs::read(fx.session_file(&fx.parent)).unwrap(), parent_before);
}

#[test]
fn a_parent_only_entry_under_the_nested_root_stays_the_parents() {
    let fx = Fx::new();
    let f = fx.file(&fx.nested.join("src/stale.go"), "new\n");
    fx.write_session(&fx.parent, json!({ &f: pending(Some("old\n")) }));
    fx.write_session(&fx.nested, json!({}));
    let v = load_project(&fx.paths.home, &fx.parent);
    assert_eq!(v.pending.iter().map(|p| p.scope.clone()).collect::<Vec<_>>(), vec![s(&fx.parent)]);
    let r = decide(&fx.paths, &fx.parent, Action::Accept, Target::All, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(fx.session(&fx.parent)["accepted"][0]["path"], f.as_str());
}

#[test]
fn a_scoped_confirmation_acts_on_that_scopes_row_only() {
    use claudegate_core::decide::decide_confirmed_scoped;
    let fx = Fx::new();
    let f = s(&fx.nested.join("x.go"));
    let rec = json!({ "id": "r", "path": &f, "before": "a", "after": "b", "decidedAt": CAP });
    for root in [&fx.parent, &fx.nested] {
        let v = json!({ "sessionId": "x", "status": "reviewed", "files": {}, "accepted": [], "rejected": { &f: rec } });
        fs::write(fx.session_file(root), v.to_string()).unwrap();
    }
    let keys = vec![(s(&fx.nested), f.clone())];
    let r = decide_confirmed_scoped(&fx.paths, &fx.parent, Action::ClearRejected, Target::All, &keys, false, NOW).unwrap();
    assert_eq!(r.done, 1);
    assert!(r.errors.is_empty());
    assert!(fx.session(&fx.nested)["rejected"].get(&f).is_none());
    assert!(fx.session(&fx.parent)["rejected"].get(&f).is_some());
}
