//! Read-only port of the extension's `reviewModel.ts` (v1.16.1): the session
//! types, `migrateSession`, `capAcceptedLog` and `hasRealChange`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Upper bound on `accepted[]`; the oldest records are dropped first.
pub const MAX_ACCEPTED_RECORDS: usize = 500;

const TOP_FIELDS: &[&str] = &["sessionId", "status", "files", "accepted", "rejected", "excludeCleanupDone"];
const ENTRY_FIELDS: &[&str] =
    &["originalContent", "reviewStatus", "newFile", "sessionId", "capturedAt", "claudeContent"];
const RECORD_FIELDS: &[&str] = &["id", "path", "before", "after", "decidedAt", "sessionId", "newFile", "reason"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Reviewed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    /// `None` means Claude created the file; serialized as `null`.
    pub original_content: Option<String>,
    pub new_file: Option<bool>,
    pub session_id: Option<String>,
    pub captured_at: Option<String>,
}

/// Serializes as `{ "reviewStatus": "pending", ... }`: `files{}` only ever holds pending entries.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingOut<'a> {
    original_content: &'a Option<String>,
    review_status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_file: &'a Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_at: &'a Option<String>,
}

impl Serialize for FileEntry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        PendingOut {
            original_content: &self.original_content,
            review_status: "pending",
            new_file: &self.new_file,
            session_id: &self.session_id,
            captured_at: &self.captured_at,
        }
        .serialize(s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRecord {
    pub id: String,
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub decided_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_file: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub session_id: String,
    pub status: Status,
    pub files: BTreeMap<String, FileEntry>,
    pub accepted: Vec<ReviewRecord>,
    pub rejected: BTreeMap<String, ReviewRecord>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exclude_cleanup_done: bool,
}

/// Whether the raw JSON had only fields the extension (v1.16.1) knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    Known,
    /// Offending field paths, e.g. `files./a.ts.foo`.
    Unknown(Vec<String>),
}

#[derive(Debug)]
pub enum SessionError {
    Json(serde_json::Error),
    NotAnObject,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Json(e) => write!(f, "invalid session JSON: {e}"),
            SessionError::NotAnObject => write!(f, "session JSON is not an object"),
        }
    }
}

impl std::error::Error for SessionError {}

pub fn make_record_id(decided_at: &str, path: &str) -> String {
    format!("{decided_at}::{path}")
}

/// A pending entry is a real change unless its baseline equals the disk content.
/// `None` disk means the file is absent.
pub fn has_real_change(original: Option<&str>, disk: Option<&str>) -> bool {
    match original {
        None => disk.is_some(),
        Some(o) => Some(o) != disk,
    }
}

pub fn parse_session(text: &str) -> Result<(Session, Shape), SessionError> {
    let raw: Value = serde_json::from_str(text).map_err(SessionError::Json)?;
    let obj = raw.as_object().ok_or(SessionError::NotAnObject)?;
    let shape = check_shape(obj);
    Ok((migrate(obj), shape))
}

fn unknown_keys(obj: &Map<String, Value>, known: &[&str], prefix: &str, out: &mut Vec<String>) {
    for k in obj.keys() {
        if !known.contains(&k.as_str()) {
            out.push(format!("{prefix}{k}"));
        }
    }
}

#[derive(Clone, Copy)]
enum Ty {
    Str,
    /// A string or `null`.
    StrOrNull,
    Bool,
    Object,
    Array,
}

impl Ty {
    fn matches(self, v: &Value) -> bool {
        match self {
            Ty::Str => v.is_string(),
            Ty::StrOrNull => v.is_string() || v.is_null(),
            Ty::Bool => v.is_boolean(),
            Ty::Object => v.is_object(),
            Ty::Array => v.is_array(),
        }
    }
}

/// Report each present field of the wrong type. A missing field is not wrong-typed.
fn check_types(obj: &Map<String, Value>, fields: &[(&str, Ty)], prefix: &str, out: &mut Vec<String>) {
    for (key, ty) in fields {
        if obj.get(*key).is_some_and(|v| !ty.matches(v)) {
            out.push(format!("{prefix}{key}"));
        }
    }
}

const ENTRY_TYPES: &[(&str, Ty)] = &[
    ("originalContent", Ty::StrOrNull),
    ("reviewStatus", Ty::Str),
    ("newFile", Ty::Bool),
    // hook.py writes `"sessionId": null` when the hook payload has no session_id.
    ("sessionId", Ty::StrOrNull),
    ("capturedAt", Ty::Str),
    ("claudeContent", Ty::StrOrNull),
];

const RECORD_TYPES: &[(&str, Ty)] = &[
    ("id", Ty::Str),
    ("path", Ty::Str),
    ("before", Ty::StrOrNull),
    ("after", Ty::StrOrNull),
    ("decidedAt", Ty::Str),
    // Copied from the entry, so it can be null too.
    ("sessionId", Ty::StrOrNull),
    ("newFile", Ty::Bool),
    ("reason", Ty::Str),
];

