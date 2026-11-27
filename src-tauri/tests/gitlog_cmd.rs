//! The gitlog commands against real fixture repositories.
mod gitfix;

use claudegate_desktop_lib::gitlog::{FileView, FilterView, GitLog, HistoryReq, PageView, Runner, Side, Tag, STALE};
use gitfix::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn gl() -> GitLog {
    GitLog::new(Runner::new(PathBuf::from("git")))
}

fn first_page(g: &GitLog, q: u64, ids: &[String], f: &FilterView) -> PageView {
    g.page("main", q, ids, f, false, None).unwrap()
}

fn ids_of(g: &GitLog, roots: &[PathBuf]) -> Vec<String> {
    g.repos_for(roots, 2, &[], true).iter().map(|r| r.id.clone()).collect()
}

/// A git wrapper that never answers in a folder named `*slow*` (it becomes `sleep`, so killing
/// it kills the only process), and is plain git elsewhere.
fn slow_git(dir: &Path) -> PathBuf {
    let p = dir.join("slowgit");
    std::fs::write(&p, "#!/bin/sh\ncase \"$(pwd -P)\" in *slow*) exec sleep 30;; esac\nexec git \"$@\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn alive(pid: u32) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Query 1 is stuck on the slow repository; query 2 supersedes it. Returns query 1's result,
/// query 2's page, and the pids query 1 had alive just before the supersede.
fn supersede() -> (Result<PageView, String>, PageView, Vec<u32>, String) {
    let (_t, root) = tmp();
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    let slow = root.join("ws/slow-repo");
    let good = root.join("ws/acme-api");
    for d in [&slow, &good] {
        init(d);
        commit(d, &[("a.txt", Some(b"a"))], "first", BASE);
    }
    let g = Arc::new(GitLog::new(Runner::new(slow_git(&tools))));
    let (s, gd) = (repo_at(&slow), repo_at(&good));
    let good_id = gd.id.clone();
    g.set_repos(vec![s.clone(), gd.clone()]);

    let g1 = g.clone();
    let both = vec![s.id.clone(), gd.id.clone()];
    let q1 = std::thread::spawn(move || g1.page("main", 1, &both, &FilterView::default(), false, None));
    let tag1 = Tag { window: "main".into(), query: 1 };
    let deadline = Instant::now() + Duration::from_secs(10);
    while g.runner.pids(&tag1).is_empty() {
        assert!(Instant::now() < deadline, "query 1 never started git");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The good repository's child finishes; the slow one stays.
    std::thread::sleep(Duration::from_millis(300));
    let pids = g.runner.pids(&tag1);
    assert!(!pids.is_empty() && pids.iter().all(|p| alive(*p)));

    let started = Instant::now();
    let p2 = g.page("main", 2, &[gd.id.clone()], &FilterView::default(), false, None).unwrap();
    let r1 = q1.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "the supersede must not wait for the slow child");
    (r1, p2, pids, good_id)
}

#[test]
fn stale_query_never_returned() {
    let (r1, p2, _, good) = supersede();
    assert_eq!(r1, Err(STALE.to_string()), "query 1's rows must never come back");
    assert_eq!(p2.query_id, 2);
    assert_eq!(p2.rows.len(), 1);
    assert_eq!(p2.rows[0].repo, good);
    assert!(p2.done);
}

#[test]
fn cancel_kills_children() {
    let (_, _, pids, _) = supersede();
    for p in pids {
        assert!(!alive(p), "query 1's child {p} is still alive");
    }
}

#[test]
fn stale_more_is_refused() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    for i in 0..5 {
        commit(&d, &[("a.txt", Some(format!("{i}").as_bytes()))], &format!("c{i}"), BASE + i);
    }
    let g = gl().with_page_size(2);
    let ids = ids_of(&g, &[root]);
    first_page(&g, 1, &ids, &FilterView::default());
    first_page(&g, 2, &ids, &FilterView::default());
    assert_eq!(g.page("main", 1, &ids, &FilterView::default(), true, None), Err(STALE.to_string()));
    assert!(g.page("main", 2, &ids, &FilterView::default(), true, None).is_ok());
}

#[test]
fn failing_repo_is_contained() {
    let (_t, root) = tmp();
    let good1 = root.join("acme-api");
    let good2 = root.join("acme-web");
    let empty = root.join("svc-empty");
    let gone = root.join("svc-gone");
    let corrupt = root.join("svc-corrupt");
    for (i, d) in [&good1, &good2, &gone, &corrupt].into_iter().enumerate() {
        init(d);
        commit(d, &[("a.txt", Some(b"a"))], "first", BASE + i as i64);
        commit(d, &[("a.txt", Some(b"b"))], "second", BASE + 100 + i as i64);
    }
    init(&empty);
    std::fs::remove_dir_all(&gone).unwrap();
    std::fs::write(corrupt.join(".git/HEAD"), "garbage\n").unwrap();

    let repos: Vec<_> = [&good1, &good2, &empty, &gone, &corrupt].into_iter().map(|d| repo_at(d)).collect();
    let ids: Vec<String> = repos.iter().map(|r| r.id.clone()).collect();
    let g = gl();
    g.set_repos(repos);
    let p = first_page(&g, 1, &ids, &FilterView::default());
    let rows_from: HashSet<&str> = p.rows.iter().map(|r| r.repo.as_str()).collect();
    assert_eq!(rows_from, HashSet::from([ids[0].as_str(), ids[1].as_str()]));
    assert_eq!(p.rows.len(), 4);
    let failed: HashSet<&str> = p.failures.iter().map(|f| f.repo.as_str()).collect();
    assert_eq!(failed, HashSet::from([ids[2].as_str(), ids[3].as_str(), ids[4].as_str()]), "{:?}", p.failures);
    assert!(p.failures.iter().all(|f| !f.reason.is_empty()));
    assert!(p.done);
}

#[test]
fn pages_cover_every_commit_once_in_order() {
    let (_t, root) = tmp();
    let busy = root.join("acme-api");
    let quiet = root.join("acme-web");
    bulk(&busy, 23, |i| BASE - (i as i64) * 60);
    // Same seconds as some of the busy repository's commits.
    bulk(&quiet, 4, |i| BASE - (i as i64) * 300);
    let g = gl().with_page_size(5);
    let ids = ids_of(&g, &[root]);
    let mut all = Vec::new();
    let mut p = first_page(&g, 7, &ids, &FilterView::default());
    let mut pages = 1;
    loop {
        assert!(p.rows.len() <= 5);
        all.extend(p.rows.clone());
        if p.done {
            break;
        }
        p = g.page("main", 7, &ids, &FilterView::default(), true, None).unwrap();
        pages += 1;
        assert!(pages < 50);
    }
    assert_eq!(all.len(), 27);
    let uniq: HashSet<(&str, &str)> = all.iter().map(|r| (r.repo.as_str(), r.sha.as_str())).collect();
    assert_eq!(uniq.len(), 27);
    let times: Vec<i64> = all.iter().map(|r| r.time - BASE).collect();
    assert!(all.windows(2).all(|w| w[0].time >= w[1].time), "newest first across pages: {times:?}");
}

/// A 0x1e byte in a subject or an author name must neither drop the commit nor skew the `--skip`
/// of the next page.
#[test]
fn rs_bytes_in_subjects_and_authors_keep_paging_exact() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    let mut want = Vec::new();
    for i in 0..11usize {
        let t = BASE + i as i64 * 60;
        let sha = match i % 3 {
            0 => commit(&d, &[("a.txt", Some(format!("{i}").as_bytes()))], &format!("evil\x1etail {i}"), t),
            1 => commit_by(&d, &format!("Ev\x1eil {i}"), &format!("plain {i}"), t),
            _ => commit(&d, &[("b.txt", Some(format!("{i}").as_bytes()))], &format!("\x1e{i}\x1e"), t),
        };
        want.push(sha);
    }
    let g = gl().with_page_size(3);
    let ids = ids_of(&g, &[root]);
    let mut all = Vec::new();
    let mut p = first_page(&g, 9, &ids, &FilterView::default());
    loop {
        all.extend(p.rows.clone());
        if p.done {
            break;
        }
        p = g.page("main", 9, &ids, &FilterView::default(), true, None).unwrap();
        assert!(all.len() < 50);
    }
    want.reverse();
    let got: Vec<String> = all.iter().map(|r| r.sha.clone()).collect();
    assert_eq!(got, want);
}

