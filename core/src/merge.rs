//! Port of the extension's merge and decision functions (`reviewModel.ts`, v1.16.1):
//! `mergeFreshCaptures`, `latestDecisionMs`, `acceptEntry` and `rejectEntry`.
//!
//! Timestamps go through `time::parse_iso_ms`, our `Date.parse`. Where the TS gets `NaN`
//! we get `None`, and every comparison treats it the way the TS treats `NaN`: unknown.
//! Pure functions; nothing here touches the disk.

use crate::model::{cap_accepted_log, make_record_id, ReviewRecord, Session};
use crate::time::parse_iso_ms;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Records and paths the caller removed from `mine` in this same persist cycle. The disk copy
/// still holds them, so without these tombstones the merge would resurrect them.
#[derive(Debug, Clone, Default)]
pub struct MergeGuards {
    /// path → `capturedAt` of a just-pruned pending entry (`None` = it had none).
    pub pruned_files: HashMap<String, Option<String>>,
    pub dropped_accepted_ids: HashSet<String>,
    pub dropped_rejected_paths: HashSet<String>,
}

/// Merge what other writers (the hook, another window) put on disk since we loaded into `mine`.
pub fn merge_fresh_captures(mut mine: Session, disk: Session, guards: &MergeGuards) -> Session {
    // A one-way latch.
    if disk.exclude_cleanup_done {
        mine.exclude_cleanup_done = true;
    }

    // accepted[] is append-only: union by id, then sort oldest-first and re-cap.
    let mut seen: HashSet<String> = mine.accepted.iter().map(|r| r.id.clone()).collect();
    let mut unioned = false;
    for rec in disk.accepted {
        if seen.contains(&rec.id) || guards.dropped_accepted_ids.contains(&rec.id) {
            continue;
        }
        seen.insert(rec.id.clone());
        mine.accepted.push(rec);
        unioned = true;
    }
    if unioned {
        sort_oldest_first(&mut mine.accepted);
        cap_accepted_log(&mut mine);
    }

    // rejected{} is latest-per-path: take disk's when we hold none, or when it is newer.
    for (path, rec) in disk.rejected {
        if guards.dropped_rejected_paths.contains(&path) {
            continue;
        }
        let take = match mine.rejected.get(&path) {
            None => true,
            Some(ours) => match (parse_iso_ms(&ours.decided_at), parse_iso_ms(&rec.decided_at)) {
                (_, None) => false,
                (None, Some(_)) => true,
                (Some(o), Some(d)) => d > o,
            },
        };
        if take {
            mine.rejected.insert(path, rec);
        }
    }

    // Disk captures we have not seen, unless a decision strictly newer supersedes them.
    for (path, entry) in disk.files {
        if mine.files.contains_key(&path) {
            continue;
        }
        // `!entry.capturedAt`: missing or empty → cannot prove it is a real capture.
        let Some(captured_at) = entry.captured_at.as_deref().filter(|c| !c.is_empty()) else {
            continue;
        };
        if guards.pruned_files.get(&path).is_some_and(|p| p.as_deref() == Some(captured_at)) {
            continue;
        }
        let Some(captured) = parse_iso_ms(captured_at) else { continue };
        // `captured >= decidedAtMs`, with no decision being -Infinity. A tie keeps the capture.
        if latest_decision_ms(&mine, &path).is_none_or(|d| captured >= d) {
            mine.files.insert(path, entry);
        }
    }

    // Drop our pending entries that someone has since decided (strictly newer decision).
    let superseded: Vec<String> = mine
        .files
        .iter()
        .filter(|(path, e)| {
            let Some(captured) = e.captured_at.as_deref().and_then(parse_iso_ms) else {
                return false;
            };
            latest_decision_ms(&mine, path).is_some_and(|d| d > captured)
        })
        .map(|(p, _)| p.clone())
        .collect();
    for p in superseded {
        mine.files.remove(&p);
    }
    mine
}

/// The newest accept or reject we hold for `path`, in ms. `None` is the TS `-Infinity`.
fn latest_decision_ms(s: &Session, path: &str) -> Option<i64> {
    s.accepted
        .iter()
        .filter(|r| r.path == path)
        .chain(s.rejected.get(path))
        .filter_map(|r| parse_iso_ms(&r.decided_at))
        .max()
}

