use claudegate_core::gitlog::{ahead_behind, discover, RealGit};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(st.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&st.stderr));
}

fn repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
}

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(t.path()).unwrap();
    (t, p)
}

fn names(r: &[claudegate_core::gitlog::Repo]) -> Vec<String> {
    r.iter().map(|r| r.name.clone()).collect()
}

#[test]
fn finds_root_and_nested_repos_depth2() {
    let (_t, root) = tmp();
    repo(&root);
    repo(&root.join("a"));
    repo(&root.join("g/b"));
    repo(&root.join("g/h/deep"));
    let found = discover(&[root.clone()], 2, &[], &RealGit);
    assert_eq!(found.len(), 3, "{:?}", names(&found));
    assert!(!names(&found).contains(&"deep".to_string()));
}

#[test]
fn skips_node_modules_and_target() {
    let (_t, root) = tmp();
    repo(&root.join("app"));
    repo(&root.join("node_modules/pkg"));
    repo(&root.join("target/x"));
    repo(&root.join("vendor/y"));
    assert_eq!(names(&discover(&[root], 2, &[], &RealGit)), vec!["app"]);
}

#[test]
fn worktrees_group_into_one_repo() {
    let (_t, root) = tmp();
    let main = root.join("main-repo");
    repo(&main);
    git(&main, &["worktree", "add", "-q", "-b", "w1", root.join("wt1").to_str().unwrap()]);
    git(&main, &["worktree", "add", "-q", "--detach", root.join("wt2").to_str().unwrap()]);
    let found = discover(&[root], 2, &[], &RealGit);
    assert_eq!(found.len(), 1);
    let r = &found[0];
    assert_eq!(r.name, "main-repo");
    assert_eq!(r.checkouts.len(), 3);
    assert!(r.checkouts[0].is_main && r.checkouts[0].branch.as_deref() == Some("main"));
    assert_eq!(r.checkouts.iter().filter(|c| c.branch.is_none()).count(), 1);
    assert!(r.checkouts.iter().all(|c| c.head.len() == 40));
    assert!(r.color < claudegate_core::gitlog::PALETTE_LEN);
}

#[test]
fn same_name_disambiguated() {
    let (_t, root) = tmp();
    repo(&root.join("one/svc"));
    repo(&root.join("two/svc"));
    repo(&root.join("two/solo"));
    let n = names(&discover(&[root], 2, &[], &RealGit));
    assert_eq!(n, vec!["solo", "svc (one)", "svc (two)"]);
}

#[test]
fn excluded_repo_removed() {
    let (_t, root) = tmp();
    repo(&root.join("keep"));
    repo(&root.join("legacy"));
    repo(&root.join("tools/old"));
    let n = names(&discover(&[root], 2, &["legacy".to_string(), "**/old".to_string()], &RealGit));
    assert_eq!(n, vec!["keep"]);
}

#[test]
fn ahead_behind_none_without_upstream() {
    let (_t, root) = tmp();
    repo(&root.join("r"));
    let found = discover(&[root], 2, &[], &RealGit);
    assert_eq!(ahead_behind(&found[0], &RealGit), None);
}

#[test]
fn ahead_behind_counts_with_upstream() {
    let (_t, root) = tmp();
    let up = root.join("up");
    repo(&up);
    let me = root.join("me");
    git(&root, &["clone", "-q", up.to_str().unwrap(), "me"]);
    git(&me, &["commit", "-q", "--allow-empty", "-m", "local"]);
    git(&up, &["commit", "-q", "--allow-empty", "-m", "u1"]);
    git(&up, &["commit", "-q", "--allow-empty", "-m", "u2"]);
    git(&me, &["fetch", "-q"]);
    let found = discover(&[me], 0, &[], &RealGit);
    assert_eq!(ahead_behind(&found[0], &RealGit), Some((1, 2)));
}

#[test]
fn seventy_repos_discovery_time() {
    let (_t, root) = tmp();
    for i in 0..70 {
        let d = root.join(format!("g{}/r{i}", i % 7));
        repo(&d);
        if i % 10 == 0 {
            git(&d, &["worktree", "add", "-q", "--detach", root.join(format!("g{}/wt{i}", i % 7)).to_str().unwrap()]);
        }
    }
    let t = std::time::Instant::now();
    let found = discover(&[root], 2, &[], &RealGit);
    let el = t.elapsed();
    eprintln!("discover 70 repos: {el:?}");
    assert_eq!(found.len(), 70);
    assert!(el.as_secs() < 10, "{el:?}");
}

