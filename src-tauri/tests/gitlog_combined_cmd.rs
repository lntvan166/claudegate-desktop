//! Combined Changes commands against real fixture repositories (CC §6 shapes).
mod gitfix;

use claudegate_desktop_lib::gitlog::{channel, FileView, FilterView, GitLog, Runner, Tag, STALE};
use claudegate_desktop_lib::gitlog_combined::{CombinedFileView, CombinedLeftView, CombinedRepoView, CombinedView, CommitRef};
use gitfix::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

fn gl() -> GitLog {
    GitLog::new(Runner::new(PathBuf::from("git")))
}

fn text(lines: &[&str]) -> Vec<u8> {
    let mut s = lines.join("\n");
    s.push('\n');
    s.into_bytes()
}

fn item(repo: &str, sha: &str) -> CommitRef {
    CommitRef { repo: repo.to_string(), sha: sha.to_string() }
}

fn file<'a>(r: &'a CombinedRepoView, path: &str) -> &'a CombinedFileView {
    r.files.iter().find(|f| f.path == path).unwrap_or_else(|| panic!("{path:?} not listed: {:?}", r.files))
}

/// The left side the UI asks for: the file at `last` without its selected commits, newest first.
fn left_of(g: &GitLog, q: u64, repo: &str, f: &CombinedFileView) -> CombinedLeftView {
    let without: Vec<String> = f.shas.iter().rev().cloned().collect();
    g.combined_left("main", q, repo, &f.last, &f.path, &without).unwrap()
}

struct Web {
    _t: tempfile::TempDir,
    g: GitLog,
    web_dir: PathBuf,
    api_dir: PathBuf,
    web: String,
    api: String,
    p: String,
    a: String,
    x: String,
    c: String,
    d1: String,
    d2: String,
}

const TS_P: [&str; 5] = ["export const one = 1;", "export const two = 2;", "export const three = 3;", "export const four = 4;", "export const five = 5;"];
const CSS_P: [&str; 3] = [".header { color: black; }", ".title { margin: 0; }", ".nav { display: flex; }"];

/// acme-web: p, then a (dana), x (rin), c (dana); acme-api: d1 (dana), r1 (rin), d2 (dana).
fn fixture() -> Web {
    let (t, root) = tmp();
    let web_dir = root.join("acme-web");
    let api_dir = root.join("acme-api");
    init(&web_dir);
    init(&api_dir);

    let mut ts = TS_P.to_vec();
    let mut css = CSS_P.to_vec();
    let p = commit(&web_dir, &[("src/header.ts", Some(&text(&ts))), ("src/header.css", Some(&text(&css)))], "header", BASE);
    ts[2] = "export const three = 'dana';";
    css[1] = ".title { margin: 4px; }";
    let a = commit_as(&web_dir, "dana", &[("src/header.ts", Some(&text(&ts))), ("src/header.css", Some(&text(&css))), ("src/debug.ts", Some(b"console.log('debug');\n"))], "Tweak the header", BASE + 10);
    ts[0] = "export const one = 'rin';";
    css[1] = ".title { margin: 8px; }";
    let x = commit_as(&web_dir, "rin", &[("src/header.ts", Some(&text(&ts))), ("src/header.css", Some(&text(&css)))], "Rin's header spacing", BASE + 20);
    ts[4] = "export const five = 'dana';";
    css[2] = ".nav { display: grid; }";
    let c = commit_as(&web_dir, "dana", &[("src/header.ts", Some(&text(&ts))), ("src/header.css", Some(&text(&css))), ("src/debug.ts", None)], "Finish the header", BASE + 30);

    let d1 = commit_as(&api_dir, "dana", &[("api/a.go", Some(b"package api\n\nfunc A() {}\n"))], "Add A", BASE + 5);
    commit_as(&api_dir, "rin", &[("api/b.go", Some(b"package api\n\nfunc B() {}\n"))], "Add B", BASE + 15);
    let d2 = commit_as(&api_dir, "dana", &[("api/a.go", Some(b"package api\n\nfunc A() int { return 1 }\n"))], "A returns", BASE + 25);

    let g = gl();
    let (w, ap) = (repo_at(&web_dir), repo_at(&api_dir));
    let (web, api) = (w.id.clone(), ap.id.clone());
    g.set_repos(vec![w, ap]);
    Web { _t: t, g, web_dir, api_dir, web, api, p, a, x, c, d1, d2 }
}

