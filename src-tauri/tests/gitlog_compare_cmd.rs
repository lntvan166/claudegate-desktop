//! Compare Branches commands against real fixture repositories (CB §4).
mod gitfix;

use claudegate_desktop_lib::gitlog::{channel, GitLog, Runner, Tag, STALE};
use claudegate_desktop_lib::gitlog_compare::{BranchNameView, RepoCompareView, BAD_PAIR};
use gitfix::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

const LEFT: &str = "release-1.4";
const RIGHT: &str = "main";

struct Fx {
    _t: tempfile::TempDir,
    g: GitLog,
    dirs: BTreeMap<&'static str, PathBuf>,
    ids: BTreeMap<&'static str, String>,
    /// acme-api's commits: release-1.4 only (r1), the cherry-pick on it (r2), main only (m1),
    /// the original of the cherry-pick (m2).
    r1: String,
    r2: String,
    m1: String,
    m2: String,
}

impl Fx {
    fn id(&self, name: &str) -> String {
        self.ids[name].clone()
    }
    fn all(&self) -> Vec<String> {
        ["acme-api", "acme-web", "svc-lib", "acme-old", "acme-docs"].iter().map(|n| self.id(n)).collect()
    }
}

const NAMES: [&str; 5] = ["acme-api", "acme-web", "svc-lib", "acme-old", "acme-docs"];

/// Five repositories, each with `main`; `release-1.4` is:
/// - acme-api: branched from the base, with a commit of its own and a cherry-pick of main's fix;
/// - acme-web: two commits ahead of main;
/// - svc-lib: main itself;
/// - acme-old: absent;
/// - acme-docs: an orphan branch, no common history.
fn fixture() -> Fx {
    let (t, root) = tmp();
    let mut dirs = BTreeMap::new();
    for n in NAMES {
        let d = root.join(n);
        init(&d);
        dirs.insert(n, d);
    }

    let api = &dirs["acme-api"];
    let c0 = commit(api, &[("api/limit.go", Some(b"package api\n\nconst Limit = 10\n")), ("api/retry.go", Some(b"package api\n\nconst Retries = 1\n")), ("README.md", Some(b"# api\n"))], "base", BASE);
    let m1 = commit_as(api, "dana", &[("api/limit.go", Some(b"package api\n\nconst Limit = 50\n"))], "feat: rate limit", BASE + 10);
    let m2 = commit_as(api, "rin", &[("api/retry.go", Some(b"package api\n\nconst Retries = 3\n"))], "fix: retry on 503", BASE + 20);
    git(api, &["checkout", "-q", "-b", LEFT, &c0]);
    let r1 = commit_as(api, "dana", &[("README.md", Some(b"# api\n\nRelease 1.4.\n"))], "docs: release notes", BASE + 30);
    let r2 = commit_as(api, "rin", &[("api/retry.go", Some(b"package api\n\nconst Retries = 3\n"))], "fix: retry on 503", BASE + 40);
    git(api, &["checkout", "-q", RIGHT]);

    let web = &dirs["acme-web"];
    commit(web, &[("src/app.ts", Some(b"export const app = 1;\n"))], "base", BASE);
    git(web, &["checkout", "-q", "-b", LEFT]);
    commit(web, &[("src/app.ts", Some(b"export const app = 2;\n"))], "two", BASE + 10);
    commit(web, &[("src/app.ts", Some(b"export const app = 3;\n"))], "three", BASE + 20);
    git(web, &["checkout", "-q", RIGHT]);

    let lib = &dirs["svc-lib"];
    commit(lib, &[("lib.go", Some(b"package lib\n"))], "base", BASE);
    git(lib, &["branch", LEFT]);

    commit(&dirs["acme-old"], &[("old.txt", Some(b"old\n"))], "base", BASE);

    let docs = &dirs["acme-docs"];
    commit(docs, &[("index.md", Some(b"# docs\n"))], "base", BASE);
    git(docs, &["checkout", "-q", "--orphan", LEFT]);
    commit(docs, &[("index.md", Some(b"# release docs\n"))], "orphan", BASE + 10);
    git(docs, &["checkout", "-q", RIGHT]);

    let g = GitLog::new(Runner::new(PathBuf::from("git")));
    let repos: Vec<_> = NAMES.iter().map(|n| repo_at(&dirs[n])).collect();
    let ids = NAMES.iter().zip(&repos).map(|(n, r)| (*n, r.id.clone())).collect();
    g.set_repos(repos);
    Fx { _t: t, g, dirs, ids, r1, r2, m1, m2 }
}

