use claudegate_core::gitlog::{blame_args, parse_blame_porcelain, BlameLine};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn blame_args_shape() {
    assert_eq!(blame_args(A, "-x.ts", 7), ["blame", "--porcelain", "-L7,7", A, "--", "-x.ts"]);
}

#[test]
fn parses_porcelain() {
    let out = format!(
        "{A} 3 7 1\nauthor Ann *Q* <x>\nauthor-mail <ann@example.com>\nauthor-time 1767225600\nauthor-tz +0000\ncommitter Ann\ncommitter-mail <ann@example.com>\ncommitter-time 1767225600\ncommitter-tz +0000\nsummary fix: the \"thing\" [x](y)\nboundary\nfilename \"x\\ny.txt\"\n\tauthor Mallory\n"
    );
    assert_eq!(
        parse_blame_porcelain(&out),
        Some(BlameLine { sha: A.into(), author: "Ann *Q* <x>".into(), email: "ann@example.com".into(), time: 1767225600, summary: "fix: the \"thing\" [x](y)".into() })
    );
}

#[test]
fn source_line_never_reads_as_a_key() {
    let out = format!("{A} 1 1 1\nauthor Ann\nsummary s\nfilename f\n\tsummary evil\n");
    assert_eq!(parse_blame_porcelain(&out).unwrap().summary, "s");
}

#[test]
fn empty_is_none() {
    assert_eq!(parse_blame_porcelain(""), None);
    assert_eq!(parse_blame_porcelain("not a header\nauthor x\n"), None);
    assert_eq!(parse_blame_porcelain(&format!("{A} x y\n")), None);
}

#[test]
fn sha256_header() {
    let s = "b".repeat(64);
    let out = format!("{s} 1 1 1\nauthor Ann\nauthor-mail <a@b>\nauthor-time 5\nsummary hi\nfilename f\n\tx\n");
    let b = parse_blame_porcelain(&out).unwrap();
    assert_eq!((b.sha.as_str(), b.time, b.summary.as_str()), (s.as_str(), 5, "hi"));
}
