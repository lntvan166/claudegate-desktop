//! Capture setup: whether Claude Code is wired to the ClaudeGate capture hook, and the pure half
//! of setting it up. Everything here is pure (callers pass in what they read) except
//! `set_up_capture`, which hands the writes to `store::install_capture`.
//!
//! `compute_settings_patch`, `backups_to_prune` and `verify_settings_content` are ports of the
//! extension's `hookInstaller.ts` (v1.18.0, with the capture-audit registration), checked against
//! its own TypeScript by `fixtures/capture/*.json` and `xcheck/run.ts`.

use crate::ojson::OJson;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    /// `python_missing`: a warning only. The hook runs under Claude Code's environment, where
    /// python3 may well be found even though this app could not find it.
    SetUpByExtension { python_missing: bool },
    /// `version` is None when the caller has not read the marker's content.
    SetUpByApp { version: Option<String>, python_missing: bool },
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

/// Precedence: SettingsUnreadable; registered+marker (`SetUpByApp`); registered
/// (`SetUpByExtension`); then, with no hook registered, PythonMissing; NotSetUp. A registered
/// hook outranks a missing python3, which only sets the `python_missing` warning there.
/// `marker` means "this app owns the setup": callers pass `app_owns_hook`, not the marker's mere
/// presence. `settings_text: None` is a missing file; an empty/whitespace file counts as missing
/// too (the extension's installer treats it as a fresh install; same choice here, for parity). Strict JSON only.
pub fn capture_status(settings_text: Option<&str>, marker: bool, python3: bool) -> CaptureStatus {
    let not_set_up = if python3 { CaptureStatus::NotSetUp } else { CaptureStatus::PythonMissing };
    let Some(text) = settings_text else { return not_set_up };
    if text.trim().is_empty() {
        return not_set_up;
    }
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return CaptureStatus::SettingsUnreadable(format!("not valid JSON: {e}")),
    };
    if !parsed.is_object() {
        return CaptureStatus::SettingsUnreadable("not a JSON object".to_string());
    }
    let python_missing = !python3;
    match (registered(&parsed), marker) {
        (true, true) => CaptureStatus::SetUpByApp { version: None, python_missing },
        (true, false) => CaptureStatus::SetUpByExtension { python_missing },
        _ => not_set_up,
    }
}

