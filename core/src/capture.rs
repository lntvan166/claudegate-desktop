//! Pure detector for whether Claude Code is wired to the ClaudeGate capture hook.
//! No filesystem access: callers pass in what they read.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    SetUpByExtension,
    /// `version` is None when the caller has not read the marker's content.
    SetUpByApp { version: Option<String> },
    NotSetUp,
    SettingsUnreadable(String),
    PythonMissing,
}


fn registered(settings: &Value) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(Value::as_array)
        .map(|entries| {
            // Same test as the extension's `isOurs`: JSON.stringify(entry).includes("claudegate").
            // Lean wide: a false negative would let the app install a duplicate entry.
            entries.iter().any(|e| e.to_string().contains("claudegate"))
        })
        .unwrap_or(false)
}

/// Precedence: PythonMissing, SettingsUnreadable, registered+marker, registered, NotSetUp.
/// `settings_text: None` is a missing file; an empty/whitespace file counts as missing
/// too (the extension's installer treats it as a fresh install). Strict JSON only.
pub fn capture_status(settings_text: Option<&str>, marker: bool, python3: bool) -> CaptureStatus {
    if !python3 {
        return CaptureStatus::PythonMissing;
    }
    let Some(text) = settings_text else { return CaptureStatus::NotSetUp };
    if text.trim().is_empty() {
        return CaptureStatus::NotSetUp;
    }
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return CaptureStatus::SettingsUnreadable(format!("not valid JSON: {e}")),
    };
    if !parsed.is_object() {
        return CaptureStatus::SettingsUnreadable("not a JSON object".to_string());
    }
    match (registered(&parsed), marker) {
        (true, true) => CaptureStatus::SetUpByApp { version: None },
        (true, false) => CaptureStatus::SetUpByExtension,
        _ => CaptureStatus::NotSetUp,
    }
}
