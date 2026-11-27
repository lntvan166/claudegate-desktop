use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};

use claudegate_core::gitlog::{compare_commits, take_ready, Commit, RepoProgress};

thread_local!(static SERIAL: Cell<u64> = const { Cell::new(0) });

fn mk(repo: &str, time: i64) -> Commit {
    let n = SERIAL.with(|s| {
        s.set(s.get() + 1);
        s.get()
    });
    Commit {
        repo: repo.into(),
        sha: format!("{n:040x}"),
        time,
        author: "dana".into(),
        email: "dana@example.com".into(),
        subject: format!("{repo}@{time}"),
        parents: vec![],
    }
}

fn prog(pending: Vec<Commit>, exhausted: bool) -> RepoProgress {
    RepoProgress { fetched: pending.len(), pending, exhausted }
}

fn map(v: Vec<(&str, RepoProgress)>) -> BTreeMap<String, RepoProgress> {
    v.into_iter().map(|(k, p)| (k.to_string(), p)).collect()
}

fn times(rows: &[Commit]) -> Vec<i64> {
    rows.iter().map(|c| c.time).collect()
}

#[test]
fn merges_newest_first_and_exhausted_flush() {
    let mut p = map(vec![("web", prog(vec![mk("web", 50), mk("web", 10)], true)), ("api", prog(vec![mk("api", 40), mk("api", 20)], true))]);
    assert_eq!(times(&take_ready(&mut p)), vec![50, 40, 20, 10]);
    assert!(p.values().all(|x| x.pending.is_empty()));
}

#[test]
fn equal_times_order_by_sha() {
    let a = Commit { sha: "f".repeat(40), ..mk("web", 7) };
    let b = Commit { sha: "0".repeat(40), ..mk("api", 7) };
    let mut p = map(vec![("web", prog(vec![a.clone()], true)), ("api", prog(vec![b.clone()], true))]);
    let rows = take_ready(&mut p);
    assert_eq!(rows.iter().map(|c| c.sha.clone()).collect::<Vec<_>>(), vec![b.sha.clone(), a.sha.clone()]);
    assert!(compare_commits(&b, &a).is_lt());
}

#[test]
fn nothing_pending_yields_nothing() {
    assert!(take_ready(&mut BTreeMap::new()).is_empty());
    assert!(take_ready(&mut map(vec![("web", prog(vec![], true))])).is_empty());
}

#[test]
fn rows_below_horizon_held_not_dropped() {
    let mut p = map(vec![("web", prog(vec![mk("web", 900), mk("web", 500)], false)), ("api", prog(vec![mk("api", 800), mk("api", 400)], true))]);
    assert_eq!(times(&take_ready(&mut p)), vec![900, 800, 500]);
    assert_eq!(times(&p["api"].pending), vec![400], "held back for a later page");
    assert!(p["web"].pending.is_empty());
}

#[test]
fn skewed_commit_sinks_to_its_date() {
    let mut p = map(vec![("web", prog(vec![mk("web", 100), mk("web", 1)], false)), ("api", prog(vec![mk("api", 90), mk("api", 80)], false))]);
    assert_eq!(times(&take_ready(&mut p)), vec![100, 90, 80]);
    assert_eq!(times(&p["web"].pending), vec![1]);
}

#[test]
fn unrefilled_open_repo_holds_everything() {
    // Not exhausted and nothing pending: where its history continues is unknown.
    let mut p = map(vec![("web", RepoProgress { fetched: 4, pending: vec![], exhausted: false }), ("api", prog(vec![mk("api", 10)], true))]);
    assert!(take_ready(&mut p).is_empty());
    assert_eq!(p["api"].pending.len(), 1, "held, not dropped");
}

/// A fake `git log --skip=<fetched> --max-count=<page>` over a repo's walk, and the page
/// loop the caller runs: refill repos that may have more and are short of a page, then take.
fn page(walks: &BTreeMap<String, Vec<Commit>>, progress: &mut BTreeMap<String, RepoProgress>, size: usize) -> (Vec<Commit>, bool) {
    loop {
        for (id, p) in progress.iter_mut() {
            if p.exhausted || p.pending.len() >= size {
                continue;
            }
            let walk = &walks[id];
            let got: Vec<Commit> = walk.iter().skip(p.fetched).take(size).cloned().collect();
            p.fetched += got.len();
            if got.len() < size {
                p.exhausted = true;
            }
            p.pending.extend(got);
        }
        let rows = take_ready(progress);
        let done = progress.values().all(|p| p.exhausted && p.pending.is_empty());
        if !rows.is_empty() || done {
            return (rows, done);
        }
    }
}

#[test]
fn horizon_no_dup_no_gap() {
    // One busy repo and two quiet ones; seconds are shared within and across repos.
    let busy: Vec<Commit> = [100, 100, 95, 90, 90, 90, 80, 75, 70, 70].iter().map(|&t| mk("busy", t)).collect();
    let quiet_a: Vec<Commit> = [100, 90, 70].iter().map(|&t| mk("quiet-a", t)).collect();
    let quiet_b: Vec<Commit> = [95, 75].iter().map(|&t| mk("quiet-b", t)).collect();
    let walks: BTreeMap<String, Vec<Commit>> = [("busy", busy), ("quiet-a", quiet_a), ("quiet-b", quiet_b)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let mut progress: BTreeMap<String, RepoProgress> = walks.keys().map(|k| (k.clone(), RepoProgress::default())).collect();

    let mut all = Vec::new();
    let mut pages = 0;
    loop {
        let (rows, done) = page(&walks, &mut progress, 4);
        pages += 1;
        let mut sorted = rows.clone();
        sorted.sort_by(compare_commits);
        assert_eq!(rows, sorted, "each page is in order");
        all.extend(rows);
        if done {
            break;
        }
        assert!(pages < 10, "paging terminates");
    }
    assert_eq!(pages, 3, "three pages");

    let mut expected: Vec<Commit> = walks.values().flatten().cloned().collect();
    expected.sort_by(compare_commits);
    let seen: HashSet<_> = all.iter().map(|c| (c.repo.clone(), c.sha.clone())).collect();
    assert_eq!(seen.len(), all.len(), "no commit twice");
    assert_eq!(all.len(), expected.len(), "no commit missing");
    assert_eq!(times(&all), times(&expected), "newest first across page boundaries");
    // Within a second, the sha order holds per page only: rows at the horizon are shown, and
    // the busy repo's next page holds more commits from that same second.
    let mut a: Vec<_> = all.iter().map(|c| (c.repo.clone(), c.sha.clone())).collect();
    let mut e: Vec<_> = expected.iter().map(|c| (c.repo.clone(), c.sha.clone())).collect();
    a.sort();
    e.sort();
    assert_eq!(a, e, "exactly the commits git walked");
}
