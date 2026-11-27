//! Combined Changes, ported from Polylog's `combinedModel.test.ts` and `combinedRead.test.ts`
//! with the same fixtures and expected values.
use claudegate_core::gitlog::{
    combine_repo, exclude_for, left_side, merge3, merge_base_args, par_map_limit, parse_range_log, range_log_args, read_combined, show_raw_args, Blob, CombinedFile, Fallback, GitRead, LeftSide, MergeResult, Mode, NetStatus, RangeCommit, RawChange, ReadError, RANGE_FORMAT,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

fn z() -> String {
    "0".repeat(40)
}
fn blob(n: u32) -> String {
    format!("{n:b>40}")
}
fn s(x: &str) -> String {
    x.to_string()
}
fn rep(c: char) -> String {
    c.to_string().repeat(40)
}
fn ch(status: char, path: &str, old_blob: &str, new_blob: &str, old_path: Option<&str>) -> RawChange {
    RawChange { status, path: s(path), old_path: old_path.map(s), old_blob: s(old_blob), new_blob: s(new_blob) }
}
fn commit(sha: &str, parents: &[&str], changes: Vec<RawChange>) -> RangeCommit {
    RangeCommit { sha: s(sha), parents: parents.iter().map(|p| s(p)).collect(), subject: None, changes }
}
fn by_path(files: &[CombinedFile]) -> HashMap<String, CombinedFile> {
    files.iter().map(|f| (f.path.clone(), f.clone())).collect()
}
fn set(xs: &[&str]) -> HashSet<String> {
    xs.iter().map(|x| s(x)).collect()
}
fn strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|x| s(x)).collect()
}

const LOG_HEAD: [&str; 8] = ["log", "--topo-order", "--raw", "-z", "-M", "--no-abbrev", "--diff-merges=first-parent", RANGE_FORMAT];

#[test]
fn range_log_args_and_root() {
    let (c, a, p) = (rep('c'), rep('a'), rep('f'));
    assert_eq!(merge_base_args(&[c.clone(), a.clone()]), strings(&["merge-base", "--octopus", &c, &a]));
    let mut want = strings(&LOG_HEAD);
    want.extend(strings(&[&c, &a, "--not", &p, "--"]));
    assert_eq!(range_log_args(&[c.clone(), a.clone()], &[p.clone()]), want);
    let mut root = strings(&LOG_HEAD);
    root.extend(strings(&[&c, "--"]));
    assert_eq!(range_log_args(&[c.clone()], &[]), root);
    assert!(!range_log_args(&[c], &[]).contains(&s("--not")));
}

#[test]
fn exclude_below_the_base() {
    let (a, c, p) = (rep('a'), rep('c'), rep('f'));
    let sel = [a.clone(), c.clone()];
    assert_eq!(exclude_for("", &sel), Vec::<String>::new());
    assert_eq!(exclude_for(&a, &sel), vec![format!("{a}^@")]);
    assert_eq!(exclude_for(&p, &sel), vec![p.clone()]);
}

#[test]
fn parse_range_log_merges_renames_roots() {
    let c = rep('c');
    let m = rep('d');
    let p1 = rep('e');
    let p2 = rep('f');
    let out = format!(
        "\x1e{m} {p1} {p2}\x1fMerge branch 'fix'\0\n:100644 100644 {} {} M\0src/a.ts\0\x1e{c} {p1}\0\n:100644 100644 {} {} R100\0old name.ts\0new name.ts\0:000000 100644 {} {} A\0b.ts\0\x1e{p1}\0\n",
        blob(1),
        blob(2),
        blob(3),
        blob(3),
        z(),
        blob(4)
    );
    let mut merge = commit(&m, &[&p1, &p2], vec![ch('M', "src/a.ts", &blob(1), &blob(2), None)]);
    merge.subject = Some(s("Merge branch 'fix'"));
    assert_eq!(
        parse_range_log(&out),
        vec![
            merge,
            commit(&c, &[&p1], vec![ch('R', "new name.ts", &blob(3), &blob(3), Some("old name.ts")), ch('A', "b.ts", &z(), &blob(4), None)]),
            commit(&p1, &[], vec![]),
        ]
    );
}

