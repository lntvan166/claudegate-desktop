//! Capture status and setup for the UI, and the startup update of the app's own hook.py.
//!
//! Everything reads `<HOME>/.claudegate` and `<HOME>/.claude/settings.json`, never
//! `CLAUDEGATE_HOME`: the registered wrapper runs `$HOME/.claudegate/hook.py`, so that is the
//! folder detection, setup and the update must agree on. HOME is a parameter everywhere, so tests
//! use a temp folder. The writes themselves are `claudegate_core`'s (`store.rs`).

use crate::commands::{blocking, check_writable_for};
use claudegate_core::capture::{capture_status as core_status, CaptureStatus, HookUpdate, InstallError, InstallReport, MARKER_FILE};
use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The hook this build ships, stamped `# CLAUDEGATE_HOOK_VERSION=…` by the repo.
pub const BUNDLED_HOOK: &str = include_str!("../resources/hook.py");

/// `capture_status` for the UI. `status` is one of `vscode`, `app`, `not-set-up`,
/// `settings-unreadable`, `python-missing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureView {
    pub status: String,
    /// The marker's version, for `app`.
    pub version: Option<String>,
    /// Why, for `settings-unreadable`.
    pub note: Option<String>,
}

/// A successful `set_up_capture`, with the status read right after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallView {
    pub settings: String,
    pub backup: Option<String>,
    pub created: Vec<String>,
    pub version: String,
    /// Setup worked, but something needs the user's attention: show it as a warning.
    pub note: Option<String>,
    pub capture: CaptureView,
}

pub fn status_key(s: &CaptureStatus) -> &'static str {
    match s {
        CaptureStatus::SetUpByExtension => "vscode",
        CaptureStatus::SetUpByApp { .. } => "app",
        CaptureStatus::NotSetUp => "not-set-up",
        CaptureStatus::SettingsUnreadable(_) => "settings-unreadable",
        CaptureStatus::PythonMissing => "python-missing",
    }
}

/// The capture status under `home`, with the marker's version for `SetUpByApp`. Read only.
pub fn read_status(home: &Path, python3: bool) -> CaptureStatus {
    let literal = home.join(".claude").join("settings.json");
    let text = match fs::read(&literal) {
        Ok(b) => match String::from_utf8(b) {
            Ok(t) => Some(t),
            Err(_) => return CaptureStatus::SettingsUnreadable("not UTF-8 text".into()),
        },
        // A link to nothing is not "no settings": setup would refuse it, so say why now.
        Err(e) if e.kind() == io::ErrorKind::NotFound && fs::symlink_metadata(&literal).is_ok() => {
            return CaptureStatus::SettingsUnreadable(format!("{} is a link to nothing", literal.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return CaptureStatus::SettingsUnreadable(format!("could not read it: {e}")),
    };
    let marker_path = home.join(".claudegate").join(MARKER_FILE);
    let marker = fs::read(&marker_path).ok();
    match core_status(text.as_deref(), fs::symlink_metadata(&marker_path).is_ok(), python3) {
        CaptureStatus::SetUpByApp { .. } => CaptureStatus::SetUpByApp {
            version: marker.map(|b| String::from_utf8_lossy(&b).trim().to_owned()).filter(|v| !v.is_empty()),
        },
        other => other,
    }
}

pub fn view_of(s: &CaptureStatus) -> CaptureView {
    let (version, note) = match s {
        CaptureStatus::SetUpByApp { version } => (version.clone(), None),
        CaptureStatus::SettingsUnreadable(r) => (None, Some(r.clone())),
        _ => (None, None),
    };
    CaptureView { status: status_key(s).to_owned(), version, note }
}

pub fn capture_view_for(home: &Path, python3: bool) -> CaptureView {
    view_of(&read_status(home, python3))
}

/// Whether `<program> --version` runs and exits 0 within `timeout`. No shell: `program` is
/// exec'd as is, found on PATH. A program still running at the deadline is killed.
pub fn python3_available_with(program: &str, timeout: Duration) -> bool {
    let child = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(15)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

const PYTHON_TIMEOUT: Duration = Duration::from_secs(3);
static PYTHON3: Mutex<Option<bool>> = Mutex::new(None);

/// `python3 --version`, checked once per session; `recheck` ("Check again") runs it again.
pub fn python3_cached(recheck: bool) -> bool {
    let mut g = PYTHON3.lock().unwrap_or_else(|p| p.into_inner());
    match *g {
        Some(found) if !recheck => found,
        _ => {
            let found = python3_available_with("python3", PYTHON_TIMEOUT);
            *g = Some(found);
            found
        }
    }
}

pub fn install_error_text(e: &InstallError) -> String {
    match e {
        InstallError::AlreadySetUp(CaptureStatus::SetUpByExtension) => {
            "Capture is already set up by VS Code; nothing was changed.".into()
        }
        InstallError::AlreadySetUp(CaptureStatus::SetUpByApp { .. }) => {
            "Capture is already set up by this app; nothing was changed.".into()
        }
        InstallError::AlreadySetUp(_) => "Capture is already set up; nothing was changed.".into(),
        other => other.to_string(),
    }
}

fn show(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn install_view(r: &InstallReport, capture: CaptureView) -> InstallView {
    InstallView {
        settings: show(&r.settings),
        backup: r.backup.as_deref().map(show),
        created: r.created.iter().map(|p| show(p)).collect(),
        version: r.version.clone(),
        note: r.note.clone(),
        capture,
    }
}

/// `set_up_capture` minus Tauri. Read-only mode (or an unreadable config) blocks it; the core
/// refuses anything but `NotSetUp`. The status is read again right after a success.
pub fn set_up_capture_with(home: &Path, config_path: &Path, python3: bool) -> Result<InstallView, String> {
    check_writable_for(config_path, "set up capture")?;
    let report = claudegate_core::capture::set_up_capture(home, BUNDLED_HOOK, python3, false)
        .map_err(|e| install_error_text(&e))?;
    Ok(install_view(&report, capture_view_for(home, python3)))
}

/// The startup update of the app's own hook.py. Read-only mode (or an unreadable config) skips
/// it; `claudegate_core::capture::update_app_hook` checks the rest.
pub fn update_hook_with(home: &Path, config_path: &Path, python3: bool) -> Result<HookUpdate, String> {
    let read_only = !matches!(crate::config::read_only_strict(config_path), Ok(false));
    claudegate_core::capture::update_app_hook(home, BUNDLED_HOOK, python3, read_only)
}

/// Run once at startup, off the main thread. Logs, never raises.
pub fn update_hook_at_startup() {
    let Some(home) = dirs::home_dir() else { return };
    match update_hook_with(&home, &crate::config::config_path(), python3_cached(false)) {
        Ok(HookUpdate::Updated { from, to }) => eprintln!("claudegate: updated ~/.claudegate/hook.py {from} -> {to}"),
        Ok(HookUpdate::Skipped(_)) => {}
        Err(e) => eprintln!("claudegate: couldn't update ~/.claudegate/hook.py: {e}"),
    }
}

fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "Couldn't find your home folder.".to_owned())
}

/// The capture status. `recheck` runs the python3 check again ("Check again").
#[tauri::command]
pub async fn capture_status(recheck: Option<bool>) -> Result<CaptureView, String> {
    blocking(move || Ok(capture_view_for(&home()?, python3_cached(recheck.unwrap_or(false))))).await?
}

/// Sets capture up (after the UI's confirmation).
#[tauri::command]
pub async fn set_up_capture() -> Result<InstallView, String> {
    blocking(move || set_up_capture_with(&home()?, &crate::config::config_path(), python3_cached(true))).await?
}
