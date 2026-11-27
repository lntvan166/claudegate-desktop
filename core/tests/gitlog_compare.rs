//! Compare Branches, ported from Polylog's `compareModel.test.ts` with the same values. The arg
//! builders differ where this app's lessons apply: `--end-of-options`, a trailing `--`, no
//! signatures, textconv or external diff, and NUL fields for the side log.
use claudegate_core::gitlog::compare::{
    both_paths, branch_refs_args, files_args, merge_base_args, pair_duplicates, parse_branch_refs, parse_diff_z, parse_rev_parse, parse_side_count, parse_side_log, rev_parse_args, side_count_args, side_log_args, tip_paths,
    valid_pair, Duplicate, Pair, Side, SideCommit, COMMIT_PAGE, SIDE_FORMAT,
};
use claudegate_core::gitlog::{ChangedFile, FileStatus};

fn rep(c: char) -> String {
    c.to_string().repeat(40)
}
fn s(x: &str) -> String {
    x.to_string()
}
fn pair(l: &str, r: &str) -> Pair {
    Pair { left: s(l), right: s(r) }
}

#[test]
fn a_pair_is_two_branch_names() {
    assert!(valid_pair(&pair("origin/release-1.4", "main")));
    for bad in ["-x", "a..b", "HEAD~3", "", "x y", "re^{commit}"] {
        assert!(!valid_pair(&pair(bad, "main")), "{bad:?}");
        assert!(!valid_pair(&pair("main", bad)), "{bad:?}");
    }
}

#[test]
fn rev_parse_same_tree_is_identical() {
    let (a, b) = (rep('a'), rep('b'));
    let (t1, t2) = (rep('1'), rep('2'));
    assert_eq!(
        rev_parse_args(&pair("origin/release-1.4", "main")),
        ["rev-parse", "--end-of-options", "origin/release-1.4^{commit}", "main^{commit}", "origin/release-1.4^{tree}", "main^{tree}", "--"]
    );
    assert_eq!(parse_rev_parse(&format!("{a}\n{b}\n{t1}\n{t2}\n")), Some((a.clone(), b.clone(), false)));
    assert_eq!(parse_rev_parse(&format!("{a}\n{b}\n{t1}\n{t1}\n")), Some((a.clone(), b.clone(), true)));
    // git echoes the separators rev-parse is given.
    assert_eq!(parse_rev_parse(&format!("--end-of-options\n{a}\n{b}\n{t1}\n{t2}\n--\n")), Some((a.clone(), b.clone(), false)));
    assert_eq!(parse_rev_parse(&format!("{a}\n{b}\n")), None);
    assert_eq!(parse_rev_parse("nope\nzz\nyy\nxx\n"), None);
    assert_eq!(parse_rev_parse(&format!("{a}\n{b}\n{t1}\n{t2}\n{t2}\n")), None, "five ids is not an answer");
}

#[test]
fn side_counts() {
    let (a, b) = (rep('a'), rep('b'));
    assert_eq!(merge_base_args(&a, &b), ["merge-base", "--end-of-options", a.as_str(), b.as_str()]);
    let range = format!("{a}...{b}");
    assert_eq!(side_count_args(Side::Left, &a, &b), ["rev-list", "--left-only", "--cherry-mark", "--no-merges", "--count", range.as_str(), "--"]);
    assert_eq!(side_count_args(Side::Right, &a, &b), ["rev-list", "--right-only", "--cherry-mark", "--no-merges", "--count", range.as_str(), "--"]);
    assert_eq!(parse_side_count("2\t1\n"), (2, 1));
    assert_eq!(parse_side_count("3\n"), (3, 0));
    assert_eq!(parse_side_count(""), (0, 0));
}

fn files_fixture() -> String {
    [
        ":000000 100644 0000000 1111111 A",
        "src/limit.go",
        ":100644 100644 2222222 3333333 R090",
        "src/old.go",
        "src/new.go",
        ":100644 100644 4444444 5555555 M",
        "bin.png",
        "40\t0\tsrc/limit.go",
        "1\t1\t",
        "src/old.go",
        "src/new.go",
        "-\t-\tbin.png",
        "",
    ]
    .join("\0")
}

fn cf(path: &str, status: FileStatus, added: Option<u32>, removed: Option<u32>) -> ChangedFile {
    ChangedFile { path: s(path), status, added, removed }
}

#[test]
fn files_parse_status_rename_counts() {
    let (a, c) = (rep('a'), rep('c'));
    assert_eq!(files_args(&c, &a), ["diff", "--raw", "--numstat", "-z", "-M", "--no-ext-diff", "--no-textconv", c.as_str(), a.as_str(), "--"]);
    assert_eq!(
        parse_diff_z(&files_fixture()),
        [
            cf("src/limit.go", FileStatus::Added, Some(40), Some(0)),
            cf("src/new.go", FileStatus::Renamed { from: s("src/old.go") }, Some(1), Some(1)),
            cf("bin.png", FileStatus::Modified, None, None),
        ]
    );
    assert_eq!(parse_diff_z(""), []);
}