#[test]
fn branch_filter_falls_back_and_counts() {
    let (_t, root) = tmp();
    let a = root.join("acme-api");
    let b = root.join("acme-web");
    for d in [&a, &b] {
        init(d);
        commit(d, &[("a.txt", Some(b"a"))], "on main", BASE);
    }
    git(&a, &["checkout", "-q", "-b", "feature"]);
    commit(&a, &[("f.txt", Some(b"f"))], "on feature", BASE + 10);
    git(&a, &["checkout", "-q", "main"]);
    let g = gl();
    let ids = ids_of(&g, &[root]);
    let f = FilterView { branch: Some("feature".into()), ..Default::default() };
    let p = first_page(&g, 1, &ids, &f);
    let bu = p.branch_use.unwrap();
    assert_eq!((bu.found, bu.fallback), (1, 1));
    let subjects: HashSet<&str> = p.rows.iter().map(|r| r.subject.as_str()).collect();
    assert!(subjects.contains("on feature"), "the branch is logged where it exists");
    assert_eq!(p.rows.len(), 3);
}

#[test]
fn mine_uses_each_repos_email() {
    let (_t, root) = tmp();
    let a = root.join("acme-api");
    init(&a);
    commit(&a, &[("a.txt", Some(b"a"))], "mine", BASE);
    git(&a, &["config", "user.email", "ann@example.com"]);
    let g = gl();
    let ids = ids_of(&g, &[root]);
    let p = first_page(&g, 1, &ids, &FilterView { mine: true, ..Default::default() });
    assert_eq!(p.rows.len(), 1);
    git(&a, &["config", "user.email", "someone-else@example.com"]);
    let p = first_page(&g, 2, &ids, &FilterView { mine: true, ..Default::default() });
    assert!(p.rows.is_empty());
}

