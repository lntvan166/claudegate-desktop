//! The performance targets on a workspace the size of the maintainer's: 70 repositories of
//! 300 commits each. Ignored by default; run it explicitly:
//! `cargo test -p claudegate-desktop --test gitlog_perf -- --ignored --nocapture`
mod gitfix;

use claudegate_desktop_lib::gitlog::{FilterView, GitLog, Runner};
use gitfix::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const REPOS: usize = 70;
const COMMITS: usize = 300;

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

#[test]
#[ignore]
fn first_page_and_changes_within_targets() {
    let (_t, root) = tmp();
    let built = Instant::now();
    for r in 0..REPOS {
        // Interleaved histories: every repository has commits throughout the range.
        bulk(&root.join(format!("group{}/acme-svc-{r:02}", r % 5)), COMMITS, |i| BASE - (i as i64) * 900 - r as i64 * 7);
    }
    println!("fixture: {REPOS} repos x {COMMITS} commits built in {:?}", built.elapsed());

    let g = GitLog::new(Runner::new(PathBuf::from("git")));
    let t = Instant::now();
    let repos = g.repos_for(&[root.clone()], 2, &[], true);
    println!("discovery (excluded from the target): {:?}", t.elapsed());
    assert_eq!(repos.len(), REPOS);
    let ids: Vec<String> = repos.iter().map(|r| r.id.clone()).collect();

    // Warm the page cache once, as the target is "on a warm cache".
    g.page("main", 1, &ids, &FilterView::default(), false).unwrap();

    let mut firsts = Vec::new();
    let mut last = None;
    for q in 2..7 {
        let t = Instant::now();
        let p = g.page("main", q, &ids, &FilterView::default(), false).unwrap();
        firsts.push(t.elapsed());
        assert_eq!(p.rows.len(), 200);
        assert!(p.failures.is_empty());
        last = Some(p);
    }
    let t = Instant::now();
    let more = g.page("main", 6, &ids, &FilterView::default(), true).unwrap();
    let more_t = t.elapsed();
    assert_eq!(more.rows.len(), 200);

    let within = FilterView { within: Some(7 * 86_400), search: Some("change 1".into()), ..Default::default() };
    let t = Instant::now();
    g.page("main", 50, &ids, &within, false).unwrap();
    let filtered_t = t.elapsed();

    let row = &last.unwrap().rows[17];
    let mut changes = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let c = g.changes(&row.repo, &row.sha).unwrap();
        changes.push(t.elapsed());
        assert_eq!(c.len(), 1);
    }
    let (first, ch) = (median(firsts.clone()), median(changes.clone()));
    println!("first page: median {first:?} (runs {firsts:?})");
    println!("load more: {more_t:?}");
    println!("first page, search + 7 days: {filtered_t:?}");
    println!("changes: median {ch:?} (runs {changes:?})");
    assert!(first < Duration::from_millis(300), "first page {first:?} over 300 ms");
    assert!(ch < Duration::from_millis(100), "changes {ch:?} over 100 ms");
}