#[test]
fn both_matches_new_path() {
    let files = parse_diff_z(&files_fixture());
    let other = parse_diff_z(&["1\t1\tsrc/new.go", "2\t0\tREADME.md", ""].join("\0"));
    assert_eq!(both_paths(&files, &other), ["src/new.go"]);
    let mut more = other.clone();
    more.push(cf("bin.png", FileStatus::Modified, None, None));
    assert_eq!(both_paths(&files, &more), ["bin.png", "src/new.go"], "sorted");
}

#[test]
fn tip_paths_follow_a_rename() {
    assert_eq!(tip_paths(Side::Left, "rate.go", Some("limit.go")), (s("rate.go"), s("limit.go")));
    assert_eq!(tip_paths(Side::Right, "rate.go", Some("limit.go")), (s("limit.go"), s("rate.go")));
    assert_eq!(tip_paths(Side::Left, "a.go", None), (s("a.go"), s("a.go")));
}

#[test]
fn side_log_marks_duplicates() {
    let (a, b, c) = (rep('a'), rep('b'), rep('c'));
    let range = format!("{a}...{b}");
    assert_eq!(
        side_log_args(Side::Left, &a, &b, COMMIT_PAGE),
        ["log", "--no-show-signature", "--left-only", "--cherry-mark", "--no-merges", "--max-count=500", SIDE_FORMAT, range.as_str(), "--"]
    );
    assert_eq!(side_log_args(Side::Right, &a, &b, 7)[2], "--right-only");
    let out = format!("<\0{a}\0{c}\01758000300\0dana\0feat: rate limit\0\n=\0{b}\0{c} {a}\01758000200\0rin\0fix: retry on 503\0\n");
    assert_eq!(
        parse_side_log(&out),
        [
            SideCommit { mark: '+', sha: a.clone(), parents: vec![c.clone()], time: 1758000300, author: s("dana"), subject: s("feat: rate limit") },
            SideCommit { mark: '=', sha: b.clone(), parents: vec![c.clone(), a.clone()], time: 1758000200, author: s("rin"), subject: s("fix: retry on 503") },
        ]
    );
    // A record cut off by a killed git is dropped.
    assert_eq!(parse_side_log(&format!("<\0{a}\0{c}\01758000300\0dana\0feat")).len(), 0);
}

#[test]
fn duplicates_pair_by_subject() {
    let (a, b, c) = (rep('a'), rep('b'), rep('c'));
    let sc = |sha: &str, time: i64, author: &str, subject: &str| SideCommit { mark: '=', sha: s(sha), parents: vec![], time, author: s(author), subject: s(subject) };
    let dl = [sc(&a, 2, "dana", "fix: retry on 503"), sc(&c, 1, "dana", "fix: squash")];
    let dr = [sc(&b, 3, "rin", "fix: retry on 503")];
    assert_eq!(
        pair_duplicates(&dl, &dr),
        [Duplicate { subject: s("fix: retry on 503"), left: Some(a.clone()), right: Some(b.clone()) }, Duplicate { subject: s("fix: squash"), left: Some(c.clone()), right: None }]
    );
    // A right one without a partner stays on its side.
    assert_eq!(pair_duplicates(&[], &dr), [Duplicate { subject: s("fix: retry on 503"), left: None, right: Some(b.clone()) }]);
}

#[test]
fn branch_refs_skip_head() {
    assert_eq!(branch_refs_args(), ["for-each-ref", "--format=%(refname)", "refs/heads", "refs/remotes"]);
    let out = "refs/heads/main\nrefs/heads/feat/billing\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/remotes/origin/release-1.4\n";
    let (names, remotes) = parse_branch_refs(out);
    assert_eq!(names, ["main", "feat/billing", "origin/main", "origin/release-1.4"]);
    assert_eq!(remotes, ["origin"]);
    assert_eq!(parse_branch_refs(""), (vec![], vec![]));
}

#[test]
fn parse_diff_z_hostile_names() {
    let out = [
        ":100644 100644 1111111 2222222 R100",
        "a\nb.go",
        "c\x1ed.go",
        ":000000 100644 0000000 3333333 A",
        "-x.go",
        ":100644 100644 4444444 5555555 M",
        "1\t2\tz.go",
        "0\t0\t",
        "a\nb.go",
        "c\x1ed.go",
        "7\t0\t-x.go",
        "3\t4\t1\t2\tz.go",
        "",
    ]
    .join("\0");
    assert_eq!(
        parse_diff_z(&out),
        [
            cf("c\x1ed.go", FileStatus::Renamed { from: s("a\nb.go") }, Some(0), Some(0)),
            cf("-x.go", FileStatus::Added, Some(7), Some(0)),
            cf("1\t2\tz.go", FileStatus::Modified, Some(3), Some(4)),
        ]
    );
}

#[test]
fn side_log_hostile_subject() {
    let a = rep('a');
    let out = format!("<\0{a}\0\01758000300\0dana\0x\x1fy\x1ez\0\n");
    let log = parse_side_log(&out);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].subject, "x\x1fy\x1ez");
    assert!(log[0].parents.is_empty());
}
