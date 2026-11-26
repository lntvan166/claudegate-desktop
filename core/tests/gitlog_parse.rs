use claudegate_core::gitlog::{parse_changes, parse_log, web_url, FileStatus};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn rec(sha: &str, ct: &str, an: &str, ae: &str, s: &str, p: &str) -> String {
    format!("{sha}\0{ct}\0{an}\0{ae}\0{s}\0{p}\x1e")
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
