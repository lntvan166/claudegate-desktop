//! A three-way line merge with `git merge-file`'s rule for clean versus conflict: changes to
//! overlapping or adjacent base lines conflict unless both sides made the same change. Combined
//! Changes uses it to take selected commits back out of a file, in memory. It is ours rather than
//! `git merge-file` because git's would write objects into the user's `.git`.
//!
//! Ported line for line from Polylog's `lineMerge.ts`; the oracle test pins its alignment.

/// The outcome of [`merge3`]. `TooLarge`: more than [`MAX_EDITS`] edits between base and a side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    Clean(String),
    Conflict,
    TooLarge,
}

/// More edited lines than this between two versions is not worth a merge: Myers' trace grows with
/// the square of the edits. The file is shown as one plain diff instead.
pub const MAX_EDITS: usize = 2000;

/// Lines with their terminators, so joining them gives back the exact bytes (CRLF, no final
/// newline).
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// One change against base: `base[start, end)` becomes `lines`.
struct Hunk<'a> {
    start: usize,
    end: usize,
    lines: Vec<&'a str>,
}

/// Myers' O(ND) diff of `a` against `b`, as hunks over `a`; `None` when it needs more than
/// [`MAX_EDITS`] edits.
fn diff<'a>(a: &[&'a str], b: &[&'a str]) -> Option<Vec<Hunk<'a>>> {
    let n = a.len() as isize;
    let m = b.len() as isize;
    let max = (n + m).min(MAX_EDITS as isize);
    let off = max + 1;
    let mut v = vec![0isize; (2 * max + 3) as usize];
    // trace[d]: v for k in [-(d+1), d+1] as step d started (what backtracking reads).
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let at = |t: &[isize], d: isize, k: isize| if k >= -(d + 1) && k <= d + 1 { t[(k + d + 1) as usize] } else { 0 };
    let mut done = false;
    let mut d = 0;
    while d <= max && !done {
        trace.push(v[(off - d - 1) as usize..(off + d + 2) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[(off + k - 1) as usize] < v[(off + k + 1) as usize]) {
                v[(off + k + 1) as usize]
            } else {
                v[(off + k - 1) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[(off + k) as usize] = x;
            if x >= n && y >= m {
                done = true;
                break;
            }
            k += 2;
        }
        d += 1;
    }
    if !done {
        return None;
    }
    // Walk back to the edit path: a list of kept (a, b) pairs.
    let mut kept: Vec<(isize, isize)> = Vec::new();
    let mut x = n;
    let mut y = m;
    let mut d = trace.len() as isize - 1;
    while d >= 0 && (x > 0 || y > 0) {
        let t = &trace[d as usize];
        let k = x - y;
        let prev_k = if k == -d || (k != d && at(t, d, k - 1) < at(t, d, k + 1)) { k + 1 } else { k - 1 };
        let prev_x = at(t, d, prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            kept.push((x, y));
        }
        if d > 0 {
            x = prev_x;
            y = prev_y;
        }
        d -= 1;
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        kept.push((x, y));
    }
    kept.reverse();
    kept.push((n, m));
    let mut hunks = Vec::new();
    let mut ai = 0isize;
    let mut bi = 0isize;
    for (ka, kb) in kept {
        if ka > ai || kb > bi {
            hunks.push(Hunk { start: ai as usize, end: ka as usize, lines: b[bi as usize..kb as usize].to_vec() });
        }
        ai = ka + 1;
        bi = kb + 1;
    }
    Some(hunks)
}

/// One side's text for `base[start, end)`, with its hunks inside that range applied.
fn side_text<'a>(base: &[&'a str], hunks: &[&Hunk<'a>], start: usize, end: usize) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = start;
    for h in hunks {
        out.extend_from_slice(&base[at..h.start]);
        out.extend_from_slice(&h.lines);
        at = h.end;
    }
    out.extend_from_slice(&base[at..end]);
    out
}

/// Merge `ours` and `theirs`, both edits of `base`. Lines compare as `&str` slices, terminators
/// included.
pub fn merge3(base: &str, ours: &str, theirs: &str) -> MergeResult {
    let b = lines(base);
    let Some(mine) = diff(&b, &lines(ours)) else {
        return MergeResult::TooLarge;
    };
    let Some(other) = diff(&b, &lines(theirs)) else {
        return MergeResult::TooLarge;
    };
    let sides = [mine, other];
    let mut out: Vec<&str> = Vec::new();
    let mut at = 0;
    let mut next = [0usize, 0usize];
    loop {
        // The next hunk on either side, then every hunk that overlaps or touches the group.
        let a = sides[0].get(next[0]);
        let t = sides[1].get(next[1]);
        if a.is_none() && t.is_none() {
            break;
        }
        let first = match (a, t) {
            (_, None) => 0,
            (Some(a), Some(t)) if a.start <= t.start => 0,
            _ => 1,
        };
        let mut group: [Vec<&Hunk>; 2] = [Vec::new(), Vec::new()];
        let head = &sides[first][next[first]];
        next[first] += 1;
        group[first].push(head);
        let mut start = head.start;
        let mut end = head.end;
        let mut grew = true;
        while grew {
            grew = false;
            for s in 0..2 {
                if let Some(h) = sides[s].get(next[s]) {
                    if h.start <= end {
                        group[s].push(h);
                        next[s] += 1;
                        start = start.min(h.start);
                        end = end.max(h.end);
                        grew = true;
                    }
                }
            }
        }
        out.extend_from_slice(&b[at..start]);
        let ours_text = side_text(&b, &group[0], start, end);
        let theirs_text = side_text(&b, &group[1], start, end);
        if group[0].is_empty() {
            out.extend(theirs_text);
        } else if group[1].is_empty() || ours_text.concat() == theirs_text.concat() {
            out.extend(ours_text);
        } else {
            return MergeResult::Conflict;
        }
        at = end;
    }
    out.extend_from_slice(&b[at..]);
    MergeResult::Clean(out.concat())
}