/// Whether this app owns the hook setup: the marker exists, and the installed hook.py (when there
/// is one) carries the `CLAUDEGATE_HOOK_VERSION` stamp this app's copy has. The extension's
/// hook.py has none, so a marker next to an unstamped hook.py means the extension has since
/// replaced the hook. The same test gates the startup update.
pub fn app_owns_hook(marker: bool, hook_py: Option<&[u8]>) -> bool {
    marker && hook_py.is_none_or(|b| bundled_hook_version(&String::from_utf8_lossy(b)).is_some())
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

/// The version the build stamped into a hook.py, e.g. `v1.16.1`. Only line 2 (after the shebang)
/// counts, the line `adoptable_hook` removes: the same text anywhere else (say, quoted in a
/// docstring of the extension's hook) is not a stamp.
pub fn bundled_hook_version(hook_py: &str) -> Option<String> {
    hook_py
        .lines()
        .nth(1)
        .and_then(|l| l.strip_prefix(STAMP_PREFIX))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// Whether an existing hook.py is a leftover setup may replace with `bundled_hook`: byte for
/// byte either the bundled hook itself (an earlier setup by this app), or the bundled hook with
/// its stamp line (line 2, `# CLAUDEGATE_HOOK_VERSION=…\n`) removed, which is the extension's hook.py at
/// the tag the bundled one was cut from. Anything else is not ours to replace.
pub fn adoptable_hook(existing: &[u8], bundled_hook: &str) -> bool {
    if existing == bundled_hook.as_bytes() {
        return true;
    }
    // The stamp is line 2 (after the shebang); a stamp anywhere else is not that line.
    let mut lines: Vec<&str> = bundled_hook.split_inclusive('\n').collect();
    if !lines.get(1).is_some_and(|l| l.starts_with(STAMP_PREFIX) && l.ends_with('\n')) {
        return false;
    }
    lines.remove(1);
    existing == lines.concat().as_bytes()
}

/// `computeSettingsPatch`'s result. `shape_error` says why: a `hooks` that is not a plain object,
/// or an event we would touch that is not a list. The extension refuses those too
/// (`parseError` + `shapeError`), echoing the original bytes, and so does the app.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPatch {
    pub content: String,
    pub changed: bool,
    pub parse_error: bool,
    pub shape_error: Option<String>,
}

fn refuse_shape(raw: &str, why: String) -> SettingsPatch {
    SettingsPatch { content: raw.to_owned(), changed: false, parse_error: true, shape_error: Some(why) }
}

/// The matcher the capture-audit hook (`audit.py`) is registered under (`POST_TOOL_MATCHER`).
pub const POST_TOOL_MATCHER: &str = "^Bash$";
/// The events the audit hook is registered under, in the order they are appended (`AUDIT_EVENTS`).
pub const AUDIT_EVENTS: [&str; 2] = ["PostToolUse", "PostToolUseFailure"];
/// The extension's POSIX audit wrapper (`installAuditWrapper`), written at mode 0755. It exits 0
/// when audit.py is absent: a PostToolUse hook that exits 2 feeds its stderr to Claude.
pub const AUDIT_SH_CONTENT: &str = "#!/usr/bin/env bash\n[ -f \"$HOME/.claudegate/audit.py\" ] || exit 0\npython3 \"$HOME/.claudegate/audit.py\"\n";
pub const AUDIT_PY_FILE: &str = "audit.py";
pub const AUDIT_SH_FILE: &str = "audit.sh";

/// `AUDIT_COMMAND_SHAPE`, `/[\\/]\.claudegate[\\/]audit\.(sh|bat)$/`: whether an audit-event hook
/// command is ours. Stricter than PreToolUse's substring rule, so a foreign
/// `/x/claudegate/lint.sh` PostToolUse hook is never rewritten.
pub fn audit_command_shape(command: &str) -> bool {
    let is_sep = |c: Option<char>| matches!(c, Some('/') | Some('\\'));
    let Some(rest) = command.strip_suffix("audit.sh").or_else(|| command.strip_suffix("audit.bat")) else {
        return false;
    };
    let mut it = rest.chars();
    if !is_sep(it.next_back()) {
        return false;
    }
    let Some(rest) = it.as_str().strip_suffix(".claudegate") else { return false };
    is_sep(rest.chars().next_back())
}

fn single_entry(matcher: &str, command: &str) -> OJson {
    OJson::Obj(vec![
        ("matcher".into(), OJson::Str(matcher.into())),
        (
            "hooks".into(),
            OJson::Arr(vec![OJson::Obj(vec![
                ("type".into(), OJson::Str("command".into())),
                ("command".into(), OJson::Str(command.into())),
            ])]),
        ),
    ])
}

/// Port of `ensureHookEntry`: make `hooks[event]` carry a correct entry of ours
/// (`{matcher, hooks: [{type: "command", command}]}`). `group_owns` decides which groups are ours,
/// `command_owns` which commands in them get repaired. True when that is a semantic change.
/// The caller has already refused an `event` that is neither absent, null nor a list.
fn ensure_hook_entry(
    hooks: &mut OJson,
    event: &str,
    matcher: &str,
    command: &str,
    group_owns: &dyn Fn(&OJson) -> bool,
    command_owns: &dyn Fn(&str) -> bool,
) -> bool {
    if !hooks.get(event).is_some_and(OJson::truthy) {
        hooks.set(event, OJson::Arr(Vec::new()));
    }
    let Some(OJson::Arr(groups)) = hooks.get_mut(event) else { return false };

    let correct = groups.iter().filter(|h| group_owns(h)).any(|h| {
        h.get("matcher").and_then(OJson::as_str) == Some(matcher)
            && matches!(h.get("hooks"), Some(OJson::Arr(xs)) if xs.iter().any(|x| x.get("command").and_then(OJson::as_str) == Some(command)))
    });
    if correct {
        // Already registered: content is the reformatted text, but nothing changed.
        return false;
    }
    let mut changed = false;
    // Stale registration: repair the command and matcher of entries that carry ours.
    for h in groups.iter_mut().filter(|h| group_owns(h)) {
        let Some(OJson::Arr(xs)) = h.get_mut("hooks") else { continue };
        let mut mine = false;
        for x in xs.iter_mut() {
            if x.get("command").and_then(OJson::as_str).is_some_and(command_owns) {
                x.set("command", OJson::Str(command.to_owned()));
                mine = true;
            }
        }
        if !mine {
            continue;
        }
        h.set("matcher", OJson::Str(matcher.to_owned()));
        changed = true;
    }
    if changed {
        return true;
    }
    // Nothing repaired (no group of ours, or ours mention claudegate only outside a command): our
    // own entry goes beside them, or verification would fail on every attempt.
    groups.push(single_entry(matcher, command));
    true
}

/// Port of `computeSettingsPatch`: the settings text that should be on disk, and whether that is
/// a semantic change. An absent/blank file is a fresh install; text that is not a JSON object
/// sets `parse_error` and echoes `raw`, so the caller never overwrites it. Key order and layout
/// are `JSON.stringify(settings, null, 2)`'s. With `audit_command`, the audit hook is registered
/// under each of `AUDIT_EVENTS` too, after PreToolUse.
pub fn compute_settings_patch(raw: &str, hook_command: &str, audit_command: Option<&str>) -> SettingsPatch {
    let mut settings = if raw.trim().is_empty() {
        OJson::Obj(Vec::new())
    } else {
        match OJson::parse(raw) {
            Ok(v @ OJson::Obj(_)) => v,
            _ => return SettingsPatch { content: raw.to_owned(), changed: false, parse_error: true, shape_error: None },
        }
    };

    // A shape we cannot patch without discarding the user's data is refused; `null` holds no data
    // and counts as absent.
    let present = |v: Option<&OJson>| v.is_some_and(|v| !matches!(v, OJson::Null));
    if present(settings.get("hooks")) && !matches!(settings.get("hooks"), Some(OJson::Obj(_))) {
        return refuse_shape(raw, "\"hooks\" in settings.json is not an object".into());
    }
    let mut touched = vec!["PreToolUse"];
    if audit_command.is_some() {
        touched.extend(AUDIT_EVENTS);
    }
    if let Some(existing) = settings.get("hooks") {
        for event in &touched {
            if present(existing.get(event)) && !matches!(existing.get(event), Some(OJson::Arr(_))) {
                return refuse_shape(raw, format!("\"hooks.{event}\" in settings.json is not a list"));
            }
        }
    }

    if !settings.get("hooks").is_some_and(OJson::truthy) {
        settings.set("hooks", OJson::Obj(Vec::new()));
    }
    let hooks = settings.get_mut("hooks").expect("set above");

    // PreToolUse: any group whose JSON mentions claudegate, as the extension (and `registered`).
    let mut changed = ensure_hook_entry(
        hooks,
        "PreToolUse",
        PRE_TOOL_MATCHER,
        hook_command,
        &|h| h.to_compact().contains("claudegate"),
        &|c| c.contains("claudegate"),
    );
    if let Some(audit) = audit_command {
        let group_owns = |h: &OJson| {
            matches!(h.get("hooks"), Some(OJson::Arr(xs)) if xs.iter().any(|x| x.get("command").and_then(OJson::as_str).is_some_and(audit_command_shape)))
        };
        for event in AUDIT_EVENTS {
            if ensure_hook_entry(hooks, event, POST_TOOL_MATCHER, audit, &group_owns, &audit_command_shape) {
                changed = true;
            }
        }
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
/// must survive, and our entries must have landed: PreToolUse, and with `audit_command` the audit
/// hook under each of `AUDIT_EVENTS`.
pub fn verify_settings_content(after: &str, previous: &str, hook_command: &str, audit_command: Option<&str>) -> Option<String> {
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
    if !entry_landed(&parsed, "PreToolUse", hook_command) {
        return Some("the claudegate hook entry is missing".into());
    }
    if let Some(audit) = audit_command {
        for event in AUDIT_EVENTS {
            if !entry_landed(&parsed, event, audit) {
                return Some(format!("the claudegate {event} audit entry is missing"));
            }
        }
    }
    None
}

fn entry_landed(settings: &Value, event: &str, command: &str) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|h| {
                h.get("hooks")
                    .and_then(Value::as_array)
                    .is_some_and(|xs| xs.iter().any(|x| x.get("command").and_then(Value::as_str) == Some(command)))
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
    /// hook.py was already there as a leftover setup adopts (`adoptable_hook`) and was replaced
    /// with the bundled hook, rather than created.
    pub adopted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstallError {
    ReadOnly,
    PythonMissing,
    /// The bundled hook.py carries no `CLAUDEGATE_HOOK_VERSION` stamp.
    BundledHook,
    /// `~/.claudegate/hook.py` (or an audit.py other than the bundled one) is already there (most
    /// likely the extension's).
    HookFilesExist,
    /// `~/.claudegate/hook.py` (or audit.py) is there next to the marker: left over from an earlier setup by
    /// this app whose settings entry was removed.
    HookFilesFromApp,
    /// `~/.claudegate/hook.sh` or `audit.sh` is there with content other than the extension's wrapper.
    ForeignWrapper,
    /// `~/.claudegate/.installed-by-desktop` is there but is not a regular file (a symlink, say).
    MarkerNotFile,
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
                 Run \"Claude Gate: Setup Hook\" in VS Code, or remove ~/.claudegate/hook.py and audit.py if the extension is gone."
            ),
            InstallError::HookFilesFromApp => write!(
                f,
                "ClaudeGate's hook files from an earlier setup in this app already exist. \
                 Remove ~/.claudegate/hook.py, hook.sh, audit.py, audit.sh and .installed-by-desktop to set up again."
            ),
            InstallError::ForeignWrapper => write!(
                f,
                "~/.claudegate/hook.sh or audit.sh already exists with different content. Remove it, then try again."
            ),
            InstallError::MarkerNotFile => write!(
                f,
                "~/.claudegate/.installed-by-desktop is not a regular file. Remove it, then try again."
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

/// Sets capture up under `home` (the user's HOME): `<home>/.claudegate/{hook.py,hook.sh,audit.py,
/// audit.sh}`, the marker, one PreToolUse entry in `<home>/.claude/settings.json`, and the audit
/// entries under `AUDIT_EVENTS`. Refuses unless the status is exactly `NotSetUp`. See
/// `store::install_capture` for the protocol.
pub fn set_up_capture(
    home: &Path,
    bundled_hook: &str,
    bundled_audit: &str,
    python3: bool,
    read_only: bool,
) -> Result<InstallReport, InstallError> {
    crate::store::install_capture(home, bundled_hook, bundled_audit, python3, read_only)
}

/// True when `candidate` is a strictly newer `vMAJOR.MINOR.PATCH` than `installed`. Anything that
/// does not parse as one (either side) is never newer, so an odd marker never triggers a write.
pub fn version_newer(candidate: &str, installed: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let v = v.trim();
        let v = v.strip_prefix('v').unwrap_or(v);
        let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
        let t = (it.next()??, it.next()??, it.next()??);
        it.next().is_none().then_some(t)
    }
    matches!((parse(candidate), parse(installed)), (Some(a), Some(b)) if a > b)
}

/// What the startup update of a stamped hook.py did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookUpdate {
    /// hook.py was rewritten with the bundled copy (and the marker, when there is one, says `to`).
    Updated { from: String, to: String },
    /// Nothing was written; why.
    Skipped(String),
}

/// Rewrites `<home>/.claudegate/hook.py` with the bundled copy when a hook is registered (by
/// either side), the installed hook.py carries our stamp on line 2, and the bundled version is
/// newer than that stamp. No marker needed; an unstamped (extension) hook is never touched.
/// audit.py is brought to `bundled_audit` alongside. Never touches settings.json, hook.sh or
/// audit.sh, and never creates the marker. See `store::update_app_hook`.
pub fn update_app_hook(
    home: &Path,
    bundled_hook: &str,
    bundled_audit: &str,
    python3: bool,
    read_only: bool,
) -> Result<HookUpdate, String> {
    crate::store::update_app_hook(home, bundled_hook, bundled_audit, python3, read_only)
}

/// How the installed hook.py compares with the bundled one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookVersion {
    /// The bundled hook byte for byte, or the bundled hook minus its stamp line (`adoptable_hook`).
    UpToDate,
    /// Stamped with a version older than the bundled one.
    Older { installed: String },
    /// Stamped with a version newer than the bundled one: replacing it would be a downgrade, so
    /// Update hook refuses it.
    Newer { installed: String },
    /// Anything else: stamped with the bundled version (or an unparseable one) but different bytes,
    /// i.e. a modified copy, or unstamped (`installed: None`, the extension's copy, whose version
    /// is not guessed and may be newer).
    Different { installed: Option<String> },
    /// There is no hook.py.
    Missing,
}

/// Classifies the installed hook.py (`None`: there is none) against `bundled_hook`. Pure.
pub fn hook_version(installed: Option<&[u8]>, bundled_hook: &str) -> HookVersion {
    let Some(bytes) = installed else { return HookVersion::Missing };
    if adoptable_hook(bytes, bundled_hook) {
        return HookVersion::UpToDate;
    }
    let stamp = bundled_hook_version(&String::from_utf8_lossy(bytes));
    match (stamp, bundled_hook_version(bundled_hook)) {
        (Some(have), Some(ship)) if version_newer(&ship, &have) => HookVersion::Older { installed: have },
        (Some(have), Some(ship)) if version_newer(&have, &ship) => HookVersion::Newer { installed: have },
        (stamp, _) => HookVersion::Different { installed: stamp },
    }
}

/// What "Update hook" wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct HookReplaced {
    /// `<home>/.claudegate/hook.py`.
    pub hook: PathBuf,
    /// The copy of the previous hook.py; `None` when there was none (it was created).
    pub backup: Option<PathBuf>,
    /// What was there before.
    pub from: HookVersion,
    /// The bundled version now installed.
    pub to: String,
    /// The marker is there and the replaced hook.py was unstamped (the extension had taken the
    /// hook over): now stamped again, the app counts it as its own and its startup update keeps it
    /// current, until the extension replaces it again. The marker itself is not written.
    pub app_owns_now: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HookReplaceError {
    ReadOnly,
    Unsupported,
    /// The bundled hook.py carries no `CLAUDEGATE_HOOK_VERSION` stamp.
    BundledHook,
    /// No ClaudeGate entry is registered in settings.json (or it could not be read): the status.
    NotRegistered(CaptureStatus),
    /// `~/.claudegate/hook.py` is a symlink, a folder, or anything but a regular file.
    NotRegularFile,
    /// It already is the bundled hook; nothing was written.
    UpToDate,
    /// It is stamped newer than the bundled hook; replacing it would downgrade it.
    Newer(String),
    /// hook.py changed between our read and our write; nothing was written.
    Changed,
    Failed(String),
}

impl std::fmt::Display for HookReplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookReplaceError::ReadOnly => write!(f, "ClaudeGate Desktop is in read-only mode. Turn it off to update the hook."),
            HookReplaceError::Unsupported => write!(f, "Updating the hook is not supported on this platform yet."),
            HookReplaceError::BundledHook => write!(f, "The bundled hook has no version stamp; this build is broken."),
            HookReplaceError::NotRegistered(_) => {
                write!(f, "Capture is not set up (no ClaudeGate hook in ~/.claude/settings.json), so hook.py was left alone.")
            }
            HookReplaceError::NotRegularFile => {
                write!(f, "~/.claudegate/hook.py is not a regular file (a symlink, say), so it was left alone.")
            }
            HookReplaceError::UpToDate => write!(f, "hook.py is already the bundled version; nothing was changed."),
            HookReplaceError::Newer(v) => {
                write!(f, "hook.py is {v}, newer than the version bundled in this app, so it was left as is.")
            }
            HookReplaceError::Changed => {
                write!(f, "hook.py changed while it was being updated (VS Code, most likely), so it was left as is. Check again.")
            }
            HookReplaceError::Failed(r) => write!(f, "Couldn't update hook.py ({r}). It was left as it was."),
        }
    }
}

impl std::error::Error for HookReplaceError {}

/// Hook backups kept in `<desktop_home>/backups/hook/`.
pub const HOOK_BACKUPS_KEPT: usize = 10;

/// "Update hook": replaces `<home>/.claudegate/hook.py` with the bundled hook, whoever installed
/// it, after backing it up under `<desktop_home>/backups/hook/`, and brings audit.py to
/// `bundled_audit` alongside. Never writes hook.sh, audit.sh, the marker or settings.json. See
/// `store::replace_hook`.
pub fn replace_hook(
    home: &Path,
    desktop_home: &Path,
    bundled_hook: &str,
    bundled_audit: &str,
    read_only: bool,
) -> Result<HookReplaced, HookReplaceError> {
    crate::store::replace_hook(home, desktop_home, bundled_hook, bundled_audit, read_only)
}