fn differs(v: &RepoCompareView) -> (String, String, String) {
    match v {
        RepoCompareView::Differs { left_sha, right_sha, base, .. } => (left_sha.clone(), right_sha.clone(), base.clone()),
        other => panic!("not differs: {other:?}"),
    }
}

#[test]
fn compare_counts_and_kinds() {
    let fx = fixture();
    let v = fx.g.compare("main", 1, &fx.all(), LEFT, RIGHT).unwrap();
    assert_eq!(v.query_id, 1);
    let order: Vec<String> = v.rows.iter().map(|r| r.repo.clone()).collect();
    assert_eq!(order, fx.all(), "rows come back in request order");

    let api = &v.rows[0].result;
    let api_main = git(&fx.dirs["acme-api"], &["rev-parse", RIGHT]);
    let api_base = git(&fx.dirs["acme-api"], &["merge-base", LEFT, RIGHT]);
    assert_eq!(
        *api,
        RepoCompareView::Differs { left_sha: fx.r2.clone(), right_sha: api_main.clone(), base: api_base, left: 1, right: 1, same_left: 1, same_right: 1 }
    );
    assert_eq!(api_main, fx.m2);
    match &v.rows[1].result {
        RepoCompareView::Differs { left, right, same_left, same_right, .. } => assert_eq!((*left, *right, *same_left, *same_right), (2, 0, 0, 0)),
        other => panic!("acme-web: {other:?}"),
    }
    assert_eq!(v.rows[2].result, RepoCompareView::Identical);
    assert_eq!(v.rows[3].result, RepoCompareView::Missing);
    assert_eq!(v.rows[4].result, RepoCompareView::Nobase);

    // The webview's shape: the row flattened, camelCase.
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j["queryId"], 1);
    assert_eq!(j["rows"][0]["kind"], "differs");
    assert_eq!(j["rows"][0]["sameLeft"], 1);
    assert_eq!(j["rows"][0]["leftSha"], fx.r2.as_str());
    assert_eq!(j["rows"][3], serde_json::json!({"repo": fx.id("acme-old"), "kind": "missing"}));

    // Swapped, the counts swap.
    let v = fx.g.compare("main", 2, &[fx.id("acme-web")], RIGHT, LEFT).unwrap();
    match &v.rows[0].result {
        RepoCompareView::Differs { left, right, .. } => assert_eq!((*left, *right), (0, 2)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn compare_rejects_bad_input() {
    let fx = fixture();
    for bad in ["-x", "--output=/tmp/x", "a..b", "HEAD~1", "", "x y", "main^{tree}"] {
        assert_eq!(fx.g.compare("main", 1, &fx.all(), bad, RIGHT), Err(BAD_PAIR.to_string()), "{bad:?}");
        assert_eq!(fx.g.compare("main", 1, &fx.all(), LEFT, bad), Err(BAD_PAIR.to_string()), "{bad:?}");
    }
    assert_eq!(fx.g.compare("main", 1, &["/nowhere/.git".to_string()], LEFT, RIGHT), Err("Unknown repository; refresh the list".to_string()));
    let v = fx.g.compare("main", 2, &fx.all(), LEFT, RIGHT).unwrap();
    let (l, r, b) = differs(&v.rows[0].result);
    let api = fx.id("acme-api");
    assert_eq!(fx.g.compare_files("main", 2, &api, "HEAD", &r, &b), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.compare_files("main", 2, &api, &l, &r, "-x"), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.compare_commits("main", 2, &api, "up", &l, &r, 10), Err("Not a side".to_string()));
    assert_eq!(fx.g.compare_commits("main", 2, &api, "left", "main", &r, 10), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.compare_commits("main", 2, "/nowhere/.git", "left", &l, &r, 10), Err("Unknown repository; refresh the list".to_string()));
    assert_eq!(fx.g.branches(&["/nowhere/.git".to_string()]), Err("Unknown repository; refresh the list".to_string()));
    // Refused requests superseded nothing.
    assert!(fx.g.compare_files("main", 2, &api, &l, &r, &b).is_ok());
}

#[test]
fn compare_files_both_tag() {
    let fx = fixture();
    let api = fx.id("acme-api");
    let v = fx.g.compare("main", 1, std::slice::from_ref(&api), LEFT, RIGHT).unwrap();
    let (l, r, b) = differs(&v.rows[0].result);
    let f = fx.g.compare_files("main", 1, &api, &l, &r, &b).unwrap();
    let paths = |v: &[claudegate_desktop_lib::gitlog::ChangedFileView]| v.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
    assert_eq!(paths(&f.left), ["README.md", "api/retry.go"]);
    assert_eq!(paths(&f.right), ["api/limit.go", "api/retry.go"]);
    assert_eq!(f.both, ["api/retry.go"]);
    let readme = &f.left[0];
    assert_eq!((readme.status, readme.added, readme.removed), ("modified", Some(2), Some(0)));
    let j = serde_json::to_value(&f).unwrap();
    assert_eq!(j["both"], serde_json::json!(["api/retry.go"]));
    assert_eq!(j["right"][0]["status"], "modified");

    // A newer pair makes this one stale.
    fx.g.compare("main", 2, std::slice::from_ref(&api), RIGHT, LEFT).unwrap();
    assert_eq!(fx.g.compare_files("main", 1, &api, &l, &r, &b), Err(STALE.to_string()));
}

#[test]
fn compare_commits_marks_duplicates() {
    let fx = fixture();
    let api = fx.id("acme-api");
    let v = fx.g.compare("main", 1, std::slice::from_ref(&api), LEFT, RIGHT).unwrap();
    let (l, r, _) = differs(&v.rows[0].result);
    let left = fx.g.compare_commits("main", 1, &api, "left", &l, &r, 500).unwrap();
    let marks: Vec<(String, String)> = left.iter().map(|c| (c.mark.clone(), c.sha.clone())).collect();
    assert_eq!(marks, [("=".to_string(), fx.r2.clone()), ("+".to_string(), fx.r1.clone())]);
    assert_eq!(left[0].subject, "fix: retry on 503");
    assert_eq!(left[0].author, "rin");
    assert_eq!(left[0].time, BASE + 40);
    assert_eq!(left[0].parents, [fx.r1.clone()]);
    let right = fx.g.compare_commits("main", 1, &api, "right", &l, &r, 500).unwrap();
    let marks: Vec<(String, String)> = right.iter().map(|c| (c.mark.clone(), c.sha.clone())).collect();
    assert_eq!(marks, [("=".to_string(), fx.m2.clone()), ("+".to_string(), fx.m1.clone())]);
    // `max` is clamped to at least one.
    assert_eq!(fx.g.compare_commits("main", 1, &api, "right", &l, &r, 0).unwrap().len(), 1);
    let j = serde_json::to_value(&right[0]).unwrap();
    assert_eq!(j["mark"], "=");
}

/// A git wrapper that never answers in a folder named `*slow*` (it becomes `sleep`, so killing
/// it kills the only process), and is plain git elsewhere. Copied from `gitlog_cmd.rs`.
fn slow_git(dir: &Path) -> PathBuf {
    let p = dir.join("slowgit");
    std::fs::write(&p, "#!/bin/sh\ncase \"$(pwd -P)\" in *slow*) exec sleep 30;; esac\nexec git \"$@\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

#[test]
fn compare_superseded_is_stale() {
    let (_t, root) = tmp();
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let (slow, good) = (root.join("ws/slow-repo"), root.join("ws/acme-api"));
    for d in [&slow, &good] {
        init(d);
        commit(d, &[("a.txt", Some(b"a\n"))], "first", BASE);
        git(d, &["branch", LEFT]);
    }
    let g = Arc::new(GitLog::new(Runner::new(slow_git(&tools))));
    let (s, gd) = (repo_at(&slow), repo_at(&good));
    let (sid, gid) = (s.id.clone(), gd.id.clone());
    g.set_repos(vec![s, gd]);

    let g1 = g.clone();
    let q1 = std::thread::spawn(move || g1.compare("main", 1, &[sid], LEFT, RIGHT));
    let tag1 = Tag { window: channel("main", "compare"), query: 1 };
    let deadline = Instant::now() + Duration::from_secs(10);
    let pids = loop {
        let p = g.runner.pids(&tag1);
        if !p.is_empty() {
            break p;
        }
        assert!(Instant::now() < deadline, "query 1 never started git");
        std::thread::sleep(Duration::from_millis(20));
    };

    let started = Instant::now();
    let v2 = g.compare("main", 2, &[gid], LEFT, RIGHT).unwrap();
    assert_eq!(v2.rows[0].result, RepoCompareView::Identical);
    assert_eq!(q1.join().unwrap(), Err(STALE.to_string()));
    assert!(started.elapsed() < Duration::from_secs(10), "the supersede must not wait for the slow child");
    let deadline = Instant::now() + Duration::from_secs(2);
    while pids.iter().any(|p| alive(*p)) {
        assert!(Instant::now() < deadline, "the superseded git is still alive");
        std::thread::sleep(Duration::from_millis(20));
    }
    // An older query arriving late is stale and kills nothing.
    assert_eq!(g.compare("main", 1, &[], LEFT, RIGHT), Err(STALE.to_string()));
}

/// Every file under `.git` with its size and modification time.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.join(".git")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                out.insert(e.path(), (m.len(), m.modified().unwrap()));
            }
        }
    }
    out
}