#[test]
fn parse_range_log_hostile_names() {
    let (sha, p) = (rep('a'), rep('f'));
    let raw = |st: &str| format!(":100644 100644 {} {} {st}", blob(1), blob(2));
    let body = format!(
        "\x1e{sha} {p}\x1ffirst\x1esub\x1fmore\0\n{}\0a\nb.ts\0x\x1ey.ts\0{}\0:100644 100644 0 0 M\0{}\0-rf.ts\0{}\0tab\there.ts\0{}\0old => new.ts\0",
        raw("R100"),
        raw("M"),
        raw("M"),
        raw("M"),
        raw("M")
    );
    let want_changes = vec![
        ch('R', "x\x1ey.ts", &blob(1), &blob(2), Some("a\nb.ts")),
        ch('M', ":100644 100644 0 0 M", &blob(1), &blob(2), None),
        ch('M', "-rf.ts", &blob(1), &blob(2), None),
        ch('M', "tab\there.ts", &blob(1), &blob(2), None),
        ch('M', "old => new.ts", &blob(1), &blob(2), None),
    ];
    let parsed = parse_range_log(&body);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].sha, sha);
    assert_eq!(parsed[0].parents, vec![p.clone()]);
    assert_eq!(parsed[0].subject.as_deref(), Some("first\x1esub\x1fmore"));
    assert_eq!(parsed[0].changes, want_changes);
    // Cut off by a kill right after a raw header: only that change is dropped.
    for cut in [format!("{body}{}", raw("M")), format!("{body}{}\0", raw("M")), format!("{body}{}\0only-old.ts", raw("R100"))] {
        let parsed = parse_range_log(&cut);
        assert_eq!(parsed.len(), 1, "{cut:?}");
        assert_eq!(parsed[0].changes, want_changes, "{cut:?}");
    }
}

/// The spec's example: a (dana), x (rin), c (dana) in acme-web; a and c selected.
fn example() -> Vec<RangeCommit> {
    vec![
        commit("c", &["x"], vec![ch('M', "src/header.ts", &blob(3), &blob(4), None), ch('M', "src/header.css", &blob(13), &blob(14), None), ch('D', "src/debug.ts", &blob(21), &z(), None)]),
        commit("x", &["a"], vec![ch('M', "src/header.ts", &blob(2), &blob(3), None), ch('M', "src/header.css", &blob(12), &blob(13), None)]),
        commit(
            "a",
            &["p"],
            vec![
                ch('M', "src/header.ts", &blob(1), &blob(2), None),
                ch('M', "src/header.css", &blob(11), &blob(12), None),
                ch('A', "src/debug.ts", &z(), &blob(21), None),
                ch('M', "src/only-a.ts", &blob(31), &blob(32), None),
            ],
        ),
    ]
}

fn file(path: &str, old_path: Option<&str>, status: NetStatus, shas: &[&str], last: &str, before_ref: Option<&str>, before_path: &str, included: &[&str], fast: bool) -> CombinedFile {
    CombinedFile { path: s(path), old_path: old_path.map(s), status, shas: strings(shas), last: s(last), before_ref: before_ref.map(s), before_path: s(before_path), included: strings(included), fast }
}

#[test]
fn combine_each_file_once() {
    let (files, no_net) = combine_repo(&set(&["a", "c"]), &example());
    let f = by_path(&files);
    let mut keys: Vec<&String> = f.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["src/header.css", "src/header.ts", "src/only-a.ts"]);
    assert_eq!(f["src/header.ts"], file("src/header.ts", None, NetStatus::Modified, &["a", "c"], "c", Some("p"), "src/header.ts", &["x"], false));
    assert_eq!(f["src/header.css"].included, strings(&["x"]));
    assert_eq!(f["src/only-a.ts"], file("src/only-a.ts", None, NetStatus::Modified, &["a"], "a", Some("p"), "src/only-a.ts", &[], true));
    assert_eq!(no_net, strings(&["src/debug.ts"]));
}

#[test]
fn later_unselected_outside_range() {
    let (files, _) = combine_repo(&set(&["a", "x"]), &example());
    let f = by_path(&files);
    assert_eq!(f["src/header.ts"].included, Vec::<String>::new());
    assert!(f["src/header.ts"].fast);
}

fn linear(changes: Vec<Vec<RawChange>>) -> Vec<RangeCommit> {
    let n = changes.len();
    changes
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let parent = if i == n - 1 { s("p") } else { format!("s{}", i + 1) };
            RangeCommit { sha: format!("s{i}"), parents: vec![parent], subject: None, changes: c }
        })
        .collect()
}
fn all(n: usize) -> HashSet<String> {
    (0..n).map(|i| format!("s{i}")).collect()
}
fn statuses(log: &[RangeCommit]) -> Vec<NetStatus> {
    combine_repo(&all(log.len()), log).0.into_iter().map(|f| f.status).collect()
}

