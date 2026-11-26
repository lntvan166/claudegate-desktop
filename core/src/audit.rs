//! Reads the capture audit that the extension's `hooks/audit.py` appends to
//! `<home>/audit/<md5(root)>.jsonl` (rotated to `.jsonl.1`): one line per Bash call and per
//! innermost registered root. Read-only; a line that does not parse, or that is flagged
//! `background`, is skipped.
//!
//! Line fields used (camelCase, as audit.py writes them): `at` (ISO 8601 string; an epoch-ms
//! number is accepted too), `root`, `commandHead`, `background`, `truncated`, the path lists
//! `missed` / `uncertain` (capped at 200 per line) with their uncapped `missedCount` /
//! `uncertainCount`, `phantomCount` (or the length of `phantom`), and the counters `captured`,
//! `covered`, `implicit`, `outOfScope`.

use crate::paths::audit_file_for;
use crate::time::parse_iso_ms;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const WINDOW_MS: i64 = 7 * 24 * 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditItem {
    pub path: String,
    pub at_ms: i64,
    pub command_head: String,
    pub root: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditStats {
    /// Non-background lines in the window.
    pub calls: u64,
    pub captured: u64,
    pub missed: u64,
    pub uncertain: u64,
    pub phantom: u64,
    pub covered: u64,
    pub implicit: u64,
    pub out_of_scope: u64,
    /// Lines whose scan ran out of budget.
    pub truncated: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditView {
    pub missed: Vec<AuditItem>,
    pub uncertain: Vec<AuditItem>,
    pub stats: AuditStats,
}

fn at_ms_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => parse_iso_ms(s),
        _ => None,
    }
}

fn count(line: &Value, key: &str) -> u64 {
    line.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// A list's true size: its `<key>Count` when present (the list itself is capped), else its length.
fn list_count(line: &Value, key: &str) -> u64 {
    match line.get(format!("{key}Count")).and_then(Value::as_u64) {
        Some(n) => n,
        None => line.get(key).and_then(Value::as_array).map_or(0, |a| a.len() as u64),
    }
}

/// Every audit line of `roots` (both files each) inside the window, deduplicated by path.
/// `roots` are the project's scopes: its primary root, its worktrees and its nested roots.
pub fn read_audit(home: &Path, roots: &[PathBuf], since_ms: i64) -> AuditView {
    let mut files: Vec<PathBuf> = vec![];
    for r in roots {
        let f = audit_file_for(home, r);
        let old = PathBuf::from(format!("{}.1", f.display()));
        for p in [old, f] {
            if !files.contains(&p) {
                files.push(p);
            }
        }
    }
    let mut stats = AuditStats::default();
    // path -> (is_uncertain, item): the latest sighting of a path wins, across both lists.
    let mut latest: HashMap<String, (bool, AuditItem)> = HashMap::new();
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        for raw in text.lines() {
            let Ok(line) = serde_json::from_str::<Value>(raw) else { continue };
            if !line.is_object() || line.get("background").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let Some(at) = line.get("at").and_then(at_ms_of) else { continue };
            if at < since_ms {
                continue;
            }
            stats.calls += 1;
            stats.captured += count(&line, "captured");
            stats.missed += list_count(&line, "missed");
            stats.uncertain += list_count(&line, "uncertain");
            stats.phantom += list_count(&line, "phantom");
            stats.covered += count(&line, "covered");
            stats.implicit += count(&line, "implicit");
            stats.out_of_scope += count(&line, "outOfScope");
            if line.get("truncated").and_then(Value::as_bool) == Some(true) {
                stats.truncated += 1;
            }
            let head = line.get("commandHead").and_then(Value::as_str).unwrap_or("");
            let root = line.get("root").and_then(Value::as_str).unwrap_or("");
            for (key, uncertain) in [("missed", false), ("uncertain", true)] {
                let Some(list) = line.get(key).and_then(Value::as_array) else { continue };
                for p in list.iter().filter_map(Value::as_str) {
                    let newer = latest.get(p).map_or(true, |(u, i)| at > i.at_ms || (at == i.at_ms && *u && !uncertain));
                    if newer {
                        let item = AuditItem { path: p.into(), at_ms: at, command_head: head.into(), root: root.into() };
                        latest.insert(p.into(), (uncertain, item));
                    }
                }
            }
        }
    }
    let mut view = AuditView { stats, ..Default::default() };
    for (uncertain, item) in latest.into_values() {
        if uncertain { view.uncertain.push(item) } else { view.missed.push(item) }
    }
    for l in [&mut view.missed, &mut view.uncertain] {
        l.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| a.path.cmp(&b.path)));
    }
    view
}