#[test]
fn compare_writes_nothing() {
    let fx = fixture();
    let status: Vec<String> = NAMES.iter().map(|n| git(&fx.dirs[n], &["--no-optional-locks", "status", "--porcelain"])).collect();
    let before: Vec<_> = NAMES.iter().map(|n| snapshot(&fx.dirs[n])).collect();

    let v = fx.g.compare("main", 1, &fx.all(), LEFT, RIGHT).unwrap();
    for (n, row) in NAMES.iter().zip(&v.rows) {
        if let RepoCompareView::Differs { left_sha, right_sha, base, .. } = &row.result {
            fx.g.compare_files("main", 1, &fx.id(n), left_sha, right_sha, base).unwrap();
            for side in ["left", "right"] {
                fx.g.compare_commits("main", 1, &fx.id(n), side, left_sha, right_sha, 500).unwrap();
            }
        }
    }
    fx.g.branches(&fx.all()).unwrap();

    let after: Vec<_> = NAMES.iter().map(|n| snapshot(&fx.dirs[n])).collect();
    assert_eq!(before, after, "nothing under .git changed");
    let status_after: Vec<String> = NAMES.iter().map(|n| git(&fx.dirs[n], &["--no-optional-locks", "status", "--porcelain"])).collect();
    assert_eq!(status, status_after);
}