/// A user or repo `i18n.logOutputEncoding=UTF-16` would interleave NULs into the record framing.
#[test]
fn utf16_log_encoding_config_does_not_hide_commits() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    git(&d, &["config", "i18n.logOutputEncoding", "UTF-16"]);
    let mut want = Vec::new();
    for i in 0..7usize {
        want.push(commit(&d, &[("a.txt", Some(format!("{i}").as_bytes()))], &format!("subject {i}"), BASE + i as i64 * 60));
    }
    let g = gl().with_page_size(3);
    let ids = ids_of(&g, &[root]);
    let mut all = Vec::new();
    let mut p = first_page(&g, 11, &ids, &FilterView::default());
    loop {
        all.extend(p.rows.clone());
        if p.done {
            break;
        }
        p = g.page("main", 11, &ids, &FilterView::default(), true, None).unwrap();
        assert!(all.len() < 50);
    }
    want.reverse();
    let got: Vec<String> = all.iter().map(|r| r.sha.clone()).collect();
    assert_eq!(got, want);
    assert_eq!(all[0].subject, "subject 6");
}

#[test]
fn file_before_after() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    let moved = "line one\nline two\nline three\nline four\n".repeat(5);
    let root_sha = commit(
        &d,
        &[("keep.txt", Some(b"v1\n")), ("old.txt", Some(moved.as_bytes())), ("del.txt", Some(b"bye\n")), ("bin.dat", Some(b"text\n"))],
        "first",
        BASE,
    );
    let big = vec![b'x'; 2 * 1024 * 1024 + 1];
    std::fs::rename(d.join("old.txt"), d.join("renamed.txt")).unwrap();
    let sha = commit(
        &d,
        &[
            ("keep.txt", Some(b"v2\n")),
            ("new.txt", Some(b"hello\n")),
            ("del.txt", None),
            ("bin.dat", Some(&[0u8, 1, 2, 3, 0, 9])),
            ("latin.txt", Some(&[0x63, 0x61, 0x66, 0xe9])),
            ("big.txt", Some(&big)),
        ],
        "second",
        BASE + 60,
    );
    let g = gl();
    let id = ids_of(&g, &[root])[0].clone();
    let f = |path: &str, side| g.file(&id, &sha, path, side, None).unwrap();
    assert_eq!(f("new.txt", Side::Before), FileView::Missing);
    assert_eq!(f("new.txt", Side::After), FileView::Text("hello\n".into()));
    assert_eq!(f("del.txt", Side::Before), FileView::Text("bye\n".into()));
    assert_eq!(f("del.txt", Side::After), FileView::Missing);
    assert_eq!(f("renamed.txt", Side::Before), FileView::Text(moved.clone()), "before reads the renamed-from path");
    assert_eq!(f("renamed.txt", Side::After), FileView::Text(moved.clone()));
    assert_eq!(f("keep.txt", Side::Before), FileView::Text("v1\n".into()));
    assert_eq!(f("bin.dat", Side::Before), FileView::Text("text\n".into()));
    assert_eq!(f("bin.dat", Side::After), FileView::Binary);
    assert_eq!(f("latin.txt", Side::After), FileView::Binary, "not UTF-8");
    assert_eq!(f("big.txt", Side::After), FileView::TooLarge(big.len() as u64));
    assert_eq!(g.file(&id, &root_sha, "keep.txt", Side::Before, None).unwrap(), FileView::Missing, "a root commit has no before");
    assert_eq!(g.file(&id, &root_sha, "keep.txt", Side::After, None).unwrap(), FileView::Text("v1\n".into()));
    assert!(g.file(&id, &sha, "../outside", Side::After, None).is_err());
    assert!(g.file(&id, "HEAD", "keep.txt", Side::After, None).is_err());
    assert_eq!(g.file(&id, &sha, "renamed.txt", Side::Before, Some("old.txt")).unwrap(), FileView::Text(moved.clone()), "a given rename source skips the lookup");
    assert!(g.file(&id, &sha, "renamed.txt", Side::Before, Some("../x")).is_err());

    let ch = g.changes(&id, &sha).unwrap();
    let by: std::collections::HashMap<&str, _> = ch.iter().map(|c| (c.path.as_str(), c)).collect();
    assert_eq!(by["new.txt"].status, "added");
    assert_eq!(by["del.txt"].status, "deleted");
    assert_eq!(by["renamed.txt"].status, "renamed");
    assert_eq!(by["renamed.txt"].from.as_deref(), Some("old.txt"));
    assert_eq!((by["keep.txt"].added, by["keep.txt"].removed), (Some(1), Some(1)), "numstat is merged in");
    assert_eq!(by["bin.dat"].added, None, "binary: no counts");
    assert_eq!(g.message(&id, &sha).unwrap(), "second");
}

