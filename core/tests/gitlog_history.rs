use claudegate_core::gitlog::{history_args, history_paths_args, parse_history, parse_history_paths, FileStatus, HistoryFile, HISTORY_FORMAT};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// One record exactly as `git log --name-status -z` with `HISTORY_FORMAT` emits it.
fn hrec(sha: &str, ct: i64, subject: &str, status: &str, paths: &[&str]) -> String {
    let mut r = format!("\x1e{}\0{ct}\0dana\0dana@example.com\0{subject}\0\0\n{status}\0", sha);
    for p in paths {
        r.push_str(p);
        r.push('\0');
    }
    r
}

fn file(path: &str, from: Option<&str>, status: Option<FileStatus>) -> HistoryFile {
    HistoryFile { path: path.into(), from: from.map(String::from), status }
}

#[test]
fn history_args_shape() {
    let a = history_args(&s(&["HEAD"]), 200, 5, &s(&["src/client.ts", "src/old.ts"]));
    assert_eq!(a[0], "log");
    assert_eq!(a[1], HISTORY_FORMAT);
    assert_eq!(a[2], "--max-count=200");
    assert!(a.contains(&"--skip=5".to_string()));
    assert!(a.contains(&"--no-show-signature".to_string()));
    assert!(!a.contains(&"--follow".to_string()), "no --follow: it cannot page with --skip");
    assert_eq!(a[a.len() - 8..], s(&["--name-status", "-z", "-M", "--end-of-options", "HEAD", "--", ":(literal)src/client.ts", ":(literal)src/old.ts"]));
    let first = history_args(&[], 10, 0, &s(&["a.ts"]));
    assert!(!first.iter().any(|x| x.starts_with("--skip")), "no --skip on the first page");
    assert!(!first.contains(&"--end-of-options".to_string()), "no heads: no revisions at all");
}

#[test]
fn a_path_is_never_an_option() {
    let a = history_args(&s(&["HEAD"]), 1, 0, &s(&["--output=/tmp/x"]));
    assert_eq!(a[a.len() - 2..], s(&["--", ":(literal)--output=/tmp/x"]));
    let a = history_args(&[], 1, 0, &s(&["app/[id]/page.tsx"]));
    assert_eq!(a.last().unwrap(), ":(literal)app/[id]/page.tsx", "a route folder like [id] is not a glob");
    let p = history_paths_args("--output=/tmp/x", &[]);
    assert_eq!(p[p.len() - 2..], s(&["--", ":(literal)--output=/tmp/x"]));
}

#[test]
fn history_paths_args_shape() {
    let a = history_paths_args("src/client.ts", &s(&["origin/prod"]));
    assert_eq!(a[..2], s(&["log", "--follow"]));
    assert!(a.contains(&"--no-show-signature".to_string()));
    assert_eq!(a[a.len() - 8..], s(&["--name-status", "-z", "-M", "--format=%x1e", "--end-of-options", "origin/prod", "--", ":(literal)src/client.ts"]));
    let a = history_paths_args("app/[id]/page.tsx", &[]);
    assert_eq!(a[a.len() - 2..], s(&["--", ":(literal)app/[id]/page.tsx"]));
    assert!(!a.contains(&"--end-of-options".to_string()));
}

#[test]
fn parse_history_paths_names() {
    assert_eq!(parse_history_paths("\x1e\0\nM\0new.ts\0\x1e\0\nR100\0old.ts\0new.ts\0\x1e\0\nA\0old.ts\0", "new.ts"), s(&["new.ts", "old.ts"]));
    assert_eq!(parse_history_paths("", "a.ts"), s(&["a.ts"]), "an untracked file still has its own path");
}

#[test]
fn parse_history_paths_reads_names_by_position() {
    // A name that looks like a status, one with RS and a newline, one led by RS.
    let out = "\x1e\0\nM\0M\0\x1e\0\nR087\0x\x1ey\nz.ts\0M\0\x1e\0\nC100\0\x1eq.ts\0x\x1ey\nz.ts\0\x1e\0\nA\0\x1eq.ts\0";
    assert_eq!(parse_history_paths(out, "M"), s(&["M", "x\x1ey\nz.ts", "\x1eq.ts"]));
}

#[test]
fn history_ignores_log_filters() {
    let a = history_args(&s(&["feat"]), 200, 0, &s(&["a.ts"]));
    for bad in ["--grep", "--author", "--since", "--until", "-i", "-F"] {
        assert!(!a.iter().any(|x| x == bad || x.starts_with(&format!("{bad}="))), "{bad} in {a:?}");
    }
}