fn select(fx: &Web, q: u64) -> CombinedView {
    let items = [item(&fx.web, &fx.a), item(&fx.web, &fx.c), item(&fx.api, &fx.d1), item(&fx.api, &fx.d2)];
    fx.g.combined("main", q, &items).unwrap()
}

#[test]
fn combined_fixture_tree() {
    let fx = fixture();
    let v = select(&fx, 1);
    assert_eq!(v.query_id, 1);
    let order: Vec<&str> = v.repos.iter().map(|r| r.repo.as_str()).collect();
    assert_eq!(order, [fx.web.as_str(), fx.api.as_str()], "repos come back in request order");

    let web = &v.repos[0];
    assert_eq!(web.error, None);
    assert_eq!(web.commits, [fx.a.clone(), fx.c.clone()]);
    let css = file(web, "src/header.css");
    assert_eq!((css.mode, css.reason, css.status), ("fallback", Some("conflict"), "modified"));
    assert_eq!(css.included, [fx.x.clone()]);
    let ts = file(web, "src/header.ts");
    assert_eq!((ts.mode, ts.reason, ts.status), ("merge", None, "modified"));
    assert_eq!(ts.included, [fx.x.clone()]);
    assert_eq!(ts.shas, [fx.a.clone(), fx.c.clone()]);
    assert_eq!(ts.last, fx.c);
    assert_eq!(ts.before_ref.as_deref(), Some(fx.p.as_str()));
    assert_eq!(web.files.len(), 2, "{:?}", web.files);
    assert_eq!(web.no_net_change, ["src/debug.ts"]);
    assert_eq!(web.subjects.get(&fx.x).map(String::as_str), Some("Rin's header spacing"));

    let api = &v.repos[1];
    assert_eq!(api.error, None);
    let a = file(api, "api/a.go");
    assert_eq!((a.mode, a.status), ("fast", "added"));
    assert_eq!(a.shas, [fx.d1.clone(), fx.d2.clone()]);
    assert!(a.included.is_empty());
    assert_eq!(api.files.len(), 1);

    // The other way round, the repositories come back the other way round.
    let items = [item(&fx.api, &fx.d2), item(&fx.web, &fx.c), item(&fx.api, &fx.d1), item(&fx.web, &fx.a)];
    let v = fx.g.combined("main", 2, &items).unwrap();
    let order: Vec<&str> = v.repos.iter().map(|r| r.repo.as_str()).collect();
    assert_eq!(order, [fx.api.as_str(), fx.web.as_str()]);

    // The camelCase shape the webview reads.
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(j["queryId"], 2);
    let f = &j["repos"][1]["files"].as_array().unwrap().iter().find(|f| f["path"] == "src/header.css").unwrap().clone();
    assert_eq!(f["beforeRef"], fx.p.as_str());
    assert_eq!(f["beforePath"], "src/header.css");
    assert_eq!(f["reason"], "conflict");
    assert!(j["repos"][1]["noNetChange"].is_array());
}