#[test]
fn branches_counted_across_repos() {
    let fx = fixture();
    let api = &fx.dirs["acme-api"];
    git(api, &["update-ref", "refs/remotes/origin/main", &fx.m2]);
    git(api, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    let v = fx.g.branches(&fx.all()).unwrap();
    let b = |name: &str, count: usize, remote: bool| BranchNameView { name: name.into(), count, remote };
    assert_eq!(v, [b("main", 5, false), b("release-1.4", 4, false), b("origin/main", 1, true)]);
    // origin/HEAD is not a branch, and the local main is not origin's.
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j[2], serde_json::json!({"name": "origin/main", "count": 1, "remote": true}));
}

#[test]
fn run_code_reports_exit() {
    let fx = fixture();
    let r = Runner::new(PathBuf::from("git"));
    let docs = &fx.dirs["acme-docs"];
    let (main, rel) = (git(docs, &["rev-parse", RIGHT]), git(docs, &["rev-parse", LEFT]));
    let (code, out) = r.run_code(None, docs, &["merge-base", &main, &rel]).unwrap();
    assert_eq!((code, out), (1, Vec::new()));
    let api = &fx.dirs["acme-api"];
    let (code, out) = r.run_code(None, api, &["merge-base", &fx.r1, &fx.m1]).unwrap();
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&out).trim(), git(api, &["rev-parse", "HEAD~2"]));
    // A failing command is an answer here, and still an error through `run`.
    assert_eq!(r.run_code(None, api, &["rev-parse", "--verify", "--quiet", "nope"]).unwrap().0, 1);
    assert!(r.run(None, api, &["rev-parse", "--verify", "--quiet", "nope"]).is_err());
}

