//! `fixtures/merge/*.json`, whose `expected` values come from the extension's own
//! `mergeFreshCaptures` / `acceptEntry` / `rejectEntry` (v1.16.1, see xcheck/run.ts).

use claudegate_core::merge::{accept_entry, merge_fresh_captures, reject_entry, MergeGuards};
use claudegate_core::model::{parse_session, ReviewRecord, Session};
use serde_json::Value;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/merge")
}

fn session(v: &Value) -> Session {
    parse_session(&serde_json::to_string(v).unwrap()).unwrap().0
}

fn strings(v: &Value) -> impl Iterator<Item = String> + '_ {
    v.as_array().into_iter().flatten().map(|s| s.as_str().unwrap().to_owned())
}

fn guards(v: &Value) -> MergeGuards {
    let mut g = MergeGuards::default();
    if let Some(m) = v.get("prunedFiles").and_then(Value::as_object) {
        for (k, c) in m {
            g.pruned_files.insert(k.clone(), c.as_str().map(str::to_owned));
        }
    }
    g.dropped_accepted_ids.extend(strings(&v["droppedAcceptedIds"]));
    g.dropped_rejected_paths.extend(strings(&v["droppedRejectedPaths"]));
    g
}

fn run(fx: &Value) -> Value {
    let got = match fx.get("op").and_then(Value::as_str) {
        None => merge_fresh_captures(session(&fx["mine"]), session(&fx["disk"]), &guards(&fx["guards"])),
        Some(op) => {
            let mut s = session(&fx["session"]);
            let path = fx["path"].as_str().unwrap();
            let after = fx["after"].as_str().map(str::to_owned);
            let at = fx["decidedAt"].as_str().unwrap();
            match op {
                "accept" => accept_entry(&mut s, path, after, at),
                "reject" => reject_entry(&mut s, path, after, at),
                other => panic!("unknown op {other}"),
            }
            s
        }
    };
    serde_json::to_value(&got).unwrap()
}

#[test]
fn fixtures_match_the_extension() {
    let mut names: Vec<_> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    names.sort();
    assert!(names.len() >= 19, "expected every merge fixture, found {}", names.len());
    let mut failed = Vec::new();
    for p in &names {
        let fx: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        if run(&fx) != fx["expected"] {
            failed.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(failed.is_empty(), "mismatched fixtures: {failed:?}");
}

fn rec(i: usize, decided_at: &str) -> ReviewRecord {
    ReviewRecord {
        id: format!("{decided_at}::/p{i}"),
        path: format!("/p{i}"),
        before: None,
        after: None,
        decided_at: decided_at.to_owned(),
        session_id: None,
        new_file: None,
        reason: None,
    }
}

/// The TS comparator is not a total order once a `decidedAt` is unparseable. Rust's `sort_by`
/// may panic on such a comparator, so a large log with a mix must still merge.
#[test]
fn large_union_with_unparseable_times_does_not_panic() {
    let empty = r#"{"sessionId":"s","status":"active","files":{},"accepted":[],"rejected":{}}"#;
    let (mut mine, _) = parse_session(empty).unwrap();
    let (mut disk, _) = parse_session(empty).unwrap();
    for i in 0..300 {
        let t = if i % 7 == 0 { "bad".to_owned() } else { format!("2026-11-16T10:{:02}:{:02}.000Z", (i * 37) % 60, i % 60) };
        if i % 2 == 0 { mine.accepted.push(rec(i, &t)) } else { disk.accepted.push(rec(i, &t)) }
    }
    let out = merge_fresh_captures(mine, disk, &MergeGuards::default());
    assert_eq!(out.accepted.len(), 300);
}