#[test]
fn net_status_table() {
    // Newest first, as git prints them.
    assert_eq!(statuses(&linear(vec![vec![ch('M', "f", &blob(2), &blob(3), None)], vec![ch('A', "f", &z(), &blob(2), None)]])), vec![NetStatus::Added]);
    assert_eq!(statuses(&linear(vec![vec![ch('D', "f", &blob(2), &z(), None)], vec![ch('M', "f", &blob(1), &blob(2), None)]])), vec![NetStatus::Deleted]);
    assert_eq!(statuses(&linear(vec![vec![ch('A', "f", &z(), &blob(5), None)], vec![ch('D', "f", &blob(1), &z(), None)]])), vec![NetStatus::Modified]);
    assert_eq!(statuses(&linear(vec![vec![ch('M', "f", &blob(2), &blob(3), None)], vec![ch('M', "f", &blob(1), &blob(2), None)]])), vec![NetStatus::Modified]);
    let back = linear(vec![vec![ch('M', "f", &blob(2), &blob(1), None)], vec![ch('M', "f", &blob(1), &blob(2), None)]]);
    assert_eq!(combine_repo(&all(2), &back), (vec![], strings(&["f"])));
}

#[test]
fn renames_followed_by_path() {
    let log = vec![
        commit("c", &["b"], vec![ch('M', "new.ts", &blob(2), &blob(3), None)]),
        commit("b", &["a"], vec![ch('R', "new.ts", &blob(2), &blob(2), Some("old.ts"))]),
        commit("a", &["p"], vec![ch('M', "old.ts", &blob(1), &blob(2), None)]),
    ];
    let (files, _) = combine_repo(&set(&["a", "b", "c"]), &log);
    assert_eq!(files, vec![file("new.ts", Some("old.ts"), NetStatus::Renamed, &["a", "b", "c"], "c", Some("p"), "old.ts", &[], true)]);
    // Renamed by an unselected commit: still one file, and it takes the slow path.
    let (f2, _) = combine_repo(&set(&["a", "c"]), &log);
    let got: Vec<(String, String, Vec<String>, bool)> = f2.into_iter().map(|f| (f.path, f.before_path, f.included, f.fast)).collect();
    assert_eq!(got, vec![(s("new.ts"), s("old.ts"), strings(&["b"]), false)]);
}

#[test]
fn unselected_merge_in_range_is_slow() {
    // An unselected merge brings a teammate's edit into header.ts between a and c.
    let log = vec![
        commit("c", &["m"], vec![ch('M', "header.ts", &blob(3), &blob(4), None)]),
        commit("m", &["a", "t"], vec![ch('M', "header.ts", &blob(2), &blob(3), None)]),
        commit("t", &["p"], vec![ch('M', "header.ts", &blob(1), &blob(9), None)]),
        commit("a", &["p"], vec![ch('M', "header.ts", &blob(1), &blob(2), None)]),
    ];
    let (files, _) = combine_repo(&set(&["a", "c"]), &log);
    assert_eq!(files[0].included, strings(&["t", "m"]));
    assert!(!files[0].fast);
}

#[test]
fn two_branches_take_slow_path() {
    // a and c on two branches: c is not a descendant of a.
    let log = vec![commit("c", &["q"], vec![ch('M', "f.ts", &blob(1), &blob(3), None)]), commit("a", &["p"], vec![ch('M', "f.ts", &blob(1), &blob(2), None)])];
    let (files, _) = combine_repo(&set(&["a", "c"]), &log);
    assert_eq!(files[0].included, Vec::<String>::new());
    assert!(!files[0].fast);
}

#[test]
fn later_unselected_rename_does_not_move_file() {
    // Renamed by an unselected commit after the last selected one that touched it: the file is
    // shown at the path that commit left it, not where it moved later.
    let log = vec![
        commit("c", &["u"], vec![ch('M', "x.ts", &blob(5), &blob(6), None)]),
        commit("u", &["a"], vec![ch('R', "new.ts", &blob(2), &blob(2), Some("old.ts"))]),
        commit("a", &["p"], vec![ch('M', "old.ts", &blob(1), &blob(2), None)]),
    ];
    let (files, _) = combine_repo(&set(&["a", "c"]), &log);
    let f = by_path(&files);
    let mut keys: Vec<&String> = f.keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["old.ts", "x.ts"]);
    let o = &f["old.ts"];
    assert_eq!((o.status.clone(), o.last.as_str(), o.old_path.clone(), o.fast), (NetStatus::Modified, "a", None, true));
}

// ---------------------------------------------------------------------------------------------
// The left side, against a fake git.