#[test]
fn parse_history_follows_renames() {
    let out = hrec(A, 400, "edit new", "M", &["src/new.ts"]) + &hrec(B, 300, "rename", "R100", &["src/old.ts", "src/new.ts"]) + &hrec(C, 200, "edit old", "M", &["src/old.ts"]);
    let (rows, n) = parse_history(&out, "src/new.ts");
    assert_eq!(n, 3);
    let got: Vec<(&str, &HistoryFile)> = rows.iter().map(|(c, f)| (c.subject.as_str(), f)).collect();
    assert_eq!(
        got,
        vec![
            ("edit new", &file("src/new.ts", None, Some(FileStatus::Modified))),
            ("rename", &file("src/new.ts", Some("src/old.ts"), Some(FileStatus::Renamed { from: "src/old.ts".into() }))),
            ("edit old", &file("src/old.ts", None, Some(FileStatus::Modified))),
        ]
    );
    assert_eq!(rows[0].0.sha, A);
    assert_eq!(rows[0].0.time, 400);
    assert_eq!(rows[0].0.email, "dana@example.com");
    assert_eq!(parse_history("", "x"), (vec![], 0));

    let cut = hrec(A, 1, "whole", "M", &["a.ts"]) + "\x1e" + &[B, "2", "da"].join("\0");
    let (rows, n) = parse_history(&cut, "a.ts");
    assert_eq!(rows.iter().map(|(c, _)| c.subject.as_str()).collect::<Vec<_>>(), vec!["whole"], "a record cut off by a kill is dropped");
    assert_eq!(n, 1, "and not counted");

    let merge = format!("\x1e{C}\x005\0rin\0rin@example.com\0merge\0{A} {B}\0");
    let (rows, n) = parse_history(&merge, "a.ts");
    assert_eq!(n, 1);
    assert_eq!(rows[0].1, file("a.ts", None, None), "a record with no status keeps the file's current path");
    assert_eq!(rows[0].0.parents, vec![A.to_string(), B.to_string()]);

    // After a rename, a status-less record keeps the name the file had before it.
    let out = hrec(A, 3, "rename", "R100", &["old.ts", "new.ts"]) + &format!("\x1e{B}\x002\0rin\0rin@example.com\0merge\0{C} {C}\0");
    let (rows, _) = parse_history(&out, "new.ts");
    assert_eq!(rows[1].1, file("old.ts", None, None));
}

#[test]
fn parse_history_hostile_names() {
    let out = hrec(A, 2, "a\x1eb\x1fc", "R100", &["a\nR100\nb.ts", "x\x1ey.ts"]) + &hrec(B, 1, "plain", "M", &["-rf.ts"]);
    let (rows, n) = parse_history(&out, "x\x1ey.ts");
    assert_eq!(n, 2);
    assert_eq!(rows[0].0.subject, "a\x1eb\x1fc");
    assert_eq!(rows[0].1, file("x\x1ey.ts", Some("a\nR100\nb.ts"), Some(FileStatus::Renamed { from: "a\nR100\nb.ts".into() })));
    // "-rf.ts" is not a name this file had: another file's commit, counted but not shown.
    assert_eq!(rows.len(), 1);
    let (rows, n) = parse_history(&out, "-rf.ts");
    assert_eq!(n, 2);
    assert_eq!(rows.len(), 1, "read from the other file's side, only its own record");
    assert_eq!(rows[0].1, file("-rf.ts", None, Some(FileStatus::Modified)));
}

#[test]
fn a_subject_led_by_rs_does_not_start_a_record() {
    let out = hrec(A, 2, "\x1e", "M", &["a.ts"]) + &hrec(B, 1, "\x1eb", "A", &["a.ts"]);
    let (rows, n) = parse_history(&out, "a.ts");
    assert_eq!(n, 2);
    assert_eq!(rows.iter().map(|(c, _)| c.subject.as_str()).collect::<Vec<_>>(), vec!["\x1e", "\x1eb"]);
}

#[test]
fn an_unparseable_record_still_counts() {
    let out = hrec("nope", 2, "bad", "M", &["a.ts"]) + &hrec(B, 1, "good", "A", &["a.ts"]);
    let (rows, n) = parse_history(&out, "a.ts");
    assert_eq!(n, 2, "--skip advances past it");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0.subject, "good");
}

#[test]
fn several_entries_prefer_the_followed_name() {
    // The newest commit renamed old.ts to new.ts; an older one touched both a later-reused
    // new.ts and old.ts.
    let out = hrec(A, 3, "rename", "R100", &["old.ts", "new.ts"]) + &hrec(B, 2, "both", "M", &["new.ts"]) + "\nM\0old.ts\0" + &hrec(C, 1, "after", "M", &["old.ts"]);
    let (rows, n) = parse_history(&out, "new.ts");
    assert_eq!(n, 3);
    assert_eq!(rows[1].1, file("old.ts", None, Some(FileStatus::Modified)));
    assert_eq!(rows[2].0.subject, "after");
}

#[test]
fn a_reused_old_name_is_another_file() {
    // Newest first: a new, unrelated old.ts (A), then new.ts's own history.
    let out = hrec(A, 5, "new old", "A", &["old.ts"]) + &hrec(B, 4, "edit new", "M", &["new.ts"]) + &hrec(C, 3, "rename", "R100", &["old.ts", "new.ts"]);
    let (rows, n) = parse_history(&out, "new.ts");
    assert_eq!(n, 3, "counted, so --skip still advances past it");
    assert_eq!(rows.iter().map(|(c, _)| c.subject.as_str()).collect::<Vec<_>>(), vec!["edit new", "rename"]);
    // And it does not move the followed name: a merge after it keeps new.ts.
    let merge = format!("\x1e{B}\x004\0rin\0rin@example.com\0merge\0{A} {C}\0");
    let (rows, n) = parse_history(&(hrec(A, 5, "new old", "M", &["old.ts"]) + &merge), "new.ts");
    assert_eq!(n, 2);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, file("new.ts", None, None));
}

#[test]
fn delete_then_re_add_is_one_history() {
    let out = hrec(A, 3, "back", "A", &["a.ts"]) + &hrec(B, 2, "gone", "D", &["a.ts"]) + &hrec(C, 1, "first", "A", &["a.ts"]);
    let (rows, n) = parse_history(&out, "a.ts");
    assert_eq!(n, 3);
    let st: Vec<Option<FileStatus>> = rows.into_iter().map(|(_, f)| f.status).collect();
    assert_eq!(st, vec![Some(FileStatus::Added), Some(FileStatus::Deleted), Some(FileStatus::Added)]);
}
