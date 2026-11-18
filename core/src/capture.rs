//! Capture setup: whether Claude Code is wired to the ClaudeGate capture hook, and the pure half
//! of setting it up. Everything here is pure (callers pass in what they read) except
//! `set_up_capture`, which hands the writes to `store::install_capture`.
//!
//! `compute_settings_patch`, `backups_to_prune` and `verify_settings_content` are ports of the
//! extension's `hookInstaller.ts` (v1.16.1), checked against its own TypeScript by
//! `fixtures/capture/*.json` and `xcheck/run.ts`.

use crate::ojson::OJson;
use serde_json::Value;
use std::path::{Path, PathBuf};

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

/// The `PreToolUse` matcher the extension registers (`PRE_TOOL_MATCHER`).
pub const PRE_TOOL_MATCHER: &str = "^(Write|Edit|MultiEdit|Bash)$";
/// Timestamped settings.json backups kept (`SETTINGS_BACKUP_RETENTION`). Shared with the
/// extension: both prune the same `settings.json.claudegate-*.bak` ring.
pub const SETTINGS_BACKUP_RETENTION: usize = 5;
/// The extension's POSIX wrapper (`installHookWrapper`), written at mode 0755.
pub const HOOK_SH_CONTENT: &str = "#!/usr/bin/env bash\npython3 \"$HOME/.claudegate/hook.py\"\n";
pub const HOOK_PY_FILE: &str = "hook.py";
pub const HOOK_SH_FILE: &str = "hook.sh";
/// Present when this app (not the extension) set capture up; holds the bundled hook's version.
pub const MARKER_FILE: &str = ".installed-by-desktop";

const STAMP_PREFIX: &str = "# CLAUDEGATE_HOOK_VERSION=";

/// The version the build stamped into the bundled hook.py, e.g. `v1.16.1`.
pub fn bundled_hook_version(hook_py: &str) -> Option<String> {
    hook_py
        .lines()
        .find_map(|l| l.strip_prefix(STAMP_PREFIX))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// `computeSettingsPatch`'s result. `shape_error` is ours: a `hooks` or `hooks.PreToolUse` the
/// extension would throw on (or, for an array `hooks`, silently drop our entry from). The app
/// refuses those rather than guess.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPatch {
    pub content: String,
    pub changed: bool,
    pub parse_error: bool,
    pub shape_error: Option<String>,
}

fn refuse_shape(raw: &str, why: &str) -> SettingsPatch {
    SettingsPatch { content: raw.to_owned(), changed: false, parse_error: false, shape_error: Some(why.to_owned()) }
}

/// Port of `computeSettingsPatch`: the settings text that should be on disk, and whether that is
/// a semantic change. An absent/blank file is a fresh install; text that is not a JSON object
/// sets `parse_error` and echoes `raw`, so the caller never overwrites it. Key order and layout
/// are `JSON.stringify(settings, null, 2)`'s.
pub fn compute_settings_patch(raw: &str, hook_command: &str) -> SettingsPatch {
    let mut settings = if raw.trim().is_empty() {
        OJson::Obj(Vec::new())
    } else {
        match OJson::parse(raw) {
            Ok(v @ OJson::Obj(_)) => v,
            _ => return SettingsPatch { content: raw.to_owned(), changed: false, parse_error: true, shape_error: None },
        }
    };

    if !settings.get("hooks").is_some_and(OJson::truthy) {
        settings.set("hooks", OJson::Obj(Vec::new()));
    }
    let hooks = settings.get_mut("hooks").expect("set above");
    if !matches!(hooks, OJson::Obj(_)) {
        return refuse_shape(raw, "\"hooks\" in settings.json is not an object");
    }
    if !hooks.get("PreToolUse").is_some_and(OJson::truthy) {
        hooks.set("PreToolUse", OJson::Arr(Vec::new()));
    }
    let Some(OJson::Arr(pre)) = hooks.get_mut("PreToolUse") else {
        return refuse_shape(raw, "\"hooks.PreToolUse\" in settings.json is not a list");
    };

    let is_ours = |h: &OJson| h.to_compact().contains("claudegate");
    let correct = pre.iter().filter(|h| is_ours(h)).any(|h| {
        h.get("matcher").and_then(OJson::as_str) == Some(PRE_TOOL_MATCHER)
            && matches!(h.get("hooks"), Some(OJson::Arr(xs)) if xs.iter().any(|x| x.get("command").and_then(OJson::as_str) == Some(hook_command)))
    });
    let any_ours = pre.iter().any(is_ours);

    let mut changed = false;
    if correct {
        // Already registered: content is the reformatted text, but nothing changed.
    } else if any_ours {
        // Stale registration: repair the command and matcher of entries that carry ours.
        for h in pre.iter_mut().filter(|h| is_ours(h)) {
            let Some(OJson::Arr(xs)) = h.get_mut("hooks") else { continue };
            let mut mine = false;
            for x in xs.iter_mut() {
                if x.get("command").and_then(OJson::as_str).is_some_and(|c| c.contains("claudegate")) {
                    x.set("command", OJson::Str(hook_command.to_owned()));
                    mine = true;
                }
            }
            if !mine {
                continue;
            }
            h.set("matcher", OJson::Str(PRE_TOOL_MATCHER.to_owned()));
            changed = true;
        }
    } else {
        pre.push(OJson::Obj(vec![
            ("matcher".into(), OJson::Str(PRE_TOOL_MATCHER.into())),
            (
                "hooks".into(),
                OJson::Arr(vec![OJson::Obj(vec![
                    ("type".into(), OJson::Str("command".into())),
                    ("command".into(), OJson::Str(hook_command.into())),
                ])]),
            ),
        ]));
        changed = true;
    }

    SettingsPatch { content: settings.to_pretty(), changed, parse_error: false, shape_error: None }
}