fn commit_at(dir: &Path) -> String {
    let o = Command::new("git").args(["rev-parse", "HEAD"]).current_dir(dir).output().unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

#[test]
fn submodule_is_its_own_repo_at_its_working_tree() {
    let (_t, root) = tmp();
    let lib = root.join("lib-src");
    repo(&lib);
    let sup = root.join("super");
    repo(&sup);
    git(&sup, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", lib.to_str().unwrap(), "x"]);
    let found = discover(&[sup.clone()], 2, &[], &RealGit);
    assert_eq!(found.len(), 2, "{:?}", names(&found));
    let x = found.iter().find(|r| r.name == "x").unwrap();
    assert!(x.id.ends_with(".git/modules/x"), "{}", x.id);
    assert_eq!(x.checkouts.len(), 1);
    assert_eq!(x.checkouts[0].path, sup.join("x").to_string_lossy());
    assert!(x.checkouts[0].is_main);
}

#[test]
fn detached_main_head() {
    let (_t, root) = tmp();
    let r = root.join("r");
    repo(&r);
    git(&r, &["checkout", "-q", "--detach"]);
    let f = discover(&[root], 2, &[], &RealGit);
    assert_eq!(f[0].checkouts[0].branch, None);
    assert!(f[0].checkouts[0].is_main);
    assert_eq!(f[0].checkouts[0].head, commit_at(&r));
}

#[test]
fn unborn_repo_is_dropped() {
    let (_t, root) = tmp();
    std::fs::create_dir_all(root.join("empty")).unwrap();
    git(&root.join("empty"), &["init", "-q"]);
    repo(&root.join("real"));
    assert_eq!(names(&discover(&[root], 2, &[], &RealGit)), vec!["real"]);
}

#[test]
fn worktree_outside_root_is_the_same_repo() {
    let (_t, root) = tmp();
    let (_t2, elsewhere) = tmp();
    let main = root.join("r");
    repo(&main);
    let wt = elsewhere.join("wt");
    git(&main, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()]);
    let f = discover(&[root], 2, &[], &RealGit);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].checkouts.len(), 2);
    assert!(f[0].checkouts.iter().any(|c| c.path == wt.to_string_lossy()));
}

#[test]
fn bare_repo_with_worktrees_keeps_first_checkout_as_main() {
    let (_t, root) = tmp();
    let seed = root.join("seed");
    repo(&seed);
    let bare = root.join("store.git");
    git(&root, &["clone", "-q", "--bare", seed.to_str().unwrap(), bare.to_str().unwrap()]);
    let scan = root.join("scan");
    git(&bare, &["worktree", "add", "-q", "--detach", scan.join("w1").to_str().unwrap()]);
    git(&bare, &["worktree", "add", "-q", "--detach", scan.join("w2").to_str().unwrap()]);
    let f = discover(&[scan], 2, &[], &RealGit);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].checkouts.len(), 2);
    assert!(f[0].checkouts[0].is_main && !f[0].checkouts[1].is_main);
}

#[test]
fn symlink_loop_terminates() {
    let (_t, root) = tmp();
    repo(&root.join("r"));
    std::os::unix::fs::symlink("..", root.join("r").join("loop")).unwrap();
    std::os::unix::fs::symlink("..", root.join("up")).unwrap();
    assert_eq!(names(&discover(&[root], 4, &[], &RealGit)), vec!["r"]);
}

#[test]
fn unreadable_dir_is_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, root) = tmp();
    repo(&root.join("ok"));
    let locked = root.join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let f = discover(&[root], 2, &[], &RealGit);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(names(&f), vec!["ok"]);
}

#[test]
fn dash_named_repo_dir() {
    let (_t, root) = tmp();
    repo(&root.join("-x"));
    let f = discover(&[root], 2, &[], &RealGit);
    assert_eq!(names(&f), vec!["-x"]);
    assert_eq!(f[0].checkouts.len(), 1);
}

#[test]
fn same_name_same_parent_falls_back_to_full_path() {
    let (_t, root) = tmp();
    repo(&root.join("a/x/svc"));
    repo(&root.join("b/x/svc"));
    let n = names(&discover(&[root.clone()], 3, &[], &RealGit));
    assert_eq!(n, vec![root.join("a/x/svc").to_string_lossy(), root.join("b/x/svc").to_string_lossy()]);
}

#[test]
fn bare_name_exclude_never_removes_the_root_itself() {
    let (_t, root) = tmp();
    let r = root.join("legacy");
    repo(&r);
    assert_eq!(names(&discover(&[r], 1, &["legacy".to_string()], &RealGit)), vec!["legacy"]);
}