/// An executable shell script `name` in `dir`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A GitLog over `git` that runs through `wrapper`, for the fixture's repositories.
fn through(fx: &Fx, wrapper: PathBuf) -> GitLog {
    let g = GitLog::new(Runner::new(wrapper));
    g.set_repos(NAMES.iter().map(|n| repo_at(&fx.dirs[n])).collect());
    g
}

#[test]
fn nobase_runs_no_count() {
    let fx = fixture();
    let tools = tempfile::tempdir().unwrap();
    let log = tools.path().join("calls.log");
    // Each call's subcommand: the first argument after git's `-c` options.
    let wrapper = script(
        tools.path(),
        "loggit",
        &format!("for a in \"$@\"; do case \"$a\" in for-each-ref|merge-base|rev-list|log|diff) echo \"$a\" >> '{}'; break;; esac; done\nexec git \"$@\"\n", log.display()),
    );
    let g = through(&fx, wrapper);
    let v = g.compare("main", 1, &[fx.id("acme-docs")], LEFT, RIGHT).unwrap();
    assert_eq!(v.rows[0].result, RepoCompareView::Nobase);
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "for-each-ref\nmerge-base\n", "no rev-list --cherry-mark without a merge base");
    // With a base, the counts follow it.
    std::fs::remove_file(&log).unwrap();
    g.compare("main", 2, &[fx.id("acme-api")], LEFT, RIGHT).unwrap();
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "for-each-ref\nmerge-base\nrev-list\nrev-list\n");
}

#[test]
fn a_tag_never_shadows_a_branch() {
    let fx = fixture();
    let api = &fx.dirs["acme-api"];
    // A tag named like the branch, on another commit: rev-parse's DWIM would take the tag.
    git(api, &["tag", LEFT, &fx.m1]);
    let v = fx.g.compare("main", 1, &[fx.id("acme-api")], LEFT, RIGHT).unwrap();
    assert_eq!(differs(&v.rows[0].result).0, fx.r2, "the branch, not the tag");
    // A tag alone is no branch.
    git(api, &["tag", "v1.0", &fx.m1]);
    let v = fx.g.compare("main", 2, &[fx.id("acme-api")], "v1.0", RIGHT).unwrap();
    assert_eq!(v.rows[0].result, RepoCompareView::Missing);
}

#[test]
fn a_local_origin_branch_is_not_the_remote_one() {
    let fx = fixture();
    let api = &fx.dirs["acme-api"];
    git(api, &["update-ref", "refs/remotes/origin/x", &fx.r2]);
    git(api, &["update-ref", "refs/remotes/origin/y", &fx.r1]);
    // Only the remote: the remote-tracking branch is compared.
    let v = fx.g.compare("main", 1, &[fx.id("acme-api")], "origin/x", RIGHT).unwrap();
    assert_eq!(differs(&v.rows[0].result).0, fx.r2);
    // A local branch of the same name wins, and the picker lists both, each where it belongs.
    git(api, &["update-ref", "refs/heads/origin/x", &fx.r1]);
    let v = fx.g.compare("main", 2, &[fx.id("acme-api")], "origin/x", RIGHT).unwrap();
    assert_eq!(differs(&v.rows[0].result).0, fx.r1);
    let b = fx.g.branches(&[fx.id("acme-api")]).unwrap();
    let mut got: Vec<(String, bool)> = b.iter().map(|b| (b.name.clone(), b.remote)).collect();
    got.sort();
    let want: Vec<(String, bool)> = [("main", false), ("origin/x", false), ("origin/x", true), ("origin/y", true), ("release-1.4", false)].iter().map(|(n, r)| (n.to_string(), *r)).collect();
    assert_eq!(got, want);
}