#[test]
fn web_url_from_origin() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    let sha = commit(&d, &[("a.txt", Some(b"a"))], "first", BASE);
    let g = gl();
    let id = ids_of(&g, &[root])[0].clone();
    assert_eq!(g.web_url(&id, &sha).unwrap(), None, "no remote: no link");
    git(&d, &["remote", "add", "origin", "git@github.com:acme/acme-api.git"]);
    assert_eq!(g.web_url(&id, &sha).unwrap(), Some(format!("https://github.com/acme/acme-api/commit/{sha}")));
}

#[test]
fn repo_list_is_cached_until_refresh() {
    let (_t, root) = tmp();
    init(&root.join("acme-api"));
    commit(&root.join("acme-api"), &[("a", Some(b"a"))], "x", BASE);
    let g = gl();
    let roots = vec![root.clone()];
    assert_eq!(g.repo_views(&roots, 2, &[], false).len(), 1);
    init(&root.join("acme-web"));
    commit(&root.join("acme-web"), &[("a", Some(b"a"))], "x", BASE);
    assert_eq!(g.repo_views(&roots, 2, &[], false).len(), 1, "cached");
    assert_eq!(g.repo_views(&roots, 2, &[], true).len(), 2, "refresh discovers again");
    let v = g.repo_views(&roots, 2, &[], false);
    assert_eq!((v[0].ahead, v[0].behind), (None, None), "no upstream");
}

