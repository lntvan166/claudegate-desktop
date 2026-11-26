use claudegate_core::gitlog::{parse_changes, parse_log, web_url, FileStatus};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn rec(sha: &str, ct: &str, an: &str, ae: &str, s: &str, p: &str) -> String {
    rec_src(sha, ct, an, ae, s, p, "HEAD")
}

fn rec_src(sha: &str, ct: &str, an: &str, ae: &str, s: &str, p: &str, src: &str) -> String {
    format!("{sha}\0{ct}\0{an}\0{ae}\0{s}\0{p}\0{src}\x1e")
}

#[test]
fn parses_tricky_fields() {
    let subj = "fix: \"quote\" 🚀 tab\there 100% done ";
    let out = format!("{}\n{}\n", rec(A, "1700000000", "Zoë Müller 李", "z@acme.test", subj, B), rec(B, "1600000000", "Bo", "b@acme.test", "x", ""));
    let c = parse_log(&out);
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].subject, subj);
    assert_eq!(c[0].author, "Zoë Müller 李");
    assert_eq!(c[0].time, 1700000000);
    assert_eq!(c[0].parents, vec![B.to_string()]);
}

#[test]
fn parses_root_and_merge_parents() {
    let out = format!("{}{}", rec(A, "1", "a", "a@x.test", "root", ""), rec(C, "2", "a", "a@x.test", "merge", &format!("{A} {B}")));
    let c = parse_log(&out);
    assert!(c[0].parents.is_empty());
    assert_eq!(c[1].parents, vec![A.to_string(), B.to_string()]);
}

#[test]
fn parses_source() {
    let out = format!("{}\n{}\n{}\n", rec(A, "3", "a", "e", "x", ""), rec_src(B, "2", "a", "e", "y", "", "refs/heads/feature/x"), rec_src(C, "1", "a", "e", "z", "", A));
    let c = parse_log(&out);
    let src: Vec<&str> = c.iter().map(|c| c.source.as_str()).collect();
    assert_eq!(src, vec!["HEAD", "refs/heads/feature/x", A]);
    // The six-field records of the old format are not commits.
    assert!(parse_log(&format!("{A}\0001\0a\0e\0s\0\x1e")).is_empty());
}

#[test]
fn drops_truncated_and_malformed_records() {
    let out = format!("{}{}{A}\0123\0a", rec(A, "1", "a", "e", "ok", ""), rec("nothex", "1", "a", "e", "bad", ""));
    assert_eq!(parse_log(&out).len(), 1);
}

#[test]
fn parses_rename_and_binary() {
    let ns = "A\tnew.txt\nR100\told/a.rs\tnew/a.rs\nM\tpic.png\nD\tgone.rs\nC75\tx\ty\nT\tlink\n";
    let st = "5\t0\tnew.txt\n2\t1\told/a.rs => new/a.rs\n-\t-\tpic.png\n0\t9\tgone.rs\n";
    let f = parse_changes(ns, st);
    assert_eq!(f.len(), 6);
    assert_eq!(f[0].status, FileStatus::Added);
    assert_eq!((f[0].added, f[0].removed), (Some(5), Some(0)));
    assert_eq!(f[1].path, "new/a.rs");
    assert_eq!(f[1].status, FileStatus::Renamed { from: "old/a.rs".into() });
    assert_eq!((f[1].added, f[1].removed), (Some(2), Some(1)));
    assert_eq!((f[2].added, f[2].removed), (None, None));
    assert_eq!(f[3].status, FileStatus::Deleted);
    assert_eq!(f[4].status, FileStatus::Copied { from: "x".into() });
    assert_eq!(f[5].status, FileStatus::TypeChanged);
}

#[test]
fn numstat_brace_rename() {
    let f = parse_changes("R090\tsrc/a/f.rs\tsrc/b/f.rs\n", "1\t1\tsrc/{a => b}/f.rs\n");
    assert_eq!((f[0].added, f[0].removed), (Some(1), Some(1)));
}

