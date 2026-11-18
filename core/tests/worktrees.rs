use claudegate_core::worktrees::{is_worktree_root, nested_worktrees_under};
use std::fs;
use std::path::Path;

fn wt(dir: &Path, gitdir: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(".git"), format!("gitdir: {gitdir}\n")).unwrap();
}

#[test]
fn finds_worktree_not_submodule() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    wt(&r.join("wt"), "/x/.git/worktrees/wt");
    wt(&r.join("sub"), "/x/.git/modules/sub");
    wt(&r.join("node_modules/w"), "/x/.git/worktrees/w");
    assert!(is_worktree_root(&r.join("wt")));
    assert!(!is_worktree_root(&r.join("sub")));
    assert_eq!(nested_worktrees_under(r), vec![r.join("wt")]);
}

#[test]
fn main_repo_git_dir_is_not_worktree() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("repo/.git")).unwrap();
    assert!(!is_worktree_root(&t.path().join("repo")));
}

#[cfg(unix)]
#[test]
fn symlink_loop_terminates() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    wt(&r.join("wt"), "/x/.git/worktrees/wt");
    std::os::unix::fs::symlink(r, r.join("loop")).unwrap();
    assert_eq!(nested_worktrees_under(r), vec![r.join("wt")]);
}

#[test]
fn depth_limit() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    wt(&r.join("a/b/c/d/e/ok"), "/x/.git/worktrees/ok");
    wt(&r.join("a/b/c/d/e/f/deep"), "/x/.git/worktrees/deep");
    assert_eq!(nested_worktrees_under(r), vec![r.join("a/b/c/d/e/ok")]);
}

#[test]
fn does_not_descend_into_worktree_and_caps() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    wt(&r.join("wt"), "/x/.git/worktrees/wt");
    wt(&r.join("wt/inner"), "/x/.git/worktrees/inner");
    assert_eq!(nested_worktrees_under(r).len(), 1);
    for i in 0..260 {
        wt(&r.join(format!("w{i:03}")), "/x/.git/worktrees/w");
    }
    assert_eq!(nested_worktrees_under(r).len(), 256);
}

#[test]
fn max_attached_caps_the_scan() {
    use claudegate_core::worktrees::nested_worktrees_under_max;
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    for i in 0..5 {
        wt(&r.join(format!("w{i}")), "/x/.git/worktrees/w");
    }
    assert_eq!(nested_worktrees_under_max(r, 2), vec![r.join("w0"), r.join("w1")]);
    assert_eq!(nested_worktrees_under_max(r, 256).len(), 5);
    assert_eq!(nested_worktrees_under(r).len(), 5);
}