#[test]
fn late_older_query_is_stale_and_harmless() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    for i in 0..5 {
        commit(&d, &[("a.txt", Some(format!("{i}").as_bytes()))], &format!("c{i}"), BASE + i);
    }
    let g = gl().with_page_size(2);
    let ids = ids_of(&g, &[root]);
    first_page(&g, 2, &ids, &FilterView::default());
    assert_eq!(g.page("main", 1, &ids, &FilterView::default(), false, None), Err(STALE.to_string()), "an older id arriving late");
    let more = g.page("main", 2, &ids, &FilterView::default(), true, None).unwrap();
    assert_eq!(more.rows.len(), 2, "query 2's state survived");
    g.forget_window("main");
    assert_eq!(g.page("main", 2, &ids, &FilterView::default(), true, None), Err(STALE.to_string()), "a closed window's query is gone");
    assert!(g.page("main", 1, &ids, &FilterView::default(), false, None).is_ok(), "a reopened window starts afresh");
}

#[test]
fn rows_carry_their_source() {
    let (_t, root) = tmp();
    let main = root.join("acme-api");
    init(&main);
    commit(&main, &[("a.txt", Some(b"a"))], "base", BASE);
    let wt = root.join("acme-api-wt");
    git(&main, &["worktree", "add", "-q", "-b", "w1", wt.to_str().unwrap()]);
    // The worktree's tip is newer than main's: git's --source would label "base" w1.
    commit(&main, &[("m.txt", Some(b"m"))], "on main", BASE + 10);
    commit(&wt, &[("w.txt", Some(b"w"))], "on w1", BASE + 20);
    let g = gl();
    let ids = ids_of(&g, &[root]);
    assert_eq!(ids.len(), 1, "one repository for the checkout and its worktree");
    let p = first_page(&g, 1, &ids, &FilterView::default());
    let src: std::collections::HashMap<&str, &str> = p.rows.iter().map(|r| (r.subject.as_str(), r.source.as_str())).collect();
    assert_eq!(src["on main"], "HEAD");
    assert_eq!(src["base"], "HEAD", "shared history below the fork point is the main checkout's");
    assert_eq!(src["on w1"], "refs/heads/w1");
    assert_eq!(p.rows.len(), 3);
    let f = FilterView { branch: Some("w1".into()), ..Default::default() };
    let p = first_page(&g, 2, &ids, &f);
    assert!(p.rows.iter().all(|r| r.source == "w1"), "one head: every row is that branch");
}

// ---------------------------------------------------------------------------------------------
// File History
// ---------------------------------------------------------------------------------------------

/// Enough lines that git sees the rename as one.
fn body(extra: &str) -> Vec<u8> {
    format!("line one\nline two\nline three\nline four\nline five\n{extra}").into_bytes()
}

/// old.ts added (t1), modified (t2), renamed to new.ts (t3), new.ts modified (t4), then an
/// unrelated other.ts (t5).
fn renamed_file(d: &Path) {
    init(d);
    commit(d, &[("old.ts", Some(&body("")))], "add old", BASE + 1);
    commit(d, &[("old.ts", Some(&body("two\n")))], "edit old", BASE + 2);
    commit(d, &[("old.ts", None), ("new.ts", Some(&body("two\n")))], "rename", BASE + 3);
    commit(d, &[("new.ts", Some(&body("two\nfour\n")))], "edit new", BASE + 4);
    commit(d, &[("other.ts", Some(b"o\n"))], "other", BASE + 5);
}

fn hist(repo: &str, path: &str) -> HistoryReq {
    HistoryReq { repo: repo.to_string(), path: path.to_string() }
}

/// (subject, path, from, status) of each row.
fn files(p: &PageView) -> Vec<(String, String, Option<String>, Option<&'static str>)> {
    p.rows
        .iter()
        .map(|r| {
            let f = r.file.as_ref().expect("a history row carries its file");
            (r.subject.clone(), f.path.clone(), f.from.clone(), f.status)
        })
        .collect()
}