#[test]
fn combined_left_takes_selection_out() {
    let fx = fixture();
    select(&fx, 1);
    let without = [fx.c.clone(), fx.a.clone()];
    let left = fx.g.combined_left("main", 1, &fx.web, &fx.c, "src/header.ts", &without).unwrap();
    let CombinedLeftView::Text(t) = left else { panic!("{left:?}") };
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[0], "export const one = 'rin';", "rin's line stays");
    assert_eq!(lines[2], TS_P[2], "a's line is taken out");
    assert_eq!(lines[4], TS_P[4], "c's line is taken out");
    assert_eq!(lines.len(), 5);
    assert_eq!(fx.g.combined_left("main", 1, &fx.web, &fx.c, "src/header.css", &without).unwrap(), CombinedLeftView::Fallback("conflict"));

    let right = fx.g.rev_file(&fx.web, &fx.c, "src/header.ts").unwrap();
    let want = String::from_utf8(git_bytes(&fx.web_dir, &["show", &format!("{}:src/header.ts", fx.c)])).unwrap();
    assert_eq!(right, FileView::Text(want));
    assert_eq!(fx.g.rev_file(&fx.web, &fx.c, "src/debug.ts").unwrap(), FileView::Missing, "deleted at c");
    assert_eq!(fx.g.rev_file(&fx.web, "HEAD", "src/header.ts"), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.rev_file(&fx.web, &fx.c, "../x"), Err("Not a path in the repository".to_string()));

    // A newer selection makes the old query's left sides stale; opening a file never supersedes.
    select(&fx, 2);
    assert_eq!(fx.g.combined_left("main", 1, &fx.web, &fx.c, "src/header.ts", &without), Err(STALE.to_string()));
    assert!(fx.g.combined_left("main", 2, &fx.web, &fx.c, "src/header.ts", &without).is_ok());
    assert!(fx.g.combined_left("main", 2, &fx.web, &fx.c, "src/header.ts", &without).is_ok(), "still current after a left side");
}

fn git_bytes(dir: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git").args(args).current_dir(dir).env("GIT_OPTIONAL_LOCKS", "0").output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    out.stdout
}

/// What the repository looks like on disk: object files and bytes, `status`, the index's mtime.
#[derive(Debug, PartialEq)]
struct Snapshot {
    objects: (usize, u64),
    status: Vec<u8>,
    index_mtime: SystemTime,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn snapshot(dir: &Path) -> Snapshot {
    // The index's mtime before `status`, which runs with optional locks off anyway.
    let index_mtime = std::fs::metadata(dir.join(".git/index")).unwrap().modified().unwrap();
    let mut files = Vec::new();
    walk(&dir.join(".git/objects"), &mut files);
    let bytes = files.iter().map(|f| std::fs::metadata(f).unwrap().len()).sum();
    Snapshot { objects: (files.len(), bytes), status: git_bytes(dir, &["status", "--porcelain"]), index_mtime }
}

fn locks(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(&dir.join(".git"), &mut files);
    files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "lock")).collect()
}

#[test]
fn combined_writes_nothing() {
    let fx = fixture();
    let before = [snapshot(&fx.web_dir), snapshot(&fx.api_dir)];
    let v = select(&fx, 1);
    let mut lefts = 0;
    for r in &v.repos {
        for f in &r.files {
            left_of(&fx.g, 1, &r.repo, f);
            fx.g.rev_file(&r.repo, &f.last, &f.path).unwrap();
            lefts += 1;
        }
    }
    assert_eq!(lefts, 3);
    let after = [snapshot(&fx.web_dir), snapshot(&fx.api_dir)];
    assert_eq!(before, after);
    for d in [&fx.web_dir, &fx.api_dir] {
        assert!(locks(d).is_empty(), "{:?}", locks(d));
    }
}

fn is_ancestor(dir: &Path, a: &str, b: &str) -> bool {
    Command::new("git").args(["merge-base", "--is-ancestor", a, b]).current_dir(dir).status().unwrap().success()
}

