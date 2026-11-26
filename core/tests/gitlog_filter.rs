use claudegate_core::gitlog::{author_patterns, git_date, heads_for, is_valid_ref, log_args, normalize_path, pathspec_of, repo_searched, BranchUse, Filter, LOG_FORMAT};

const NOW: i64 = 1_790_164_800; // 2026-09-23T12:00:00Z

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn args(f: &Filter) -> Vec<String> {
    log_args(f, &[], None, 200, 0)
}

fn authors_of(a: &[String]) -> Vec<String> {
    a.iter().filter(|x| x.starts_with("--author=")).cloned().collect()
}

#[test]
fn git_date_is_iso_utc() {
    assert_eq!(git_date(2000), "1970-01-01T00:33:20Z");
    assert_eq!(git_date(NOW), "2026-09-23T12:00:00Z");
    assert_eq!(git_date(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(git_date(-1), "1969-12-31T23:59:59Z");
}

#[test]
fn no_filters_is_the_bare_log() {
    let bare = s(&["log", LOG_FORMAT, "--source", "--max-count=200", "--"]);
    assert_eq!(args(&Filter::default()), bare);
    assert_eq!(args(&Filter { search: Some("   ".into()), ..Default::default() }), bare, "blank search adds no --grep");
}

#[test]
fn search_is_literal_and_case_insensitive() {
    let a = args(&Filter { search: Some(" ACME-7 ".into()), ..Default::default() });
    assert_eq!(a, s(&["log", LOG_FORMAT, "--source", "--max-count=200", "--grep=ACME-7", "-i", "-F", "--"]));
    for t in ["fix(api)", "[ACME-7]", ".", "a|b", "^x$"] {
        let a = args(&Filter { search: Some(t.into()), ..Default::default() });
        assert!(a.contains(&"-F".to_string()) && a.contains(&format!("--grep={t}")), "literal: {t}");
    }
}

#[test]
fn search_never_becomes_an_option() {
    let a = args(&Filter { search: Some("--output=/tmp/pwned".into()), ..Default::default() });
    assert!(a.contains(&"--grep=--output=/tmp/pwned".to_string()));
    assert!(!a.contains(&"--output=/tmp/pwned".to_string()));
}

#[test]
fn since_until_as_dates() {
    let a = args(&Filter { since: Some(NOW - 86_400), until: Some(NOW), ..Default::default() });
    assert!(a.contains(&"--since=2026-09-22T12:00:00Z".to_string()));
    assert!(a.contains(&"--until=2026-09-23T12:00:00Z".to_string()));
    assert!(!args(&Filter::default()).iter().any(|x| x.starts_with("--since") || x.starts_with("--until")));
}

#[test]
fn cursor_is_a_skip_never_a_date() {
    let a = log_args(&Filter::default(), &[], None, 50, 3);
    assert!(a.contains(&"--skip=3".to_string()) && a.contains(&"--max-count=50".to_string()));
    assert!(!a.iter().any(|x| x.starts_with("--until")), "paging never adds --until");
    assert!(!log_args(&Filter::default(), &[], None, 50, 0).iter().any(|x| x.starts_with("--skip")));
    let c = log_args(&Filter { until: Some(NOW), ..Default::default() }, &[], None, 50, 5);
    assert!(c.contains(&"--until=2026-09-23T12:00:00Z".to_string()) && c.contains(&"--skip=5".to_string()));
}

#[test]
fn authors_one_flag_each() {
    let a = args(&Filter { authors: s(&["Rin"]), ..Default::default() });
    assert_eq!(a, s(&["log", LOG_FORMAT, "--source", "--max-count=200", "--author=Rin", "-i", "-F", "--"]));
    let many = args(&Filter { authors: s(&["dana", " rin@example.com "]), search: Some("ACME-7".into()), ..Default::default() });
    assert_eq!(authors_of(&many), s(&["--author=dana", "--author=rin@example.com"]));
    assert_eq!(many.iter().filter(|x| *x == "-F").count(), 1, "literal flags are given once");
    assert_eq!(authors_of(&args(&Filter { authors: s(&["dana", "Dana", "", "  "]), ..Default::default() })), s(&["--author=dana"]));
    let inj = args(&Filter { authors: s(&["--output=/tmp/pwned"]), ..Default::default() });
    assert!(inj.contains(&"--author=--output=/tmp/pwned".to_string()) && !inj.contains(&"--output=/tmp/pwned".to_string()));
}

#[test]
fn mine_adds_this_repos_email() {
    let f = Filter { authors: s(&["rin"]), mine: true, ..Default::default() };
    assert_eq!(authors_of(&log_args(&f, &[], Some("dana@example.com"), 200, 0)), s(&["--author=rin", "--author=dana@example.com"]));
    assert_eq!(author_patterns(&f, None), s(&["rin"]), "without a repo email, Me adds nothing");
    let off = Filter { mine: false, ..Default::default() };
    assert!(authors_of(&log_args(&off, &[], Some("dana@example.com"), 200, 0)).is_empty(), "Me off: the email is unused");
}

#[test]
fn mine_without_email_skips_repo() {
    let me_only = Filter { mine: true, ..Default::default() };
    assert!(!repo_searched(&me_only, None), "Me alone and no user.email: nothing there is mine");
    assert!(!repo_searched(&me_only, Some("  ")));
    assert!(repo_searched(&me_only, Some("dana@example.com")));
    assert!(repo_searched(&Filter { mine: true, authors: s(&["rin"]), ..Default::default() }, None), "still searched for the other authors");
    assert!(repo_searched(&Filter::default(), None));
    let ctl_only = Filter { mine: true, authors: s(&["\n\t\x01"]), ..Default::default() };
    assert!(!repo_searched(&ctl_only, None), "a chip of control characters only is no author");
    assert!(author_patterns(&ctl_only, None).is_empty());
}

#[test]
fn heads_and_branch() {
    let cur = s(&["1111111111111111111111111111111111111111", "2222222222222222222222222222222222222222"]);
    assert_eq!(heads_for(None, false, &cur), cur);
    assert_eq!(heads_for(Some("origin/prod"), true, &cur), s(&["origin/prod"]));
    assert_eq!(heads_for(Some("origin/prod"), false, &cur), cur, "no such branch: current HEADs");
    assert_eq!(heads_for(Some("--output=/tmp/x"), true, &cur), cur, "an invalid ref is never passed");
    let a = log_args(&Filter::default(), &s(&["origin/prod"]), None, 5, 0);
    assert_eq!(a[a.len() - 3..].to_vec(), s(&["--end-of-options", "origin/prod", "--"]));
    let b = log_args(&Filter::default(), &cur, None, 5, 0);
    assert_eq!(b[b.len() - 4..].to_vec(), [vec!["--end-of-options".to_string()], cur.clone(), vec!["--".to_string()]].concat());
    assert!(!args(&Filter::default()).contains(&"--end-of-options".to_string()));
}

#[test]
fn valid_refs() {
    for ok in ["origin/prod", "prod", "release-1.4", "origin/release/1.4", "feature/acme_7", "v1.2.3", "feature@x", "fix/#12", "fix/ü", "a+b", "a@b/c"] {
        assert!(is_valid_ref(ok), "{ok}");
    }
    for bad in ["", "-x", "--output=/tmp/x", "a..b", "a b", "a~1", "a^", "a:b", "a?", "a*", "a[", "a\\b", "@{u}", "a@{1}", "@", "a.lock", "a/b.lock", "a/", "a.", "/a", "a//b", ".a", "a/.b", "a\tb", "a\u{7f}b"] {
        assert!(!is_valid_ref(bad), "rejects {bad:?}");
    }
}

#[test]
fn branch_fallback_counts() {
    assert_eq!(BranchUse::tally("origin/prod", [true, false, true, false, false]), BranchUse { branch: "origin/prod".into(), found: 2, fallback: 3 });
    assert_eq!(BranchUse::tally("x", []), BranchUse { branch: "x".into(), found: 0, fallback: 0 });
}

#[test]
fn normalize_and_pathspec() {
    assert_eq!(normalize_path(" src/checkout/ ").as_deref(), Some("src/checkout"));
    assert_eq!(normalize_path("./src\\checkout").as_deref(), Some("src/checkout"));
    for bad in ["/etc", "C:\\x", "../x", "a/../../b", ":(top)x", ":!secret", "a\nb", "", "  "] {
        assert_eq!(normalize_path(bad), None, "rejected: {bad:?}");
    }
    assert_eq!(pathspec_of("src/checkout"), ":(literal)src/checkout", "no glob character: literal, and a folder matches everything inside");
    assert_eq!(pathspec_of("app/id/page.tsx"), ":(literal)app/id/page.tsx");
    assert_eq!(pathspec_of("*.sql"), ":(glob)**/*.sql", "a glob without a folder matches at any depth");
    assert_eq!(pathspec_of("app/[id]"), ":(glob)app/[id]", "as in Polylog, [ is a glob character");
    assert_eq!(pathspec_of("**/*.sql"), ":(glob)**/*.sql");
    assert_eq!(pathspec_of("docs/[ab].md"), ":(glob)docs/[ab].md");
}

#[test]
fn path_glob_argv() {
    let a = args(&Filter { path: Some("docs/my notes/*.md".into()), ..Default::default() });
    assert_eq!(a[a.len() - 2..].to_vec(), s(&["--", ":(glob)docs/my notes/*.md"]), "one argv entry, spaces and all");
    let b = args(&Filter { path: Some("*.sql".into()), ..Default::default() });
    assert_eq!(b[b.len() - 2..].to_vec(), s(&["--", ":(glob)**/*.sql"]), "a bare glob matches at any depth");
    let c = args(&Filter { path: Some(":(top)evil".into()), ..Default::default() });
    assert_eq!(c.last().map(String::as_str), Some("--"), "an unsafe path adds nothing");
    let d = log_args(&Filter { path: Some("src".into()), ..Default::default() }, &s(&["main"]), None, 5, 0);
    assert_eq!(d[d.len() - 4..].to_vec(), s(&["--end-of-options", "main", "--", ":(literal)src"]), "heads before --, path after");
}

#[test]
fn control_characters_become_spaces() {
    let a = args(&Filter { search: Some("fix\nfeat\0x\t".into()), authors: s(&["dana\nrin", "\n"]), ..Default::default() });
    assert!(a.contains(&"--grep=fix feat x".to_string()), "a newline under -F would be an OR: {a:?}");
    assert_eq!(authors_of(&a), s(&["--author=dana rin"]), "only spaces left: dropped");
    assert!(a.iter().all(|x| !x.chars().any(|c| (c as u32) < 0x20)), "no control character reaches argv");
    let me = log_args(&Filter { mine: true, ..Default::default() }, &[], Some("dana@example.com\n"), 5, 0);
    assert_eq!(authors_of(&me), s(&["--author=dana@example.com"]));
}