#[test]
fn web_url_providers() {
    let cases = [
        ("git@github.com:acme/acme-web.git", format!("https://github.com/acme/acme-web/commit/{A}")),
        ("https://github.com/acme/acme-web", format!("https://github.com/acme/acme-web/commit/{A}")),
        ("ssh://git@github.com/acme/acme-web.git", format!("https://github.com/acme/acme-web/commit/{A}")),
        ("git@gitlab.com:acme/tools/acme-api.git", format!("https://gitlab.com/acme/tools/acme-api/-/commit/{A}")),
        ("https://gitlab.com/acme/acme-api.git", format!("https://gitlab.com/acme/acme-api/-/commit/{A}")),
        ("ssh://git@gitlab.acme.test:2222/acme/acme-api.git", format!("https://gitlab.acme.test/acme/acme-api/-/commit/{A}")),
        ("git@bitbucket.org:acme/svc-lib.git", format!("https://bitbucket.org/acme/svc-lib/commits/{A}")),
        ("https://bitbucket.org/acme/svc-lib.git", format!("https://bitbucket.org/acme/svc-lib/commits/{A}")),
        ("https://dev.azure.com/acme/web/_git/acme-web", format!("https://dev.azure.com/acme/web/_git/acme-web/commit/{A}")),
        ("git@ssh.dev.azure.com:v3/acme/web/acme-web", format!("https://dev.azure.com/acme/web/_git/acme-web/commit/{A}")),
        ("https://dana:ghp_secret@github.com/acme/acme-web.git", format!("https://github.com/acme/acme-web/commit/{A}")),
        ("https://oauth2:tok@gitlab.com/acme/acme-api.git", format!("https://gitlab.com/acme/acme-api/-/commit/{A}")),
    ];
    for (remote, want) in cases {
        assert_eq!(web_url(remote, A).as_deref(), Some(want.as_str()), "{remote}");
    }
    let out = web_url("https://dana:ghp_secret@github.com/acme/acme-web.git", A).unwrap();
    assert!(!out.contains("ghp_secret") && !out.contains('@'));
}

#[test]
fn web_url_unknown_host_none() {
    assert_eq!(web_url("https://git.example.test/acme/acme-web.git", A), None);
    for r in ["/srv/git/acme-web.git", "C:\\repos\\x.git", "file:///srv/x.git", "", "git@github.com:../../x", "git@evil.test@github.com:a/b"] {
        assert_eq!(web_url(r, A), None, "{r}");
    }
    assert_eq!(web_url("git@github.com:a/b.git", "nothex"), None);
}

use claudegate_core::gitlog::unquote_c_style;