fn four() -> Vec<(String, String, Option<String>, Option<&'static str>)> {
    vec![
        ("edit new".into(), "new.ts".into(), None, Some("modified")),
        ("rename".into(), "new.ts".into(), Some("old.ts".into()), Some("renamed")),
        ("edit old".into(), "old.ts".into(), None, Some("modified")),
        ("add old".into(), "old.ts".into(), None, Some("added")),
    ]
}

#[test]
fn history_follows_rename() {
    let (_t, root) = tmp();
    renamed_file(&root.join("acme-api"));
    let g = gl();
    let ids = ids_of(&g, &[root]);
    let h = hist(&ids[0], "new.ts");
    let p = g.page("main", 1, &ids, &FilterView::default(), false, Some(&h)).unwrap();
    assert_eq!(files(&p), four());
    assert!(p.done);
    assert!(p.failures.is_empty());
    assert!(p.rows.windows(2).all(|w| w[0].time > w[1].time), "newest first");
    let json = serde_json::to_value(&p.rows[1]).unwrap();
    assert_eq!(json["file"], serde_json::json!({ "path": "new.ts", "from": "old.ts", "status": "renamed" }));
    let log = first_page(&g, 2, &ids, &FilterView::default());
    assert!(serde_json::to_value(&log.rows[0]).unwrap().get("file").is_none(), "a log row has no file");
}

#[test]
fn history_sets_filters_aside() {
    let (_t, root) = tmp();
    renamed_file(&root.join("acme-api"));
    let g = gl();
    let ids = ids_of(&g, &[root]);
    let f = FilterView { search: Some("zzz".into()), authors: vec!["nobody".into()], within: Some(1), path: Some("other.ts".into()), mine: true, ..Default::default() };
    let p = g.page("main", 1, &ids, &f, false, Some(&hist(&ids[0], "new.ts"))).unwrap();
    assert_eq!(files(&p), four());
}

#[test]
fn history_pages_with_skip() {
    let (_t, root) = tmp();
    renamed_file(&root.join("acme-api"));
    let g = gl().with_page_size(2);
    let ids = ids_of(&g, &[root]);
    let h = hist(&ids[0], "new.ts");
    let p1 = g.page("main", 1, &ids, &FilterView::default(), false, Some(&h)).unwrap();
    assert_eq!(p1.rows.len(), 2);
    assert!(!p1.done);
    let p2 = g.page("main", 1, &ids, &FilterView::default(), true, Some(&h)).unwrap();
    assert_eq!(p2.rows.len(), 2);
    let p2 = if p2.done { p2 } else {
        let p3 = g.page("main", 1, &ids, &FilterView::default(), true, Some(&h)).unwrap();
        assert!(p3.rows.is_empty() && p3.done);
        p2
    };
    let mut all = files(&p1);
    all.extend(files(&p2));
    assert_eq!(all, four(), "no row repeats, and each keeps its file across pages");
    let shas: HashSet<String> = p1.rows.iter().chain(&p2.rows).map(|r| r.sha.clone()).collect();
    assert_eq!(shas.len(), 4);
}

#[test]
fn history_only_its_repo() {
    let (_t, root) = tmp();
    renamed_file(&root.join("acme-api"));
    renamed_file(&root.join("acme-web"));
    let g = gl();
    let ids = ids_of(&g, &[root]);
    assert_eq!(ids.len(), 2);
    let p = g.page("main", 1, &ids, &FilterView::default(), false, Some(&hist(&ids[1], "new.ts"))).unwrap();
    assert_eq!(p.rows.len(), 4);
    assert!(p.rows.iter().all(|r| r.repo == ids[1]));
    assert!(g.page("main", 2, &ids, &FilterView::default(), false, Some(&hist("/not/a/known/repo", "new.ts"))).is_err(), "only the backend's own repositories");
}