#[test]
fn combined_merges_and_branches() {
    let (_t, root) = tmp();
    let d = root.join("svc-lib");
    init(&d);
    let mut s = vec!["l1", "l2", "l3", "l4", "l5", "l6", "l7"];
    let p = commit(&d, &[("lib/shared.go", Some(&text(&s)))], "base", BASE);
    s[0] = "main-1";
    let a = commit(&d, &[("lib/shared.go", Some(&text(&s)))], "on main", BASE + 10);
    git(&d, &["checkout", "-q", "-b", "feat", &p]);
    let mut f = vec!["l1", "l2", "l3", "l4", "l5", "l6", "l7"];
    f[6] = "feat-7";
    let b = commit(&d, &[("lib/shared.go", Some(&text(&f)))], "on feat", BASE + 20);
    let f2 = commit(&d, &[("lib/extra.go", Some(b"package lib\n"))], "extra", BASE + 30);
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["merge", "-q", "--no-ff", "--no-edit", "-m", "merge feat", "feat"]);
    let m = git(&d, &["rev-parse", "HEAD"]);

    let g = gl();
    let r = repo_at(&d);
    let id = r.id.clone();
    g.set_repos(vec![r]);
    let v = g.combined("main", 1, &[item(&id, &a), item(&id, &m)]).unwrap();
    let rv = &v.repos[0];
    assert_eq!(rv.error, None);
    let extra = file(rv, "lib/extra.go");
    assert_eq!(extra.shas, [m.clone()], "the merge's files are its first-parent diff");
    assert_eq!(extra.included, [f2.clone()]);
    let shared = file(rv, "lib/shared.go");
    assert_ne!(shared.mode, "fast");
    assert_eq!(shared.included, [b.clone()]);
    for f in &rv.files {
        assert!(matches!(left_of(&g, 1, &id, f), CombinedLeftView::Text(_) | CombinedLeftView::Fallback(_)));
    }

    // Two branches.
    let v = g.combined("main", 2, &[item(&id, &a), item(&id, &b)]).unwrap();
    let rv = &v.repos[0];
    assert_eq!(rv.error, None);
    assert!(!rv.files.is_empty());
    for f in &rv.files {
        assert!(["fast", "merge", "fallback"].contains(&f.mode));
        if f.mode == "fast" {
            assert!([&a, &b].iter().all(|s| is_ancestor(&d, s, &f.last)), "{f:?}");
        }
    }
    assert_ne!(file(rv, "lib/shared.go").mode, "fast", "a and b are on two branches");
}

#[test]
fn combined_hostile_names() {
    let (_t, root) = tmp();
    let d = root.join("acme-web");
    init(&d);
    let names = ["a\nb.ts", "-rf.ts", "x\x1ey.ts", "sp ace:colon.ts"];
    let base = ["one", "two", "three", "four", "five"];
    let all = |l: [&str; 5]| -> Vec<(String, Vec<u8>)> { names.iter().map(|n| (n.to_string(), text(&l))).collect() };
    let write = |l: [&str; 5], msg: &str, t: i64| {
        let files = all(l);
        let refs: Vec<(&str, Option<&[u8]>)> = files.iter().map(|(n, b)| (n.as_str(), Some(b.as_slice()))).collect();
        commit(&d, &refs, msg, t)
    };
    write(base, "base", BASE);
    let s1 = write(["ONE", "two", "three", "four", "five"], "s1", BASE + 10);
    write(["ONE", "two", "THREE", "four", "five"], "u", BASE + 20);
    let s2 = write(["ONE", "two", "THREE", "four", "FIVE"], "s2", BASE + 30);

    let g = gl();
    let r = repo_at(&d);
    let id = r.id.clone();
    g.set_repos(vec![r]);
    let v = g.combined("main", 1, &[item(&id, &s1), item(&id, &s2)]).unwrap();
    let rv = &v.repos[0];
    assert_eq!(rv.error, None);
    let mut listed: Vec<&str> = rv.files.iter().map(|f| f.path.as_str()).collect();
    listed.sort();
    let mut want = names.to_vec();
    want.sort();
    assert_eq!(listed, want);
    for f in &rv.files {
        assert_eq!(f.shas, [s1.clone(), s2.clone()]);
        let left = left_of(&g, 1, &id, f);
        assert_eq!(left, CombinedLeftView::Text(String::from_utf8(text(&["one", "two", "THREE", "four", "five"])).unwrap()), "{:?}", f.path);
        assert!(matches!(g.rev_file(&id, &s2, &f.path).unwrap(), FileView::Text(_)), "{:?}", f.path);
    }
}