/// A fake git: `show --raw` records keyed by their last argument, `diff --raw` output keyed
/// `"diff {a} {b}"`, the merge-base and range-log answers, and blobs keyed by id or `sha:path`.
#[derive(Default)]
struct FakeGit {
    commits: HashMap<String, String>,
    blobs: HashMap<String, Vec<u8>>,
    merge_base: Option<String>,
    log: Option<String>,
    cancel_blobs: bool,
    calls: std::sync::Mutex<Vec<Vec<String>>>,
}

impl GitRead for FakeGit {
    fn read(&self, args: &[&str]) -> Result<Vec<u8>, ReadError> {
        self.calls.lock().unwrap().push(args.iter().map(|a| s(a)).collect());
        let target = args[args.len() - 1];
        match args[0] {
            "show" if args.contains(&"--raw") => self.commits.get(target).map(|t| t.as_bytes().to_vec()).ok_or_else(|| ReadError::Failed(format!("bad commit {target}"))),
            "diff" => Ok(self.commits.get(&format!("diff {} {target}", args[args.len() - 2])).cloned().unwrap_or_default().into_bytes()),
            "merge-base" => self.merge_base.clone().map(|b| format!("{b}\n").into_bytes()).ok_or_else(|| ReadError::Failed(s("no merge base"))),
            "log" => self.log.clone().map(String::into_bytes).ok_or_else(|| ReadError::Failed(s("no log"))),
            _ => Err(ReadError::Failed(format!("unexpected {args:?}"))),
        }
    }
    fn blob(&self, spec: &str, max: u64) -> Result<Blob, ReadError> {
        if self.cancel_blobs {
            return Err(ReadError::Cancelled);
        }
        Ok(match self.blobs.get(spec) {
            Some(b) if b.len() as u64 > max => Blob::TooLarge(b.len() as u64),
            Some(b) => Blob::Bytes(b.clone()),
            None => Blob::Missing,
        })
    }
}

fn rec(sha: &str, parent: &str, raw: &[String]) -> String {
    format!("\x1e{sha} {parent}\0\n{}", raw.concat())
}
fn m(path: &str, from: &str, to: &str) -> String {
    format!(":100644 100644 {from} {to} M\0{path}\0")
}
fn text(t: &str) -> LeftSide {
    LeftSide::Text(s(t))
}

const V1: &str = "import\nconst h = el(\"title\");\nh.text = title;\nh.pad = 8;\nreturn h;\n";
const VA: &str = "import\nconst h = el(\"title\");\nh.text = clip(title);\nh.pad = 8;\nreturn h;\n"; // a: line 3
const VX: &str = "import { el }\nconst h = el(\"title\");\nh.text = clip(title);\nh.pad = 8;\nreturn h;\n"; // x (rin): line 1, one line clear of a's
const VC: &str = "import { el }\nconst h = el(\"title\");\nh.text = clip(title);\nh.pad = 8;\nreturn h; // done\n"; // c: line 5

struct Ids {
    a: String,
    x: String,
    c: String,
    p: String,
}
fn ids() -> Ids {
    Ids { a: rep('a'), x: rep('e'), c: rep('c'), p: rep('f') }
}
fn header_commits(i: &Ids) -> Vec<(String, String)> {
    vec![
        (i.a.clone(), rec(&i.a, &i.p, &[m("header.ts", &blob(1), &blob(2))])),
        (i.x.clone(), rec(&i.x, &i.a, &[m("header.ts", &blob(2), &blob(3))])),
        (i.c.clone(), rec(&i.c, &i.x, &[m("header.ts", &blob(3), &blob(4))])),
    ]
}
fn fake(commits: &[(String, String)], blobs: &[(String, Vec<u8>)]) -> FakeGit {
    FakeGit { commits: commits.iter().cloned().collect(), blobs: blobs.iter().cloned().collect(), ..Default::default() }
}
fn b(k: impl Into<String>, v: &str) -> (String, Vec<u8>) {
    (k.into(), v.as_bytes().to_vec())
}

#[test]
fn show_raw_args_shape() {
    let c = rep('c');
    assert_eq!(show_raw_args(&c), strings(&["show", "--raw", "-z", "-M", "--no-abbrev", "--diff-merges=first-parent", "--format=%x1e%H %P%x1f%s", &c]));
}

#[test]
fn left_side_takes_selected_out() {
    let i = ids();
    let git = fake(&header_commits(&i), &[b(blob(1), V1), b(blob(2), VA), b(blob(3), VX), b(blob(4), VC), b(format!("{}:header.ts", i.c), VC)]);
    let left = left_side(&git, &i.c, "header.ts", &[i.c.clone(), i.a.clone()]).unwrap();
    assert_eq!(left, text("import { el }\nconst h = el(\"title\");\nh.text = title;\nh.pad = 8;\nreturn h;\n"));
}

