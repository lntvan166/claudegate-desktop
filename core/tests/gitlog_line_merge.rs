use claudegate_core::gitlog::{merge3, MergeResult, MAX_EDITS};
use std::process::Command;
use std::time::Instant;

#[test]
fn separated_changes_merge() {
    assert_eq!(merge3("a\nb\nc\nd\ne\n", "a\nB\nc\nd\ne\n", "a\nb\nc\nd\nE\n"), MergeResult::Clean("a\nB\nc\nd\nE\n".into()));
}

#[test]
fn adjacent_changes_conflict() {
    assert_eq!(merge3("a\nb\nc\n", "a\nB\nc\n", "a\nb\nC\n"), MergeResult::Conflict);
}

#[test]
fn overlapping_conflict_unless_identical() {
    assert_eq!(merge3("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n"), MergeResult::Conflict);
    assert_eq!(merge3("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n"), MergeResult::Clean("a\nX\nc\n".into()));
}

#[test]
fn one_side_changed_takes_that_side() {
    assert_eq!(merge3("a\nb\n", "a\nb\n", "a\nB\n"), MergeResult::Clean("a\nB\n".into()));
    assert_eq!(merge3("a\nb\n", "a\nB\n", "a\nb\n"), MergeResult::Clean("a\nB\n".into()));
}

#[test]
fn empty_sides() {
    assert_eq!(merge3("", "x\n", ""), MergeResult::Clean("x\n".into()));
    assert_eq!(merge3("", "", "y\n"), MergeResult::Clean("y\n".into()));
    assert_eq!(merge3("x\n", "x\n", ""), MergeResult::Clean("".into()));
}

#[test]
fn no_trailing_newline_and_crlf_keep_bytes() {
    assert_eq!(merge3("a\nb", "a\nb", "a\nB"), MergeResult::Clean("a\nB".into()));
    assert_eq!(merge3("a\r\nb\r\n", "a\r\nb\r\n", "a\r\nB\r\n"), MergeResult::Clean("a\r\nB\r\n".into()));
}

/// The oracle: `git merge-file -p ours base theirs` on scratch files in a fresh temp dir, never
/// in a repository. `None` when it could not be run. `Some(Some(text))` is clean, `Some(None)` a
/// conflict.
fn oracle(base: &str, ours: &str, theirs: &str) -> Option<Option<String>> {
    let dir = tempfile::tempdir().unwrap();
    for (n, t) in [("base", base), ("ours", ours), ("theirs", theirs)] {
        std::fs::write(dir.path().join(n), t).unwrap();
    }
    let mut cmd = Command::new("git");
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(&k);
        }
    }
    let out = cmd.args(["merge-file", "-p", "ours", "base", "theirs"]).current_dir(dir.path()).output().ok()?;
    if out.status.success() {
        Some(Some(String::from_utf8(out.stdout).unwrap()))
    } else {
        Some(None)
    }
}

fn git_available() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

struct Lcg(u64);

impl Lcg {
    fn rand(&mut self, n: u64) -> u64 {
        self.0 = (self.0 * 1103515245 + 12345) % 2147483648;
        self.0 % n
    }
}

fn edit(r: &mut Lcg, lines: &[String]) -> Vec<String> {
    let mut out = lines.to_vec();
    let i = r.rand(out.len() as u64 + 1) as usize;
    let op = r.rand(3);
    if op == 0 {
        let n = r.rand(100);
        out.insert(i, format!("new{n}"));
    } else if op == 1 && !out.is_empty() {
        let at = i.min(out.len() - 1);
        out.remove(at);
    } else if !out.is_empty() {
        let at = i.min(out.len() - 1);
        out[at] = format!("chg{}", r.rand(100));
    }
    out
}

fn text(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

#[test]
fn agrees_with_git_merge_file() {
    if !git_available() {
        eprintln!("SKIP agrees_with_git_merge_file: git is not on PATH");
        return;
    }
    let mut cases: Vec<(String, String, String)> = [
        ("a\nb\nc\nd\ne\n", "a\nB\nc\nd\ne\n", "a\nb\nc\nd\nE\n"),
        ("a\nb\nc\n", "a\nB\nc\n", "a\nb\nC\n"),
        ("a\nb\nc\n", "a\nX\nc\n", "a\nY\nc\n"),
        ("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n"),
        ("", "x\n", ""),
        ("x\n", "x\n", ""),
        ("a\nb", "a\nb", "a\nB"),
    ]
    .iter()
    .map(|(b, o, t)| (b.to_string(), o.to_string(), t.to_string()))
    .collect();
    let mut r = Lcg(7);
    for _ in 0..40 {
        let len = 4 + r.rand(6) as usize;
        let base: Vec<String> = (0..len).map(|j| format!("l{j}")).collect();
        let ours = edit(&mut r, &base);
        let theirs = edit(&mut r, &base);
        cases.push((text(&base), text(&ours), text(&theirs)));
    }
    let (mut clean, mut conflict) = (0, 0);
    for (b, o, t) in &cases {
        let want = oracle(b, o, t).expect("git merge-file could not be run");
        let got = merge3(b, o, t);
        match (&got, &want) {
            (MergeResult::Clean(g), Some(w)) => {
                assert_eq!(g, w, "text differs from git for {:?}", (b, o, t));
                clean += 1;
            }
            (MergeResult::Conflict, None) => conflict += 1,
            _ => panic!("clean/conflict differs from git for {:?}: got {got:?}, git {want:?}", (b, o, t)),
        }
    }
    eprintln!("agrees with git merge-file on {} cases ({clean} clean, {conflict} conflict)", cases.len());
    assert_eq!(cases.len(), 47);
}

#[test]
fn huge_rewrite_is_too_large() {
    let n = 6000;
    let base: String = (0..n).map(|i| format!("line {i}\n")).collect();
    let ours: String = (0..n).map(|i| format!("other {i}\n")).collect();
    let start = Instant::now();
    let r = merge3(&base, &ours, &base);
    let ms = start.elapsed().as_millis();
    eprintln!("huge rewrite refused in {ms} ms (MAX_EDITS = {MAX_EDITS})");
    assert_eq!(r, MergeResult::TooLarge);
    assert!(ms < 1500, "took {ms} ms");
}
