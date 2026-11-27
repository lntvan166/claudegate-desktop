//! Merging per-repository pages into one log, newest first, at a horizon.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::parse::Commit;

/// One repository's place in the merged log for the life of one query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoProgress {
    /// Records consumed from git's walk so far: the next page's `--skip`.
    pub fetched: usize,
    /// Fetched but not shown yet: held back by the horizon.
    pub pending: Vec<Commit>,
    /// git returned a short page: nothing more to fetch.
    pub exhausted: bool,
}

/// Time descending, then sha ascending, then repo, so the order is total and stable.
pub fn compare_commits(a: &Commit, b: &Commit) -> Ordering {
    b.time.cmp(&a.time).then_with(|| a.sha.cmp(&b.sha)).then_with(|| a.repo.cmp(&b.repo))
}

/// Moves every pending commit at or above the horizon into the returned rows, newest first.
///
/// The horizon is the newest "oldest pending commit" among repositories that may still have
/// unfetched history: nothing below it can be shown yet, or that history could later land
/// above it. Rows below the horizon stay pending, held and never dropped, so a commit with a
/// skewed clock sinks to its date instead of truncating its repository.
///
/// The caller refills every repository that is not exhausted and has nothing pending before
/// calling this. One that is left empty says nothing about where its history continues, so
/// it holds every row back rather than risk a gap.
pub fn take_ready(progress: &mut BTreeMap<String, RepoProgress>) -> Vec<Commit> {
    let mut horizon = i64::MIN;
    for p in progress.values().filter(|p| !p.exhausted) {
        let oldest = p.pending.iter().map(|c| c.time).min().unwrap_or(i64::MAX);
        horizon = horizon.max(oldest);
    }
    if horizon == i64::MAX {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for p in progress.values_mut() {
        let (ready, held): (Vec<Commit>, Vec<Commit>) = std::mem::take(&mut p.pending).into_iter().partition(|c| c.time >= horizon);
        rows.extend(ready);
        p.pending = held;
    }
    rows.sort_by(compare_commits);
    rows
}