fn check_record(r: &Value, prefix: &str, out: &mut Vec<String>) {
    let Some(r) = r.as_object() else {
        out.push(prefix.to_owned());
        return;
    };
    let p = format!("{prefix}.");
    unknown_keys(r, RECORD_FIELDS, &p, out);
    check_types(r, RECORD_TYPES, &p, out);
    // id, path and decidedAt are required; a record without them is not loadable.
    for key in ["id", "path", "decidedAt"] {
        if !r.contains_key(key) {
            out.push(format!("{p}{key}"));
        }
    }
}

fn check_shape(top: &Map<String, Value>) -> Shape {
    let mut bad = Vec::new();
    unknown_keys(top, TOP_FIELDS, "", &mut bad);
    check_types(
        top,
        &[
            ("sessionId", Ty::Str),
            ("status", Ty::Str),
            ("files", Ty::Object),
            ("accepted", Ty::Array),
            ("rejected", Ty::Object),
            ("excludeCleanupDone", Ty::Bool),
        ],
        "",
        &mut bad,
    );
    if let Some(files) = top.get("files").and_then(Value::as_object) {
        for (path, e) in files {
            let prefix = format!("files.{path}");
            let Some(e) = e.as_object() else {
                bad.push(prefix);
                continue;
            };
            let p = format!("{prefix}.");
            unknown_keys(e, ENTRY_FIELDS, &p, &mut bad);
            check_types(e, ENTRY_TYPES, &p, &mut bad);
        }
    }
    if let Some(list) = top.get("accepted").and_then(Value::as_array) {
        for (i, r) in list.iter().enumerate() {
            check_record(r, &format!("accepted.{i}"), &mut bad);
        }
    }
    if let Some(map) = top.get("rejected").and_then(Value::as_object) {
        for (path, r) in map {
            check_record(r, &format!("rejected.{path}"), &mut bad);
        }
    }
    if bad.is_empty() {
        Shape::Known
    } else {
        Shape::Unknown(bad)
    }
}

fn str_field(e: &Value, key: &str) -> Option<String> {
    e.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Port of `migrateSession`. Unlike the TS, a pre-existing `accepted`/`rejected`
/// item that is not record-shaped is skipped rather than passed through.
fn migrate(raw: &Map<String, Value>) -> Session {
    let session_id = raw
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(now_iso);
    let status = if raw.get("status").and_then(Value::as_str) == Some("reviewed") {
        Status::Reviewed
    } else {
        Status::Active
    };
    let mut accepted: Vec<ReviewRecord> = raw
        .get("accepted")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
        .unwrap_or_default();
    let mut rejected: BTreeMap<String, ReviewRecord> = raw
        .get("rejected")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| serde_json::from_value(v.clone()).ok().map(|r| (k.clone(), r)))
                .collect()
        })
        .unwrap_or_default();
    let mut files = BTreeMap::new();

    if let Some(raw_files) = raw.get("files").and_then(Value::as_object) {
        for (path, e) in raw_files {
            let status = e.get("reviewStatus").and_then(Value::as_str);
            let original = str_field(e, "originalContent");
            if status == Some("accepted") || status == Some("rejected") {
                let decided_at = str_field(e, "capturedAt").unwrap_or_else(|| session_id.clone());
                let after = str_field(e, "claudeContent").or_else(|| original.clone());
                let rec = ReviewRecord {
                    id: make_record_id(&decided_at, path),
                    path: path.clone(),
                    before: original,
                    after,
                    decided_at,
                    session_id: str_field(e, "sessionId"),
                    new_file: None,
                    reason: None,
                };
                if status == Some("accepted") {
                    accepted.push(rec);
                } else {
                    rejected.insert(path.clone(), rec);
                }
            } else {
                files.insert(
                    path.clone(),
                    FileEntry {
                        original_content: original,
                        new_file: e.get("newFile").and_then(Value::as_bool),
                        session_id: str_field(e, "sessionId"),
                        captured_at: str_field(e, "capturedAt"),
                    },
                );
            }
        }
    }
    // capAcceptedLog: drop oldest-first.
    if accepted.len() > MAX_ACCEPTED_RECORDS {
        let excess = accepted.len() - MAX_ACCEPTED_RECORDS;
        accepted.drain(..excess);
    }
    Session {
        session_id,
        status,
        files,
        accepted,
        rejected,
        exclude_cleanup_done: raw.get("excludeCleanupDone") == Some(&Value::Bool(true)),
    }
}

/// `new Date().toISOString()` stand-in, used only when a session has no `sessionId`.
fn now_iso() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let (secs, ms) = (d.as_secs() as i64, d.subsec_millis());
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
