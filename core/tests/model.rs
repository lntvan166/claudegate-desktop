use claudegate_core::model::{has_real_change, parse_session, Shape};
use serde_json::Value;
use std::path::PathBuf;

fn fixture(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../fixtures/sessions")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn check(name: &str) {
    let fx = fixture(name);
    let input = serde_json::to_string(&fx["input"]).unwrap();
    let (session, shape) = parse_session(&input).unwrap();
    assert_eq!(serde_json::to_value(&session).unwrap(), fx["expected"], "{name}: session");
    match (&fx["shape"], shape) {
        (Value::String(s), Shape::Known) => assert_eq!(s, "known"),
        (Value::Array(want), Shape::Unknown(got)) => {
            let want: Vec<&str> = want.iter().map(|v| v.as_str().unwrap()).collect();
            assert_eq!(want, got, "{name}: shape");
        }
        (want, got) => panic!("{name}: shape mismatch, want {want}, got {got:?}"),
    }
}

#[test]
fn current() {
    check("current");
}

#[test]
fn legacy_statuses() {
    check("legacy-statuses");
}

#[test]
fn missing_top_level() {
    check("missing-top-level");
}

#[test]
fn unknown_field() {
    check("unknown-field");
}

#[test]
fn accepted_over_cap() {
    check("accepted-over-cap");
    let (s, _) = parse_session(&serde_json::to_string(&fixture("accepted-over-cap")["input"]).unwrap()).unwrap();
    assert_eq!(s.accepted.len(), 500);
}

#[test]
fn real_change() {
    assert!(!has_real_change(None, None));
    assert!(has_real_change(None, Some("x")));
    assert!(!has_real_change(Some("a"), Some("a")));
    assert!(has_real_change(Some("a"), None));
}

#[test]
fn bad_json_is_error() {
    assert!(parse_session("").is_err());
    assert!(parse_session("{").is_err());
}

fn shape_of(json: &str) -> Shape {
    parse_session(json).unwrap().1
}

fn unknown(paths: &[&str]) -> Shape {
    Shape::Unknown(paths.iter().map(|s| s.to_string()).collect())
}

#[test]
fn bad_accepted_record_is_reported() {
    let j = r#"{"sessionId":"s","accepted":[3,{"path":"/a","before":5,"decidedAt":"d"}]}"#;
    assert_eq!(shape_of(j), unknown(&["accepted.0", "accepted.1.before", "accepted.1.id"]));
}

#[test]
fn bad_rejected_record_is_reported() {
    let j = r#"{"sessionId":"s","rejected":{"/a":"x","/b":{"id":"i","path":"/b","decidedAt":7}}}"#;
    assert_eq!(shape_of(j), unknown(&["rejected./a", "rejected./b.decidedAt"]));
}

#[test]
fn non_string_original_content_is_reported() {
    let j = r#"{"sessionId":"s","files":{"/a":{"originalContent":42,"reviewStatus":"pending"}}}"#;
    assert_eq!(shape_of(j), unknown(&["files./a.originalContent"]));
}

#[test]
fn non_object_file_entry_is_reported() {
    let j = r#"{"sessionId":"s","files":{"/a":"oops","/b":{"originalContent":null}}}"#;
    assert_eq!(shape_of(j), unknown(&["files./a"]));
}

#[test]
fn wrong_container_types_are_reported() {
    let j = r#"{"sessionId":1,"files":[],"accepted":{},"rejected":null}"#;
    assert_eq!(shape_of(j), unknown(&["sessionId", "files", "accepted", "rejected"]));
}

#[test]
fn hook_shaped_entry_is_known() {
    let j = r#"{"sessionId":"s","status":"active","files":{"/a":{"originalContent":null,"reviewStatus":"pending","newFile":true,"sessionId":null,"capturedAt":"2026-05-31T10:00:00+00:00"}},"accepted":[{"id":"i","path":"/a","before":null,"after":"x","decidedAt":"d","sessionId":null,"newFile":true}],"rejected":{}}"#;
    let (s, shape) = parse_session(j).unwrap();
    assert_eq!(shape, Shape::Known);
    assert_eq!(s.files["/a"].session_id, None);
}