#[test]
fn a_merge_is_not_counted() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    let c0 = commit(&d, &[("a.go", Some(b"a\n"))], "base", BASE);
    git(&d, &["branch", LEFT]);
    git(&d, &["checkout", "-q", "-b", "feat"]);
    let f1 = commit(&d, &[("b.go", Some(b"b\n"))], "feat: b", BASE + 10);
    git(&d, &["checkout", "-q", RIGHT]);
    let m0 = commit(&d, &[("c.go", Some(b"c\n"))], "feat: c", BASE + 20);
    git(&d, &["merge", "-q", "--no-ff", "-m", "merge feat", "feat"]);
    let merge = git(&d, &["rev-parse", "HEAD"]);
    assert_ne!(merge, m0);

    let g = GitLog::new(Runner::new(PathBuf::from("git")));
    let r = repo_at(&d);
    let id = r.id.clone();
    g.set_repos(vec![r]);
    let v = g.compare("main", 1, std::slice::from_ref(&id), LEFT, RIGHT).unwrap();
    assert_eq!(v.rows[0].result, RepoCompareView::Differs { left_sha: c0.clone(), right_sha: merge.clone(), base: c0.clone(), left: 0, right: 2, same_left: 0, same_right: 0 });
    let right = g.compare_commits("main", 1, &id, "right", &c0, &merge, 500).unwrap();
    let mut shas: Vec<String> = right.iter().map(|c| c.sha.clone()).collect();
    shas.sort();
    let mut want = vec![f1, m0];
    want.sort();
    assert_eq!(shas, want, "the merge commit is not listed");
}

#[test]
fn a_broken_repository_is_an_error_not_missing() {
    let fx = fixture();
    // The folder removed after discovery.
    std::fs::remove_dir_all(&fx.dirs["acme-old"]).unwrap();
    // A folder that is no git repository (any more).
    std::fs::remove_dir_all(fx.dirs["svc-lib"].join(".git")).unwrap();
    let v = fx.g.compare("main", 1, &[fx.id("acme-old"), fx.id("svc-lib")], LEFT, RIGHT).unwrap();
    match &v.rows[0].result {
        RepoCompareView::Error { reason } => assert!(reason.contains("is missing"), "{reason}"),
        other => panic!("removed folder: {other:?}"),
    }
    match &v.rows[1].result {
        RepoCompareView::Error { reason } => assert!(reason.contains("not a git repository"), "{reason}"),
        other => panic!("not a repository: {other:?}"),
    }
}

#[test]
fn merge_base_failure_keeps_gits_reason() {
    let fx = fixture();
    let tools = tempfile::tempdir().unwrap();
    let wrapper = script(tools.path(), "badbase", "for a in \"$@\"; do [ \"$a\" = merge-base ] && { echo 'fatal: merge-base broke' >&2; exit 2; }; done\nexec git \"$@\"\n");
    let g = through(&fx, wrapper);
    let v = g.compare("main", 1, &[fx.id("acme-api")], LEFT, RIGHT).unwrap();
    assert_eq!(v.rows[0].result, RepoCompareView::Error { reason: "fatal: merge-base broke".into() });
}

#[test]
fn a_signal_keeps_gits_reason() {
    let tools = tempfile::tempdir().unwrap();
    let wrapper = script(tools.path(), "killedgit", "echo 'fatal: out of memory' >&2\nkill -9 $$\n");
    let r = Runner::new(wrapper);
    assert_eq!(r.run(None, tools.path(), &["status"]), Err(claudegate_desktop_lib::gitlog::RunError::Failed("fatal: out of memory".into())));
    assert_eq!(r.run_code(None, tools.path(), &["status"]), Err(claudegate_desktop_lib::gitlog::RunError::Failed("fatal: out of memory".into())));
    // Nothing on stderr: the status says what happened.
    let quiet = script(tools.path(), "quietgit", "kill -9 $$\n");
    let e = Runner::new(quiet).run(None, tools.path(), &["status"]).unwrap_err();
    assert!(e.reason().contains("signal"), "{e:?}");
}