#[test]
fn left_side_overlap_falls_back() {
    // x edited the same line as a: they can't be shown apart.
    let i = ids();
    let vx2 = "import\nconst h = el(\"title\");\nh.text = clip(title, 80);\nh.pad = 8;\nreturn h;\n";
    let vc2 = vx2.replace("return h;", "return h; // done");
    let git = fake(&header_commits(&i), &[b(blob(1), V1), b(blob(2), VA), b(blob(3), vx2), b(blob(4), &vc2), b(format!("{}:header.ts", i.c), &vc2)]);
    assert_eq!(left_side(&git, &i.c, "header.ts", &[i.c.clone(), i.a.clone()]).unwrap(), LeftSide::Fallback(Fallback::Conflict));
}

#[test]
fn left_side_binary_and_too_large() {
    let i = ids();
    let bin = "PNG\0\x01";
    let git = fake(&[(i.c.clone(), rec(&i.c, &i.x, &[m("logo.png", &blob(5), &blob(6))]))], &[b(blob(5), bin), b(blob(6), &format!("{bin}x")), b(format!("{}:logo.png", i.c), &format!("{bin}x"))]);
    assert_eq!(left_side(&git, &i.c, "logo.png", &[i.c.clone()]).unwrap(), LeftSide::Fallback(Fallback::Binary));
    let big = "x\n".repeat(20_001);
    let big_y = format!("{big}y\n");
    let git2 = fake(&[(i.c.clone(), rec(&i.c, &i.x, &[m("big.txt", &blob(7), &blob(8))]))], &[b(blob(7), &big), b(blob(8), &big_y), b(format!("{}:big.txt", i.c), &big_y)]);
    assert_eq!(left_side(&git2, &i.c, "big.txt", &[i.c.clone()]).unwrap(), LeftSide::Fallback(Fallback::TooLarge));
    // Not UTF-8: binary, as the single-commit view treats it.
    let git3 = fake(
        &[(i.c.clone(), rec(&i.c, &i.x, &[m("latin1.txt", &blob(5), &blob(6))]))],
        &[(blob(5), vec![0x61, 0x0a]), (blob(6), vec![0xff, 0xfe]), (format!("{}:latin1.txt", i.c), vec![0xff, 0xfe])],
    );
    assert_eq!(left_side(&git3, &i.c, "latin1.txt", &[i.c.clone()]).unwrap(), LeftSide::Fallback(Fallback::Binary));
    // Over the byte limit: the reader refuses it before reading it.
    let huge = "y".repeat(1_000_001);
    let git4 = fake(&[(i.c.clone(), rec(&i.c, &i.x, &[m("huge.txt", &blob(7), &blob(8))]))], &[b(blob(7), "y"), b(blob(8), &huge), b(format!("{}:huge.txt", i.c), &huge)]);
    assert_eq!(left_side(&git4, &i.c, "huge.txt", &[i.c.clone()]).unwrap(), LeftSide::Fallback(Fallback::TooLarge));
}

#[test]
fn left_side_follows_unselected_rename() {
    // a edits old.ts, b (unselected) renames it, c edits new.ts: a's step reads the old path's blobs.
    let i = ids();
    let bb = rep('d');
    let git = fake(
        &[
            (i.c.clone(), rec(&i.c, &bb, &[m("new.ts", &blob(2), &blob(3))])),
            (i.a.clone(), rec(&i.a, &i.p, &[m("old.ts", &blob(1), &blob(9))])),
            (format!("diff {} {bb}", i.a), format!(":100644 100644 {} {} R090\0old.ts\0new.ts\0", blob(9), blob(2))),
        ],
        &[b(blob(1), "1\n2\n3\n"), b(blob(9), "1\nTWO\n3\n"), b(blob(2), "1\nTWO\n3\n4\n"), b(blob(3), "1\nTWO\n3\n4\nFIVE\n"), b(format!("{}:new.ts", i.c), "1\nTWO\n3\n4\nFIVE\n")],
    );
    assert_eq!(left_side(&git, &i.c, "new.ts", &[i.c.clone(), i.a.clone()]).unwrap(), text("1\n2\n3\n4\n"));
    let calls = git.calls.lock().unwrap();
    assert!(calls.iter().any(|c| *c == strings(&["diff", "--raw", "-z", "-M", "--no-abbrev", &i.a, &bb])), "{calls:?}");
}