/// Port of `backupsToPrune`: which of `names` are stale `<base>.claudegate-*.bak` backups, oldest
/// first, keeping the newest `keep`. The legacy `<base>.claudegate.bak` never matches.
pub fn backups_to_prune(names: &[String], base_name: &str, keep: usize) -> Vec<String> {
    let prefix = format!("{base_name}.claudegate-");
    let mut mine: Vec<String> = names.iter().filter(|n| n.starts_with(&prefix) && n.ends_with(".bak")).cloned().collect();
    mine.sort();
    if mine.len() <= keep {
        return Vec::new();
    }
    let excess = mine.len() - keep;
    mine.truncate(excess);
    mine
}

/// Port of `verifySettingsContent`: `None` when `after` is an acceptable result of our patch, or
/// the reason it is not (the caller then restores the backup). Every top-level key of `previous`
/// must survive, and our entry must have landed.
pub fn verify_settings_content(after: &str, previous: &str, hook_command: &str) -> Option<String> {
    let Ok(parsed) = serde_json::from_str::<Value>(after) else {
        return Some("the file on disk is not valid JSON".into());
    };
    let Some(after_obj) = parsed.as_object() else {
        return Some("the file on disk is not a JSON object".into());
    };
    // Keys in JavaScript's order, so the first lost key named is the one the extension names.
    if let Ok(before @ OJson::Obj(_)) = OJson::parse(previous) {
        for key in before.keys() {
            if !after_obj.contains_key(key) {
                return Some(format!("top-level key \"{key}\" was lost"));
            }
        }
    }
    let landed = entry_landed(&parsed, hook_command);
    if !landed {
        return Some("the claudegate hook entry is missing".into());
    }
    None
}

fn entry_landed(settings: &Value, hook_command: &str) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|h| {
                h.get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|xs| xs.iter().any(|x| x.get("command").and_then(Value::as_str) == Some(hook_command)))
            })
        })
}

/// What a successful setup wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallReport {
    /// The file written: the realpath of `~/.claude/settings.json`.
    pub settings: PathBuf,
    /// The pre-install copy, `None` when there was no settings file.
    pub backup: Option<PathBuf>,
    /// Files this call created (hook.py, and hook.sh unless an identical one was there).
    pub created: Vec<PathBuf>,
    /// The version written to the marker.
    pub version: String,
    /// Set when another program rewrote settings.json right after our write but kept our entry:
    /// setup worked, and the file was left as that program wrote it.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstallError {
    ReadOnly,
    PythonMissing,
    /// The bundled hook.py carries no `CLAUDEGATE_HOOK_VERSION` stamp.
    BundledHook,
    /// `~/.claudegate/hook.py` is already there (most likely the extension's).
    HookFilesExist,
    /// `~/.claudegate/hook.py` is there next to the marker: left over from an earlier setup by
    /// this app whose settings entry was removed.
    HookFilesFromApp,
    /// `~/.claudegate/hook.sh` is there with content other than the extension's wrapper.
    ForeignWrapper,
    /// Capture is already set up (or the status is anything but `NotSetUp`).
    AlreadySetUp(CaptureStatus),
    /// settings.json could not be read, or is not a plain JSON object of the expected shape.
    SettingsUnreadable(String),
    /// Not supported on this platform (the extension writes hook.bat on Windows).
    Unsupported,
    /// Something failed mid-install. Everything this call wrote was rolled back unless
    /// `rollback` says otherwise.
    Failed { reason: String, rollback: Option<String> },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::ReadOnly => write!(f, "ClaudeGate Desktop is in read-only mode. Turn it off to set up capture."),
            InstallError::PythonMissing => write!(f, "Python 3 is required but was not found. Install it, then try again."),
            InstallError::BundledHook => write!(f, "The bundled hook has no version stamp; this build is broken."),
            InstallError::HookFilesExist => write!(
                f,
                "ClaudeGate's hook files already exist (likely from the VS Code extension). \
                 Reinstall the extension or remove ~/.claudegate/hook.py first."
            ),
            InstallError::HookFilesFromApp => write!(
                f,
                "ClaudeGate's hook files from an earlier setup in this app already exist. \
                 Remove ~/.claudegate/hook.py, hook.sh and .installed-by-desktop to set up again."
            ),
            InstallError::ForeignWrapper => write!(
                f,
                "~/.claudegate/hook.sh already exists with different content. Remove it, then try again."
            ),
            InstallError::AlreadySetUp(s) => write!(f, "Capture is already set up ({s:?}); nothing was changed."),
            InstallError::SettingsUnreadable(r) => write!(
                f,
                "~/.claude/settings.json can't be updated safely ({r}). Your configuration was left untouched."
            ),
            InstallError::Unsupported => write!(f, "Setting up capture is not supported on this platform yet."),
            InstallError::Failed { reason, rollback: None } => {
                write!(f, "Capture setup failed ({reason}). Everything it changed was put back.")
            }
            InstallError::Failed { reason, rollback: Some(r) } => {
                write!(f, "Capture setup failed ({reason}), and putting things back also failed: {r}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

/// Sets capture up under `home` (the user's HOME): `<home>/.claudegate/{hook.py,hook.sh}`, the
/// marker, and one PreToolUse entry in `<home>/.claude/settings.json`. Refuses unless the status
/// is exactly `NotSetUp`. See `store::install_capture` for the protocol.
pub fn set_up_capture(home: &Path, bundled_hook: &str, python3: bool, read_only: bool) -> Result<InstallReport, InstallError> {
    crate::store::install_capture(home, bundled_hook, python3, read_only)
}