/// The TS comparator: by `decidedAt`, with an unparseable time (or a tie) comparing equal.
fn compare(a: Option<i64>, b: Option<i64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => Ordering::Equal,
    }
}

/// `accepted.sort(comparator)`, stable like V8's.
///
/// When every `decidedAt` parses (or none does) the comparator is a total preorder, and every
/// stable sort gives V8's result, so `sort_by` is used. A mix is not a total order: the result
/// then depends on the algorithm, and Rust's `sort_by` may panic on it. For that case this runs
/// V8's TimSort up to its first merge (first run, then binary insertion), which is V8's exact
/// algorithm for fewer than 64 elements. Beyond that it is deterministic but may order the
/// unparseable records differently from V8.
fn sort_oldest_first(recs: &mut Vec<ReviewRecord>) {
    let keys: Vec<Option<i64>> = recs.iter().map(|r| parse_iso_ms(&r.decided_at)).collect();
    let mut idx: Vec<usize> = (0..recs.len()).collect();
    let parsed = keys.iter().filter(|k| k.is_some()).count();
    if parsed == keys.len() || parsed == 0 {
        idx.sort_by(|&a, &b| compare(keys[a], keys[b]));
    } else {
        v8_small_sort(&mut idx, |a, b| compare(keys[a], keys[b]));
    }
    let mut slots: Vec<Option<ReviewRecord>> = recs.drain(..).map(Some).collect();
    recs.extend(idx.into_iter().map(|i| slots[i].take().expect("each index once")));
}

/// V8 `array-sort.tq`: `CountAndMakeRun` from the start, then `BinaryInsertionSort` over the rest.
fn v8_small_sort<T: Copy>(a: &mut [T], cmp: impl Fn(T, T) -> Ordering) {
    let n = a.len();
    if n < 2 {
        return;
    }
    // CountAndMakeRun(0, n)
    let mut run = 2;
    let descending = cmp(a[1], a[0]) == Ordering::Less;
    let mut prev = a[1];
    for &cur in &a[2..] {
        let order = cmp(cur, prev);
        if (descending && order != Ordering::Less) || (!descending && order == Ordering::Less) {
            break;
        }
        prev = cur;
        run += 1;
    }
    if descending {
        a[..run].reverse();
    }
    // BinaryInsertionSort(0, run, n)
    for start in run..n {
        let pivot = a[start];
        let (mut left, mut right) = (0, start);
        while left < right {
            let mid = left + (right - left) / 2;
            if cmp(pivot, a[mid]) == Ordering::Less {
                right = mid;
            } else {
                left = mid + 1;
            }
        }
        a.copy_within(left..start, left + 1);
        a[left] = pivot;
    }
}

/// Port of `acceptEntry`: log the accept, drop the pending entry and any prior reject.
pub fn accept_entry(s: &mut Session, path: &str, after: Option<String>, decided_at: &str) {
    let Some(entry) = s.files.remove(path) else { return };
    s.accepted.push(ReviewRecord {
        id: make_record_id(decided_at, path),
        path: path.to_owned(),
        before: entry.original_content,
        after,
        decided_at: decided_at.to_owned(),
        session_id: entry.session_id,
        new_file: entry.new_file,
        reason: None,
    });
    cap_accepted_log(s);
    s.rejected.remove(path);
}

/// Port of `rejectEntry` without a reason (v1 records none).
pub fn reject_entry(s: &mut Session, path: &str, after: Option<String>, decided_at: &str) {
    let Some(entry) = s.files.remove(path) else { return };
    s.rejected.insert(
        path.to_owned(),
        ReviewRecord {
            id: make_record_id(decided_at, path),
            path: path.to_owned(),
            before: entry.original_content,
            after,
            decided_at: decided_at.to_owned(),
            session_id: entry.session_id,
            new_file: entry.new_file,
            reason: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::v8_small_sort;

    #[test]
    fn v8_sort_is_stable_and_reverses_strict_descending_runs() {
        let mut v = [(3, 'a'), (2, 'b'), (1, 'c'), (2, 'd'), (1, 'e')];
        v8_small_sort(&mut v, |x, y| x.0.cmp(&y.0));
        assert_eq!(v, [(1, 'c'), (1, 'e'), (2, 'b'), (2, 'd'), (3, 'a')]);
    }
}