#[test]
fn unquotes_c_style() {
    assert_eq!(unquote_c_style(r#""caf\303\251.rs""#), "café.rs");
    assert_eq!(unquote_c_style(r#""a\tb\\c\"d.rs""#), "a\tb\\c\"d.rs");
    assert_eq!(unquote_c_style("plain.rs"), "plain.rs");
}

#[test]
fn quoted_paths_in_changes() {
    let f = parse_changes("A\t\"caf\\303\\251.rs\"\nM\t\"a\\tb.rs\"\nM\t\"q\\\"x.rs\"\n", "1\t0\t\"caf\\303\\251.rs\"\n2\t0\t\"a\\tb.rs\"\n3\t0\t\"q\\\"x.rs\"\n");
    assert_eq!(f[0].path, "café.rs");
    assert_eq!(f[1].path, "a\tb.rs");
    assert_eq!(f[2].path, "q\"x.rs");
    assert_eq!((f[0].added, f[1].added, f[2].added), (Some(1), Some(2), Some(3)));
}

#[test]
fn quoted_rename() {
    let f = parse_changes("R100\t\"caf\\303\\251.rs\"\t\"d\\303\\251j\\303\\240.rs\"\n", "1\t1\t\"caf\\303\\251.rs\" => \"d\\303\\251j\\303\\240.rs\"\n");
    assert_eq!(f[0].path, "déjà.rs");
    assert_eq!(f[0].status, FileStatus::Renamed { from: "café.rs".into() });
    assert_eq!(f[0].added, Some(1));
    let g = parse_changes("R100\tsrc/\"x\"\tsrc/y\n", "");
    assert_eq!(g.len(), 1);
    let f = parse_changes("R090\ta/\"o\\303\\251\"/f.rs\ta/n/f.rs\n", "4\t4\ta/{\"o\\303\\251\" => n}/f.rs\n");
    assert_eq!(f[0].path, "a/n/f.rs");
    assert_eq!(f[0].added, Some(4));
}

#[test]
fn brace_rename_empty_sides() {
    let f = parse_changes("R1\tsrc/f.rs\tsrc/b/f.rs\nR1\tsrc/a/g.rs\tsrc/g.rs\n", "1\t0\tsrc/{ => b}/f.rs\n2\t0\tsrc/{a => }/g.rs\n");
    assert_eq!(f[0].added, Some(1));
    assert_eq!(f[1].added, Some(2));
}

#[test]
fn log_without_trailing_rs_and_crlf() {
    let full = rec(A, "1", "a", "e", "ok", "");
    let cut = format!("{}{B}\0{}\0a\0e\0s\0\0HEAD", full, 2);
    assert_eq!(parse_log(&cut).len(), 1);
    let f = parse_changes("M\ta.rs\r\nA\tb.rs\r\n", "1\t2\ta.rs\r\n3\t4\tb.rs\r\n");
    assert_eq!(f.len(), 2);
    assert_eq!((f[1].path.as_str(), f[1].added, f[1].removed), ("b.rs", Some(3), Some(4)));
}

#[test]
fn web_url_rejects_ftp_and_encoded_dots() {
    assert_eq!(web_url("ftp://github.com/acme/acme-web.git", A), None);
    assert_eq!(web_url("https://github.com/acme/%2e%2e/x", A), None);
    assert_eq!(web_url("https://github.com/acme/a%2Fb", A), None);
}

#[test]
fn real_git_rename_lines() {
    let f = parse_changes("R100\t\"a/old/caf\\303\\251.rs\"\t\"a/new/caf\\303\\251.rs\"\n", "");
    assert_eq!((f[0].path.as_str(), &f[0].status), ("a/new/café.rs", &FileStatus::Renamed { from: "a/old/café.rs".into() }));
    let f = parse_changes("R100\ta/old/café.rs\ta/new/café.rs\n", "0\t0\ta/{old => new}/café.rs\n");
    assert_eq!(f[0].path, "a/new/café.rs");
    assert_eq!((f[0].added, f[0].removed), (Some(0), Some(0)));
    let f = parse_changes("R100\t\"tab\\tname.rs\"\t\"tab\\trenamed.rs\"\n", "0\t0\t\"tab\\tname.rs\" => \"tab\\trenamed.rs\"\n");
    assert_eq!(f[0].path, "tab\trenamed.rs");
    assert_eq!(f[0].status, FileStatus::Renamed { from: "tab\tname.rs".into() });
    assert_eq!(f[0].added, Some(0));
}

#[test]
fn never_panics_on_random_input() {
    let alphabet = ["a", "b", "é", "李", "🚀", "{", "}", " => ", "=>", "\"", "\\", "\t", "\n", "\r", "\0", "\x1e", " ", "/", ".git", "%2e", ":", "@", "R100", "A", "-", "1", "http://", "git@", "ssh://", "[", "]"];
    let mut x: u64 = 0x2545F4914F6CDD1D;
    let mut next = || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 33) as usize
    };
    for _ in 0..200 {
        let n = next() % 40;
        let a: String = (0..n).map(|_| alphabet[next() % alphabet.len()]).collect();
        let b: String = (0..n).map(|_| alphabet[next() % alphabet.len()]).collect();
        let _ = parse_changes(&a, &b);
        let _ = parse_changes(&b, &a);
        // Well-formed numstat lines with a random path, so numstat_new_path sees braces,
        // arrows and quotes rather than lines the tab split rejects first.
        let counts = ["-", "0", "1", "12"];
        let line_safe: Vec<&str> = alphabet.iter().copied().filter(|t| !t.contains(['\t', '\n', '\r'])).collect();
        let numstat: String = (0..1 + next() % 4)
            .map(|_| {
                let path: String = (0..1 + next() % 12).map(|_| line_safe[next() % line_safe.len()]).collect();
                format!("{}\t{}\t{}\n", counts[next() % counts.len()], counts[next() % counts.len()], path)
            })
            .collect();
        let _ = parse_changes(&a, &numstat);
        let _ = parse_log(&a);
        let _ = unquote_c_style(&a);
        let _ = web_url(&a, A);
        let _ = web_url(&format!("https://github.com/{a}"), A);
        let _ = web_url(&format!("git@github.com:{a}.git"), A);
        let _ = web_url(&format!("git@github.com:é{a}é"), A);
    }
}
