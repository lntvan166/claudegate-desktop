//! The audit reader on hand-written audit files in a temp dir.
use claudegate_core::audit::{read_audit, WINDOW_MS};
use claudegate_core::paths::audit_file_for;
use claudegate_core::time::parse_iso_ms;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const ROOT: &str = "/work/acme/acme-api";
const T0: i64 = 1_794_564_000_000; // 2026-11-13T10:00:00Z

fn iso(ms: i64) -> String {
    // Only whole seconds are used by the tests.
    let secs = ms / 1000;
    let (d, r) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // days -> civil date (Howard Hinnant)
    let z = d + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}+00:00", r / 3600, r % 3600 / 60, r % 60)
}

fn line(at: i64, missed: &[&str], uncertain: &[&str]) -> Value {
    json!({
        "at": iso(at), "sessionId": "s1", "toolUseId": "t", "event": "PostToolUse", "durationMs": 40,
        "cwd": ROOT, "root": ROOT, "commandHead": "gofmt -w ./...", "background": false, "truncated": false,
        "missed": missed, "uncertain": uncertain, "phantom": [],
        "missedCount": missed.len(), "uncertainCount": uncertain.len(), "phantomCount": 0,
        "covered": 0, "implicit": 0, "outOfScope": 0, "tooLarge": 0, "captured": 1
    })
}

fn write(home: &Path, root: &str, suffix: &str, lines: &[String]) {
    let f = audit_file_for(home, Path::new(root));
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    let name = format!("{}{suffix}", f.display());
    fs::write(name, lines.join("\n") + "\n").unwrap();
}

fn s(v: &Value) -> String {
    v.to_string()
}

fn roots() -> Vec<PathBuf> {
    vec![PathBuf::from(ROOT)]
}

#[test]
fn iso_helper_round_trips() {
    assert_eq!(parse_iso_ms(&iso(T0)), Some(T0));
}

#[test]
fn reads_both_files() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), ROOT, ".1", &[s(&line(T0, &["/work/acme/acme-api/old.go"], &[]))]);
    write(t.path(), ROOT, "", &[s(&line(T0 + 1000, &["/work/acme/acme-api/new.go"], &[]))]);
    let v = read_audit(t.path(), &roots(), T0 - 1000);
    let paths: Vec<_> = v.missed.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["/work/acme/acme-api/new.go", "/work/acme/acme-api/old.go"]);
    assert_eq!(v.stats.calls, 2);
    assert_eq!(v.stats.captured, 2);
    assert_eq!(v.stats.missed, 2);
    assert_eq!(v.missed[0].command_head, "gofmt -w ./...");
    assert_eq!(v.missed[0].root, ROOT);
}

#[test]
fn skips_corrupt_lines() {
    let t = tempfile::tempdir().unwrap();
    write(
        t.path(),
        ROOT,
        "",
        &[
            "{not json".into(),
            "[1,2]".into(),
            s(&json!({"missed": ["/work/acme/acme-api/no-at.go"]})),
            s(&line(T0, &["/work/acme/acme-api/ok.go"], &[])),
            "{\"at\":".into(),
        ],
    );
    let v = read_audit(t.path(), &roots(), 0);
    assert_eq!(v.missed.len(), 1);
    assert_eq!(v.stats.calls, 1);
}

#[test]
fn skips_background() {
    let t = tempfile::tempdir().unwrap();
    let mut bg = line(T0, &["/work/acme/acme-api/bg.go"], &[]);
    bg["background"] = json!(true);
    write(t.path(), ROOT, "", &[s(&bg)]);
    let v = read_audit(t.path(), &roots(), 0);
    assert!(v.missed.is_empty());
    assert_eq!(v.stats.calls, 0);
    assert_eq!(v.stats.captured, 0);
}

#[test]
fn dedups_latest() {
    let t = tempfile::tempdir().unwrap();
    let p = "/work/acme/acme-api/a.go";
    let mut later = line(T0 + 5000, &[p], &[]);
    later["commandHead"] = json!("prettier --write .");
    // The file order is not the time order: the later miss comes first.
    write(t.path(), ROOT, "", &[s(&later), s(&line(T0, &[p], &[]))]);
    let v = read_audit(t.path(), &roots(), 0);
    assert_eq!(v.missed.len(), 1);
    assert_eq!(v.missed[0].at_ms, T0 + 5000);
    assert_eq!(v.missed[0].command_head, "prettier --write .");
    // Stats still count both calls.
    assert_eq!((v.stats.calls, v.stats.missed), (2, 2));
}

#[test]
fn latest_sighting_decides_the_list() {
    let t = tempfile::tempdir().unwrap();
    let p = "/work/acme/acme-api/a.go";
    write(t.path(), ROOT, "", &[s(&line(T0, &[p], &[])), s(&line(T0 + 1000, &[], &[p]))]);
    let v = read_audit(t.path(), &roots(), 0);
    assert!(v.missed.is_empty());
    assert_eq!(v.uncertain.len(), 1);
    assert_eq!(v.stats.uncertain, 1);
}

#[test]
fn window_7_days() {
    let t = tempfile::tempdir().unwrap();
    let now = T0 + 10 * 86_400_000;
    let since = now - WINDOW_MS;
    write(
        t.path(),
        ROOT,
        "",
        &[
            s(&line(since - 1000, &["/work/acme/acme-api/stale.go"], &[])),
            s(&line(since + 1000, &["/work/acme/acme-api/fresh.go"], &[])),
        ],
    );
    let v = read_audit(t.path(), &roots(), since);
    assert_eq!(v.missed.len(), 1);
    assert_eq!(v.missed[0].path, "/work/acme/acme-api/fresh.go");
    assert_eq!(v.stats.calls, 1);
}

#[test]
fn counts_come_from_the_count_fields_and_at_may_be_epoch_ms() {
    let t = tempfile::tempdir().unwrap();
    let mut l = line(T0, &["/work/acme/acme-api/a.go"], &[]);
    l["at"] = json!(T0);
    l["missedCount"] = json!(300); // the list is capped at 200 per line
    l["phantomCount"] = json!(2);
    l["phantom"] = json!(["/x", "/y"]);
    l["covered"] = json!(3);
    l["implicit"] = json!(4);
    l["outOfScope"] = json!(5);
    l["truncated"] = json!(true);
    write(t.path(), ROOT, "", &[s(&l)]);
    let v = read_audit(t.path(), &roots(), 0);
    let st = v.stats;
    assert_eq!(
        (st.calls, st.captured, st.missed, st.phantom, st.covered, st.implicit, st.out_of_scope, st.truncated),
        (1, 1, 300, 2, 3, 4, 5, 1)
    );
}
