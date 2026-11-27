//! Blame of one line: `git blame --porcelain -L n,n`, read into who last changed it
//! (Polylog's blame, one line at a time).
//!
//! The porcelain output is `<sha> <orig> <final> <n>`, then `key value` lines, then the source
//! line itself behind a TAB. An author name or a summary can hold anything but a newline, so keys
//! are read only before that TAB line, and only the first time each is seen.

use super::parse::is_sha;

/// `git blame` for the one 1-based `line` of `path` at `rev`. `rev` is a commit id the caller
/// validated; `--` keeps the path from reading as an option or a revision. `--no-textconv`: the
/// diff shows the raw blob, so line numbers must count the raw blob's lines, not a converted text.
pub fn blame_args(rev: &str, path: &str, line: u32) -> Vec<String> {
    ["blame", "--porcelain", "--no-textconv", &format!("-L{line},{line}"), rev, "--", path].map(String::from).to_vec()
}

/// The commit that last changed a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub sha: String,
    pub author: String,
    pub email: String,
    /// Seconds since the epoch.
    pub time: i64,
    pub summary: String,
}

/// The first record of `git blame --porcelain` output, or `None` when there is none (an empty
/// output, or one that does not start with a commit header).
pub fn parse_blame_porcelain(out: &str) -> Option<BlameLine> {
    let mut lines = out.lines();
    let head = lines.next()?;
    let mut parts = head.split(' ');
    let sha = parts.next().filter(|s| is_sha(s))?;
    // `<orig> <final> [<n>]`: all numbers, or this is not a header.
    let nums: Vec<&str> = parts.collect();
    if nums.len() < 2 || nums.len() > 3 || nums.iter().any(|n| n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let mut b = BlameLine { sha: sha.to_string(), author: String::new(), email: String::new(), time: 0, summary: String::new() };
    let (mut has_author, mut has_email, mut has_time, mut has_summary) = (false, false, false, false);
    for line in lines {
        // The source line (and so the end of the keys).
        if line.starts_with('\t') {
            break;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "author" if !has_author => (b.author, has_author) = (value.to_string(), true),
            "author-mail" if !has_email => {
                let v = value.strip_prefix('<').unwrap_or(value);
                (b.email, has_email) = (v.strip_suffix('>').unwrap_or(v).to_string(), true);
            }
            "author-time" if !has_time => {
                (b.time, has_time) = (value.trim().parse().unwrap_or(0), true);
            }
            "summary" if !has_summary => (b.summary, has_summary) = (value.to_string(), true),
            _ => {}
        }
    }
    Some(b)
}
