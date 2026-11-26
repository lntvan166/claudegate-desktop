//! Pure parsers for git output. No process is spawned here.

use std::collections::HashMap;

/// Fields separated by NUL, records terminated by RS (0x1e).
pub const LOG_FORMAT: &str = "--format=%H%x00%ct%x00%an%x00%ae%x00%s%x00%P%x1e";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// The repo id; set by the caller, empty after `parse_log`.
    pub repo: String,
    pub sha: String,
    pub time: i64,
    pub author: String,
    pub email: String,
    pub subject: String,
    pub parents: Vec<String>,
}

pub fn is_sha(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn parse_log(out: &str) -> Vec<Commit> {
    let mut records: Vec<&str> = out.split('\x1e').collect();
    // What follows the last RS is git's trailing newline or a record cut off
    // by a killed process; neither is a commit.
    records.pop();
    let mut commits = Vec::new();
    for rec in records {
        let rec = rec.strip_prefix('\n').unwrap_or(rec);
        let f: Vec<&str> = rec.split('\0').collect();
        if f.len() != 6 {
            continue;
        }
        let Ok(time) = f[1].parse::<i64>() else { continue };
        if !is_sha(f[0]) {
            continue;
        }
        commits.push(Commit {
            repo: String::new(),
            sha: f[0].to_string(),
            time,
            author: f[2].to_string(),
            email: f[3].to_string(),
            subject: f[4].to_string(),
            parents: f[5].split(' ').filter(|p| !p.is_empty()).map(String::from).collect(),
        });
    }
    commits
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
    Copied { from: String },
    TypeChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    /// `None` means binary.
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// Undo git's C-style path quoting (`core.quotepath`): a path wrapped in `"` with
/// `\\`, `\"`, `\t`, `\n`, `\a`, `\b`, `\f`, `\r`, `\v` and `\NNN` octal bytes.
/// A string that is not wrapped in quotes is returned unchanged.
pub fn unquote_c_style(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
        return s.to_string();
    };
    let b = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        match b[i] {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(11),
            b'0'..=b'7' => {
                let mut v: u32 = 0;
                let mut n = 0;
                while n < 3 && i < b.len() && matches!(b[i], b'0'..=b'7') {
                    v = v * 8 + (b[i] - b'0') as u32;
                    i += 1;
                    n += 1;
                }
                out.push(v as u8);
                continue;
            }
            c => out.push(c), // `\\`, `\"` and anything else: the character itself
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Byte offset of the first / last `pat` that is not inside a quoted section.
fn find_unquoted(s: &str, pat: &str, last: bool) -> Option<usize> {
    let b = s.as_bytes();
    let (mut in_q, mut i, mut found) = (false, 0, None);
    while i < b.len() {
        match b[i] {
            b'\\' if in_q => i += 1,
            b'"' => in_q = !in_q,
            _ if !in_q && s[i..].starts_with(pat) => {
                found = Some(i);
                if !last {
                    return found;
                }
            }
            _ => {}
        }
        i += 1;
    }
    found
}

/// Resolve numstat's rename notation (`old => new`, `pre/{a => b}/post`) to the new path,
/// unquoting each piece.
fn numstat_new_path(p: &str) -> String {
    if let (Some(open), Some(close)) = (find_unquoted(p, "{", false), find_unquoted(p, "}", true)) {
        if open < close {
            if let Some(arrow) = find_unquoted(&p[open + 1..close], " => ", false) {
                let inner = &p[open + 1..close];
                let new = unquote_c_style(&inner[arrow + 4..]);
                let pre = unquote_c_style(&p[..open]);
                let post = unquote_c_style(&p[close + 1..]);
                // git drops the slash on an empty side: `a/{b => }/c` is `a/c`, so
                // joining pieces would leave `a//c`; remove only that one join.
                let post = if new.is_empty() && pre.ends_with('/') {
                    post.strip_prefix('/').unwrap_or(&post).to_string()
                } else {
                    post
                };
                return format!("{pre}{new}{post}");
            }
        }
    }
    match find_unquoted(p, " => ", false) {
        Some(i) => unquote_c_style(&p[i + 4..]),
        None => unquote_c_style(p),
    }
}

/// `name_status` is `git show --name-status` text (tab separated), `numstat`
/// is `--numstat` text. Order follows name-status; numstat-only files count as modified.
pub fn parse_changes(name_status: &str, numstat: &str) -> Vec<ChangedFile> {
    let mut stats: HashMap<String, (Option<u32>, Option<u32>)> = HashMap::new();
    let mut stat_order: Vec<String> = Vec::new();
    for line in numstat.lines() {
        let mut it = line.splitn(3, '\t');
        let (Some(a), Some(d), Some(p)) = (it.next(), it.next(), it.next()) else { continue };
        let num = |s: &str| if s == "-" { Some(None) } else { s.parse::<u32>().ok().map(Some) };
        let (Some(a), Some(d)) = (num(a), num(d)) else { continue };
        if p.is_empty() {
            continue;
        }
        let path = numstat_new_path(p);
        stat_order.push(path.clone());
        stats.insert(path, (a, d));
    }

    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in name_status.lines() {
        let line = line.trim_start_matches('\n');
        if line.is_empty() {
            continue;
        }
        let parts: Vec<String> = line.split('\t').map(unquote_c_style).collect();
        let Some(letter) = parts[0].chars().next() else { continue };
        let two = matches!(letter, 'R' | 'C');
        let need = if two { 3 } else { 2 };
        if parts.len() < need {
            continue;
        }
        let path = parts[need - 1].clone();
        let status = match letter {
            'A' => FileStatus::Added,
            'M' => FileStatus::Modified,
            'D' => FileStatus::Deleted,
            'T' => FileStatus::TypeChanged,
            'R' => FileStatus::Renamed { from: parts[1].clone() },
            'C' => FileStatus::Copied { from: parts[1].clone() },
            _ => continue,
        };
        let (added, removed) = stats.get(&path).copied().unwrap_or((None, None));
        seen.insert(path.clone());
        files.push(ChangedFile { path, status, added, removed });
    }
    for path in stat_order {
        if seen.insert(path.clone()) {
            let (added, removed) = stats[&path];
            files.push(ChangedFile { path, status: FileStatus::Modified, added, removed });
        }
    }
    files
}