#[test]
fn left_side_older_than_rename_reads_old_path() {
    // After following the rename back, older selected commits are read at the old path too.
    let i = ids();
    let bb = rep('d');
    let q = rep('9');
    let git = fake(
        &[
            (i.c.clone(), rec(&i.c, &bb, &[m("new.ts", &blob(2), &blob(3))])),
            (i.a.clone(), rec(&i.a, &q, &[m("old.ts", &blob(10), &blob(9))])),
            (q.clone(), rec(&q, &i.p, &[m("old.ts", &blob(1), &blob(10))])),
            (format!("diff {} {bb}", i.a), format!(":100644 100644 {} {} R090\0old.ts\0new.ts\0", blob(9), blob(2))),
        ],
        &[
            b(blob(1), "1\n2\n3\n"),
            b(blob(10), "ONE\n2\n3\n"),
            b(blob(9), "ONE\nTWO\n3\n"),
            b(blob(2), "ONE\nTWO\n3\n4\n"),
            b(blob(3), "ONE\nTWO\n3\n4\nFIVE\n"),
            b(format!("{}:new.ts", i.c), "ONE\nTWO\n3\n4\nFIVE\n"),
        ],
    );
    assert_eq!(left_side(&git, &i.c, "new.ts", &[i.c.clone(), i.a.clone(), q.clone()]).unwrap(), text("1\n2\n3\n4\n"));
}

#[test]
fn left_side_past_edit_limit_too_large() {
    // More edits than the merge takes: too large, not a conflict.
    let i = ids();
    let before: String = (0..3000).map(|n| format!("l{n}\n")).collect();
    let after: String = (0..3000).map(|n| format!("o{n}\n")).collect();
    let git = fake(&[(i.c.clone(), rec(&i.c, &i.x, &[m("gen.ts", &blob(5), &blob(6))]))], &[b(blob(5), &before), b(blob(6), &after), b(format!("{}:gen.ts", i.c), &after)]);
    assert_eq!(left_side(&git, &i.c, "gen.ts", &[i.c.clone()]).unwrap(), LeftSide::Fallback(Fallback::TooLarge));
}

#[test]
fn left_side_lost_path_falls_back() {
    // A selected commit whose file can't be found at the followed path: fall back, never skip it.
    let i = ids();
    let bb = rep('d');
    let git = fake(
        &[
            (i.c.clone(), rec(&i.c, &bb, &[m("new.ts", &blob(2), &blob(3))])),
            (i.a.clone(), rec(&i.a, &i.p, &[m("old.ts", &blob(1), &blob(9))])),
            (format!("diff {} {bb}", i.a), s("")),
        ],
        &[b(blob(1), "1\n"), b(blob(9), "2\n"), b(blob(2), "2\n"), b(blob(3), "2\n3\n"), b(format!("{}:new.ts", i.c), "2\n3\n")],
    );
    assert_eq!(left_side(&git, &i.c, "new.ts", &[i.c.clone(), i.a.clone()]).unwrap(), LeftSide::Fallback(Fallback::Conflict));
}

struct Lcg(u64);
impl Lcg {
    fn rand(&mut self, n: usize) -> usize {
        self.0 = (self.0 * 1103515245 + 12345) % 2147483648;
        (self.0 % n as u64) as usize
    }
}