#[test]
fn combined_binary_and_huge() {
    let (_t, root) = tmp();
    let d = root.join("acme-web");
    init(&d);
    let png = |k: u8| -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0, 0, 0, 13, 0, 0];
        b.extend(std::iter::repeat_n(7u8, 64));
        b[20] = k;
        b
    };
    let gen = |edits: &[(usize, &str)]| -> Vec<u8> {
        let mut l: Vec<String> = (0..25_000).map(|i| format!("line {i}")).collect();
        for (at, s) in edits {
            l[*at] = s.to_string();
        }
        let refs: Vec<&str> = l.iter().map(String::as_str).collect();
        text(&refs)
    };
    commit(&d, &[("logo.png", Some(&png(1))), ("gen.txt", Some(&gen(&[])))], "base", BASE);
    let s1 = commit(&d, &[("logo.png", Some(&png(2))), ("gen.txt", Some(&gen(&[(0, "s1")])))], "s1", BASE + 10);
    commit(&d, &[("logo.png", Some(&png(3))), ("gen.txt", Some(&gen(&[(0, "s1"), (100, "u")])))], "u", BASE + 20);
    let s2 = commit(&d, &[("logo.png", Some(&png(4))), ("gen.txt", Some(&gen(&[(0, "s1"), (100, "u"), (200, "s2")])))], "s2", BASE + 30);

    let g = gl();
    let r = repo_at(&d);
    let id = r.id.clone();
    g.set_repos(vec![r]);
    let v = g.combined("main", 1, &[item(&id, &s1), item(&id, &s2)]).unwrap();
    let rv = &v.repos[0];
    assert_eq!(rv.error, None);
    let logo = file(rv, "logo.png");
    assert_eq!((logo.mode, logo.reason), ("fallback", Some("binary")));
    let big = file(rv, "gen.txt");
    assert_eq!((big.mode, big.reason), ("fallback", Some("tooLarge")));
    assert_eq!(left_of(&g, 1, &id, logo), CombinedLeftView::Fallback("binary"));
    assert_eq!(left_of(&g, 1, &id, big), CombinedLeftView::Fallback("tooLarge"));
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

