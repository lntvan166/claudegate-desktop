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

fn is_sha(s: &str) -> bool {
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

/// Resolve numstat's rename notation (`old => new`, `pre/{a => b}/post`) to the new path.
fn numstat_new_path(p: &str) -> String {
    if let (Some(open), Some(close)) = (p.find('{'), p.rfind('}')) {
        if open < close {
            if let Some((_, new)) = p[open + 1..close].split_once(" => ") {
                let joined = format!("{}{}{}", &p[..open], new, &p[close + 1..]);
                return joined.replace("//", "/");
            }
        }
    }
    match p.split_once(" => ") {
        Some((_, new)) => new.to_string(),
        None => p.to_string(),
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
        let path = numstat_new_path(p);
        if p.is_empty() {
            continue;
        }
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
        let parts: Vec<&str> = if line.contains('\t') {
            line.split('\t').collect()
        } else {
            line.split_whitespace().collect()
        };
        let Some(letter) = parts[0].chars().next() else { continue };
        let two = matches!(letter, 'R' | 'C');
        let need = if two { 3 } else { 2 };
        if parts.len() < need {
            continue;
        }
        let path = parts[need - 1].to_string();
        let status = match letter {
            'A' => FileStatus::Added,
            'M' => FileStatus::Modified,
            'D' => FileStatus::Deleted,
            'T' => FileStatus::TypeChanged,
            'R' => FileStatus::Renamed { from: parts[1].to_string() },
            'C' => FileStatus::Copied { from: parts[1].to_string() },
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