#[test]
fn history_keeps_branch() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    renamed_file(&d);
    git(&d, &["checkout", "-q", "-b", "feat"]);
    commit(&d, &[("new.ts", Some(&body("two\nfour\nfeat\n")))], "on feat", BASE + 6);
    git(&d, &["checkout", "-q", "main"]);
    let g = gl();
    let ids = ids_of(&g, &[root]);
    let h = hist(&ids[0], "new.ts");
    let f = FilterView { branch: Some("feat".into()), ..Default::default() };
    let p = g.page("main", 1, &ids, &f, false, Some(&h)).unwrap();
    let subjects: Vec<&str> = p.rows.iter().map(|r| r.subject.as_str()).collect();
    assert_eq!(subjects, vec!["on feat", "edit new", "rename", "edit old", "add old"]);
    assert!(p.rows.iter().all(|r| r.source == "feat"));
    let p = g.page("main", 2, &ids, &FilterView::default(), false, Some(&h)).unwrap();
    assert_eq!(p.rows.len(), 4, "without the branch: the current HEAD");
}

#[test]
fn history_rejects_bad_path() {
    let (_t, root) = tmp();
    renamed_file(&root.join("acme-api"));
    let g = gl();
    let ids = ids_of(&g, &[root]);
    for bad in ["../x", "/etc/passwd", "", "a/../b", "./a"] {
        assert_eq!(g.page("main", 1, &ids, &FilterView::default(), false, Some(&hist(&ids[0], bad))), Err("Not a path in the repository".to_string()), "{bad:?}");
    }
}

#[test]
fn history_never_supersedes_the_log() {
    let (_t, root) = tmp();
    renamed_file(&root.join("acme-api"));
    let g = gl().with_page_size(2);
    let ids = ids_of(&g, &[root]);
    let log = first_page(&g, 5, &ids, &FilterView::default());
    assert_eq!(log.rows.len(), 2);
    g.page("main", 6, &ids, &FilterView::default(), false, Some(&hist(&ids[0], "new.ts"))).unwrap();
    let more = g.page("main", 5, &ids, &FilterView::default(), true, None).unwrap();
    assert_eq!(more.rows.len(), 2, "the log keeps paging after a File History query");
    assert!(g.page("main", 6, &ids, &FilterView::default(), true, Some(&hist(&ids[0], "new.ts"))).is_ok(), "and History keeps its own state");
    g.forget_window("main");
    assert_eq!(g.page("main", 6, &ids, &FilterView::default(), true, Some(&hist(&ids[0], "new.ts"))), Err(STALE.to_string()), "a closed window drops its history too");
}

#[test]
fn history_of_a_file_with_hostile_names() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    init(&d);
    let from = "a\nR100\nb.ts";
    let to = "x\x1ey.ts";
    commit(&d, &[(from, Some(&body("")))], "add", BASE + 1);
    commit(&d, &[(from, None), (to, Some(&body("")))], "a\x1eb\x1fc", BASE + 2);
    commit(&d, &[(to, Some(&body("more\n")))], "edit", BASE + 3);
    let g = gl();
    let ids = ids_of(&g, &[root]);
    let p = g.page("main", 1, &ids, &FilterView::default(), false, Some(&hist(&ids[0], to))).unwrap();
    assert_eq!(
        files(&p),
        vec![
            ("edit".into(), to.into(), None, Some("modified")),
            ("a\x1eb\x1fc".into(), to.into(), Some(from.into()), Some("renamed")),
            ("add".into(), from.into(), None, Some("added")),
        ]
    );
}

/// A user's `log.follow=true` would turn the paged walk into `--follow`, which cannot page.
#[test]
fn history_pages_under_log_follow_config() {
    let (_t, root) = tmp();
    let d = root.join("acme-api");
    renamed_file(&d);
    git(&d, &["config", "log.follow", "true"]);
    git(&d, &["config", "log.showRoot", "false"]);
    let g = gl().with_page_size(3);
    let ids = ids_of(&g, &[root]);
    let h = hist(&ids[0], "new.ts");
    let mut all = Vec::new();
    let mut p = g.page("main", 1, &ids, &FilterView::default(), false, Some(&h)).unwrap();
    loop {
        all.extend(files(&p));
        if p.done {
            break;
        }
        p = g.page("main", 1, &ids, &FilterView::default(), true, Some(&h)).unwrap();
        assert!(all.len() < 20);
    }
    assert_eq!(all, four());
}