fn wait_for_pids(g: &GitLog, tag: &Tag) -> Vec<u32> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pids = g.runner.pids(tag);
        if !pids.is_empty() {
            return pids;
        }
        assert!(Instant::now() < deadline, "{tag:?} never started git");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn all_dead_within(pids: &[u32], d: Duration) {
    let deadline = Instant::now() + d;
    while pids.iter().any(|p| alive(*p)) {
        assert!(Instant::now() < deadline, "still alive: {:?}", pids.iter().filter(|p| alive(**p)).collect::<Vec<_>>());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A slow repository and a good one, each with one commit, behind the slow git wrapper.
fn slow_and_good() -> (tempfile::TempDir, Arc<GitLog>, (String, String), (String, String)) {
    let (t, root) = tmp();
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let slow = root.join("ws/slow-repo");
    let good = root.join("ws/acme-api");
    let mut shas = Vec::new();
    for d in [&slow, &good] {
        init(d);
        shas.push(commit(d, &[("a.txt", Some(b"a\n"))], "first", BASE));
    }
    let g = Arc::new(GitLog::new(Runner::new(slow_git(&tools))));
    let (s, gd) = (repo_at(&slow), repo_at(&good));
    let ids = (s.id.clone(), gd.id.clone());
    g.set_repos(vec![s, gd]);
    (t, g, (ids.0, shas[0].clone()), (ids.1, shas[1].clone()))
}

#[test]
fn combined_superseded_is_stale_and_killed() {
    let (_t, g, slow, good) = slow_and_good();
    let g1 = g.clone();
    let slow_item = item(&slow.0, &slow.1);
    let q1 = std::thread::spawn(move || g1.combined("main", 1, &[slow_item]));
    let tag1 = Tag { window: channel("main", "combined"), query: 1 };
    let pids = wait_for_pids(&g, &tag1);
    assert!(pids.iter().all(|p| alive(*p)));

    let started = Instant::now();
    let v2 = g.combined("main", 2, &[item(&good.0, &good.1)]).unwrap();
    assert_eq!(v2.query_id, 2);
    assert_eq!(v2.repos.len(), 1);
    assert_eq!(v2.repos[0].error, None);
    assert_eq!(v2.repos[0].files.len(), 1);
    assert_eq!(q1.join().unwrap(), Err(STALE.to_string()));
    assert!(started.elapsed() < Duration::from_secs(10), "the supersede must not wait for the slow child");
    all_dead_within(&pids, Duration::from_secs(2));
    assert!(g.runner.pids(&tag1).is_empty());
}

#[test]
fn combined_channel_leaves_the_log_alone() {
    let (_t, g, slow, good) = slow_and_good();
    let g1 = g.clone();
    let slow_id = slow.0.clone();
    let page = std::thread::spawn(move || g1.page("main", 1, &[slow_id], &FilterView::default(), false));
    let log_tag = Tag { window: "main".into(), query: 1 };
    let pids = wait_for_pids(&g, &log_tag);

    // A higher id than the log's query, as the UI's ids (seeded from the clock) would be.
    let v = g.combined("main", 7, &[item(&good.0, &good.1)]).unwrap();
    assert_eq!(v.repos[0].error, None);
    assert!(pids.iter().all(|p| alive(*p)), "a combined query must not kill the log's paging");
    assert!(g.runner.is_current(&log_tag));
    assert_eq!(g.runner.pids(&log_tag), pids);

    g.forget_window("main");
    assert_eq!(page.join().unwrap(), Err(STALE.to_string()));
    all_dead_within(&pids, Duration::from_secs(2));
    assert!(!g.runner.is_current(&Tag { window: channel("main", "combined"), query: 7 }), "the window's channels are forgotten too");
}

#[test]
fn combined_rejects_bad_input() {
    let fx = fixture();
    let many: Vec<CommitRef> = (0..101u32).map(|i| item(&fx.web, &format!("{i:040x}"))).collect();
    assert_eq!(fx.g.combined("main", 1, &many), Err("Combined Changes shows up to 100 commits".to_string()));
    // 100 distinct commits, one of them twice: not over the limit.
    let mut hundred: Vec<CommitRef> = many[..100].to_vec();
    hundred.push(many[0].clone());
    let v = fx.g.combined("main", 2, &hundred).unwrap();
    assert!(v.repos[0].error.is_some(), "commits the repository doesn't have: its error, not the call's");

    assert_eq!(fx.g.combined("main", 3, &[item(&fx.web, "HEAD")]), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.combined("main", 3, &[item(&fx.web, &fx.a), item(&fx.web, "a1b2c3")]), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.combined("main", 3, &[item("/nowhere/.git", &fx.a)]), Err("Unknown repository; refresh the list".to_string()));

    select(&fx, 4);
    let ok = [fx.c.clone(), fx.a.clone()];
    assert_eq!(fx.g.combined_left("main", 4, &fx.web, &fx.c, "src/header.ts", &[fx.c.clone(), "HEAD~1".into()]), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.combined_left("main", 4, &fx.web, "HEAD", "src/header.ts", &ok), Err("Not a commit id".to_string()));
    assert_eq!(fx.g.combined_left("main", 4, &fx.web, &fx.c, "../x", &ok), Err("Not a path in the repository".to_string()));
    // `without` is bounded like a selection, and names each commit once.
    let too_many: Vec<String> = (0..101u32).map(|i| format!("{i:040x}")).collect();
    assert_eq!(fx.g.combined_left("main", 4, &fx.web, &fx.c, "src/header.ts", &too_many), Err("Combined Changes shows up to 100 commits".to_string()));
    assert_eq!(fx.g.combined_left("main", 4, &fx.web, &fx.c, "src/header.ts", &[fx.c.clone(), fx.a.clone(), fx.c.clone()]), Err("A commit is listed twice".to_string()));
    assert_eq!(fx.g.combined_left("main", 4, "/nowhere/.git", &fx.c, "src/header.ts", &ok), Err("Unknown repository; refresh the list".to_string()));
    // Refused requests superseded nothing.
    assert!(fx.g.combined_left("main", 4, &fx.web, &fx.c, "src/header.ts", &ok).is_ok());
}