#[test]
fn left_side_round_trips() {
    // Whatever the alignment, the left side plus the selected changes must give the right side
    // exactly; otherwise the file falls back. Fuzzed with repeated, code-like lines.
    let i = ids();
    let mut r = Lcg(11);
    let pool = ["}", "{", "return x;", "", "if (a) {", "x++;", "// note"];
    let edit = |r: &mut Lcg, t: &[&'static str]| -> Vec<&'static str> {
        let mut o = t.to_vec();
        let mut e = 1 + r.rand(3);
        while e > 0 {
            let at = r.rand(o.len() + 1);
            let op = r.rand(3);
            if op == 0 {
                o.insert(at, pool[r.rand(pool.len())]);
            } else if op == 1 && o.len() > 1 {
                o.remove(at.min(o.len() - 1));
            } else {
                let k = at.min(o.len() - 1);
                o[k] = pool[r.rand(pool.len())];
            }
            e -= 1;
        }
        o
    };
    let txt = |l: &[&str]| l.iter().map(|x| format!("{x}\n")).collect::<String>();
    let commits = vec![
        (i.a.clone(), rec(&i.a, &i.p, &[m("f.ts", &blob(1), &blob(2))])),
        (i.x.clone(), rec(&i.x, &i.a, &[m("f.ts", &blob(2), &blob(3))])),
        (i.c.clone(), rec(&i.c, &i.x, &[m("f.ts", &blob(3), &blob(4))])),
    ];
    let mut checked = 0;
    for _ in 0..400 {
        let n = 6 + r.rand(8);
        let v0: Vec<&'static str> = (0..n).map(|_| pool[r.rand(pool.len())]).collect();
        let v1 = edit(&mut r, &v0);
        let v2 = edit(&mut r, &v1);
        let v3 = edit(&mut r, &v2);
        let git = fake(&commits, &[b(blob(1), &txt(&v0)), b(blob(2), &txt(&v1)), b(blob(3), &txt(&v2)), b(blob(4), &txt(&v3)), b(format!("{}:f.ts", i.c), &txt(&v3))]);
        let LeftSide::Text(left) = left_side(&git, &i.c, "f.ts", &[i.c.clone(), i.a.clone()]).unwrap() else { continue };
        checked += 1;
        let back = match merge3(&txt(&v0), &left, &txt(&v1)) {
            MergeResult::Clean(a) => merge3(&txt(&v2), &a, &txt(&v3)),
            other => other,
        };
        assert_eq!(back, MergeResult::Clean(txt(&v3)), "left side does not round-trip: {:?}", (&v0, &v1, &v2, &v3));
    }
    eprintln!("every left side round-trips to the right side ({checked} of 400 merged)");
    assert!(checked > 0);
}

#[test]
fn left_side_added_file_is_empty() {
    // Added by the oldest step: its "before" is empty.
    let i = ids();
    let git = fake(
        &[(i.c.clone(), rec(&i.c, &i.a, &[m("new.ts", &blob(1), &blob(2))])), (i.a.clone(), rec(&i.a, &i.p, &[format!(":000000 100644 {} {} A\0new.ts\0", z(), blob(1))]))],
        &[b(blob(1), "a\n"), b(blob(2), "a\nb\n"), b(format!("{}:new.ts", i.c), "a\nb\n")],
    );
    assert_eq!(left_side(&git, &i.c, "new.ts", &[i.c.clone(), i.a.clone()]).unwrap(), text(""));
}

// ---------------------------------------------------------------------------------------------
// One repository's read.

const CSS1: &str = "h1 {\n  color: red;\n  margin: 0;\n}\n";
const CSSA: &str = "h1 {\n  color: blue;\n  margin: 0;\n}\n"; // a: line 2
const CSSX: &str = "h1 {\n  color: navy;\n  margin: 0;\n}\n"; // x (rin): the same line
const CSSC: &str = "h1 {\n  color: navy;\n  margin: 4px;\n}\n"; // c: line 3

/// The spec's example as git would answer it: a, x (rin), c; a and c selected.
fn example_git() -> (FakeGit, Ids) {
    let i = ids();
    let (a, x, c, p) = (&i.a, &i.x, &i.c, &i.p);
    let c_raw = [m("src/header.ts", &blob(3), &blob(4)), m("src/header.css", &blob(13), &blob(14)), format!(":100644 000000 {} {} D\0src/debug.ts\0", blob(21), z())];
    let x_raw = [m("src/header.ts", &blob(2), &blob(3)), m("src/header.css", &blob(12), &blob(13))];
    let a_raw = [m("src/header.ts", &blob(1), &blob(2)), m("src/header.css", &blob(11), &blob(12)), format!(":000000 100644 {} {} A\0src/debug.ts\0", z(), blob(21)), m("src/only-a.ts", &blob(31), &blob(32))];
    let with_subject = |sha: &str, parent: &str, subject: &str, raw: &[String]| format!("\x1e{sha} {parent}\x1f{subject}\0\n{}", raw.concat());
    let log = [with_subject(c, x, "header: done", &c_raw), with_subject(x, a, "header: wider title", &x_raw), with_subject(a, p, "header: clip", &a_raw)].concat();
    let mut git = fake(
        &[(c.clone(), rec(c, x, &c_raw)), (x.clone(), rec(x, a, &x_raw)), (a.clone(), rec(a, p, &a_raw))],
        &[
            b(blob(1), V1),
            b(blob(2), VA),
            b(blob(3), VX),
            b(blob(4), VC),
            b(format!("{c}:src/header.ts"), VC),
            b(blob(11), CSS1),
            b(blob(12), CSSA),
            b(blob(13), CSSX),
            b(blob(14), CSSC),
            b(format!("{c}:src/header.css"), CSSC),
        ],
    );
    git.merge_base = Some(a.clone());
    git.log = Some(log);
    (git, i)
}

#[test]
fn read_combined_modes() {
    let (git, i) = example_git();
    let read = read_combined(&git, &[i.c.clone(), i.a.clone()]).unwrap();
    // The range starts below the base, which is itself selected: its parents are excluded.
    let calls = git.calls.lock().unwrap().clone();
    let log_call = calls.iter().find(|c| c[0] == "log").expect("range log read");
    assert_eq!(log_call[log_call.len() - 3..], strings(&["--not", &format!("{}^@", i.a), "--"]));
    assert_eq!(read.commits, vec![i.a.clone(), i.c.clone()]);
    let f: HashMap<&str, _> = read.files.iter().map(|r| (r.file.path.as_str(), r)).collect();
    let mut keys: Vec<&&str> = f.keys().collect();
    keys.sort();
    assert_eq!(keys, vec![&"src/header.css", &"src/header.ts", &"src/only-a.ts"]);
    assert_eq!((f["src/header.ts"].mode, f["src/header.ts"].reason), (Mode::Merge, None));
    assert_eq!((f["src/header.css"].mode, f["src/header.css"].reason), (Mode::Fallback, Some(Fallback::Conflict)));
    // The fallback still names the unselected commit it includes.
    assert_eq!(f["src/header.css"].file.included, vec![i.x.clone()]);
    assert_eq!((f["src/only-a.ts"].mode, f["src/only-a.ts"].reason), (Mode::Fast, None));
    assert_eq!(read.no_net_change, strings(&["src/debug.ts"]));
    assert_eq!(read.subjects.len(), 1);
    assert_eq!(read.subjects.get(&i.x).map(String::as_str), Some("header: wider title"));
}

#[test]
fn read_combined_binary_fallback_names_included() {
    let (mut git, i) = example_git();
    // header.css becomes binary at x: the fallback says binary, and still lists x.
    git.blobs.insert(blob(13), b"\0bin".to_vec());
    let read = read_combined(&git, &[i.a.clone(), i.c.clone()]).unwrap();
    let css = read.files.iter().find(|r| r.file.path == "src/header.css").unwrap();
    assert_eq!((css.mode, css.reason), (Mode::Fallback, Some(Fallback::Binary)));
    assert_eq!(css.file.included, vec![i.x.clone()]);
}

#[test]
fn read_combined_net_zero_slow_file_is_left_out() {
    // c undoes a's change to header.ts; x's edit sits between. Taking both out gives back the
    // right side exactly: no net change.
    let (mut git, i) = example_git();
    let vx3 = V1.replace("import", "import { el }");
    git.blobs.insert(blob(2), VA.as_bytes().to_vec());
    git.blobs.insert(blob(3), VX.as_bytes().to_vec());
    git.blobs.insert(blob(4), vx3.as_bytes().to_vec());
    git.blobs.insert(format!("{}:src/header.ts", i.c), vx3.as_bytes().to_vec());
    let read = read_combined(&git, &[i.a.clone(), i.c.clone()]).unwrap();
    assert!(read.files.iter().all(|r| r.file.path != "src/header.ts"));
    assert_eq!(read.no_net_change, strings(&["src/debug.ts", "src/header.ts"]));
}

#[test]
fn read_combined_without_base_or_log() {
    // No merge base (unrelated histories): no --not at all.
    let (mut git, i) = example_git();
    git.merge_base = None;
    read_combined(&git, &[i.a.clone(), i.c.clone()]).unwrap();
    let calls = git.calls.lock().unwrap().clone();
    let log_call = calls.iter().find(|c| c[0] == "log").unwrap();
    assert!(!log_call.contains(&s("--not")));
    // The range log failing is the repository's failure.
    let (mut git, i) = example_git();
    git.log = None;
    assert!(matches!(read_combined(&git, &[i.a.clone(), i.c.clone()]), Err(ReadError::Failed(_))));
}

#[test]
fn read_combined_cancel_propagates() {
    let (mut git, i) = example_git();
    git.cancel_blobs = true;
    assert_eq!(read_combined(&git, &[i.a.clone(), i.c.clone()]).unwrap_err(), ReadError::Cancelled);
}

#[test]
fn par_map_limit_keeps_order_and_limit() {
    let items: Vec<usize> = (0..20).collect();
    let now = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let out = par_map_limit(&items, 4, |&n| {
        let c = now.fetch_add(1, Ordering::SeqCst) + 1;
        peak.fetch_max(c, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(5));
        now.fetch_sub(1, Ordering::SeqCst);
        n * 10
    });
    assert_eq!(out, (0..20).map(|n| n * 10).collect::<Vec<_>>());
    let p = peak.load(Ordering::SeqCst);
    assert!(p <= 4 && p >= 1, "peak {p}");
}
