//! Capture status and setup for the UI, and the startup update of the app's own hook.py.
//!
//! Everything reads `<HOME>/.claudegate` and `<HOME>/.claude/settings.json`, never
//! `CLAUDEGATE_HOME`: the registered wrapper runs `$HOME/.claudegate/hook.py`, so that is the
//! folder detection, setup and the update must agree on. HOME is a parameter everywhere, so tests
//! use a temp folder. The writes themselves are `claudegate_core`'s (`store.rs`).

use crate::commands::{blocking, check_writable_for};
use crate::editor::{fallback_dirs, is_executable};
use claudegate_core::capture::{
    app_owns_hook, bundled_hook_version, capture_status as core_status, hook_version, CaptureStatus, HookReplaced,
    HookUpdate, HookVersion, InstallError, InstallReport, HOOK_PY_FILE, MARKER_FILE,
};
use std::ffi::OsStr;
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
    /// For `vscode`/`app`: python3 was not found from this app. A warning only: the hook runs
    /// under Claude Code's environment.
    pub python_missing: bool,
    /// For `vscode`: this app set capture up, and the extension has since replaced hook.py.
    pub taken_over: bool,
    /// Whether this build can set capture up at all (core refuses with `Unsupported` off unix).
    /// The UI asks this rather than guessing from the platform string.
    pub setup_supported: bool,
    /// For `vscode`/`app`: how the installed hook.py compares with the bundled one.
    pub hook: Option<HookView>,
}

/// `HookVersion` for the UI. `state` is one of `upToDate`, `older`, `newer`, `different`, `missing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookView {
    pub state: String,
    /// The installed hook.py's `CLAUDEGATE_HOOK_VERSION` stamp (line 2); `None` for an unstamped
    /// file (the extension's, version unknown), and for `upToDate`/`missing`.
    pub installed: Option<String>,
    /// The bundled hook's version, e.g. `v1.16.1`.
    pub bundled: String,
}

pub fn hook_view_of(v: &HookVersion) -> HookView {
    let (state, installed) = match v {
        HookVersion::UpToDate => ("upToDate", None),
        HookVersion::Older { installed } => ("older", Some(installed.clone())),
        HookVersion::Newer { installed } => ("newer", Some(installed.clone())),
        HookVersion::Different { installed } => ("different", installed.clone()),
        HookVersion::Missing => ("missing", None),
    };
    HookView { state: state.to_owned(), installed, bundled: bundled_hook_version(BUNDLED_HOOK).unwrap_or_default() }
}

/// A successful "Update hook", with the status read right after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookReplacedView {
    pub hook: String,
    /// The previous hook.py's copy; `None` when there was none.
    pub backup: Option<String>,
    /// What was there before.
    pub from: HookView,
    pub to: String,
    /// The marker was there beside the extension's unstamped hook: now stamped, this app keeps the
    /// hook up to date (its startup update) until the extension replaces it.
    pub app_owns_now: bool,
    pub capture: CaptureView,
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
    /// hook.py was a leftover (the extension's, or an earlier setup's) and was reused.
    pub adopted: bool,
    pub capture: CaptureView,
}

pub fn status_key(s: &CaptureStatus) -> &'static str {
    match s {
        CaptureStatus::SetUpByExtension { .. } => "vscode",
        CaptureStatus::SetUpByApp { .. } => "app",
        CaptureStatus::NotSetUp => "not-set-up",
        CaptureStatus::SettingsUnreadable(_) => "settings-unreadable",
        CaptureStatus::PythonMissing => "python-missing",
    }
}

/// What `read_status` saw beyond the status itself.
struct Read {
    status: CaptureStatus,
    taken_over: bool,
    hook: Option<HookVersion>,
}

fn read_full(home: &Path, python3: bool) -> Read {
    let literal = home.join(".claude").join("settings.json");
    let unreadable = |r: String| Read { status: CaptureStatus::SettingsUnreadable(r), taken_over: false, hook: None };
    let text = match fs::read(&literal) {
        Ok(b) => match String::from_utf8(b) {
            Ok(t) => Some(t),
            Err(_) => return unreadable("not UTF-8 text".into()),
        },
        // A link to nothing is not "no settings": setup would refuse it, so say why now.
        Err(e) if e.kind() == io::ErrorKind::NotFound && fs::symlink_metadata(&literal).is_ok() => {
            return unreadable(format!("{} is a link to nothing", literal.display()))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return unreadable(format!("could not read it: {e}")),
    };
    let cg = home.join(".claudegate");
    let marker_path = cg.join(MARKER_FILE);
    let marker_there = fs::symlink_metadata(&marker_path).is_ok();
    // The same ownership test the startup update uses: the marker, and our stamp in hook.py.
    let hook_py = fs::read(cg.join(HOOK_PY_FILE)).ok();
    let ours = app_owns_hook(marker_there, hook_py.as_deref());
    let status = match core_status(text.as_deref(), ours, python3) {
        CaptureStatus::SetUpByApp { python_missing, .. } => CaptureStatus::SetUpByApp {
            version: fs::read(&marker_path)
                .ok()
                .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
                .filter(|v| !v.is_empty()),
            python_missing,
        },
        other => other,
    };
    let taken_over = marker_there && matches!(status, CaptureStatus::SetUpByExtension { .. });
    // Only a registered hook has a version worth reporting ("missing" means registered, no file).
    let hook = matches!(status, CaptureStatus::SetUpByApp { .. } | CaptureStatus::SetUpByExtension { .. })
        .then(|| hook_version(hook_py.as_deref(), BUNDLED_HOOK));
    Read { status, taken_over, hook }
}

/// The capture status under `home`, with the marker's version for `SetUpByApp`. Read only.
pub fn read_status(home: &Path, python3: bool) -> CaptureStatus {
    read_full(home, python3).status
}

pub fn view_of(s: &CaptureStatus) -> CaptureView {
    let (version, note, python_missing) = match s {
        CaptureStatus::SetUpByApp { version, python_missing } => (version.clone(), None, *python_missing),
        CaptureStatus::SetUpByExtension { python_missing } => (None, None, *python_missing),
        CaptureStatus::SettingsUnreadable(r) => (None, Some(r.clone()), false),
        _ => (None, None, false),
    };
    CaptureView {
        status: status_key(s).to_owned(),
        version,
        note,
        python_missing,
        taken_over: false,
        setup_supported: cfg!(unix),
        hook: None,
    }
}

pub fn capture_view_for(home: &Path, python3: bool) -> CaptureView {
    let r = read_full(home, python3);
    CaptureView { taken_over: r.taken_over, hook: r.hook.as_ref().map(hook_view_of), ..view_of(&r.status) }
}

/// Whether `<program> --version` runs and exits 0 within `timeout`. No shell: `program` is
/// exec'd as is, found on PATH. A program still running at the deadline is killed.
pub fn python3_available_with(program: &str, timeout: Duration) -> bool {
    python3_available_with_args(program, &["--version"], timeout)
}

/// `program args…` exits 0 within `timeout`; no shell, null stdio, killed at the deadline.
fn python3_available_with_args(program: &str, args: &[&str], timeout: Duration) -> bool {
    let child = Command::new(program)
        .args(args)
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

/// Where python3 is, searched the way the editor launcher searches (`editor::resolve_program`):
/// each PATH entry, then `editor::fallback_dirs` (a GUI app's PATH is often bare). On macOS
/// `/usr/bin/python3` is a stub that opens the Command Line Tools installer when they are
/// missing, so it counts only when `clt_installed()` (`xcode-select -p`) says they are there; that
/// check runs at most once per search, and never off macOS.
pub fn pick_python3(
    path_var: Option<&OsStr>,
    home: Option<&Path>,
    is_exec: &dyn Fn(&Path) -> bool,
    macos: bool,
    clt_installed: &dyn Fn() -> bool,
) -> Option<PathBuf> {
    let stub = Path::new("/usr/bin/python3");
    let clt = std::cell::OnceCell::new();
    let mut dirs: Vec<PathBuf> = path_var.map(|v| std::env::split_paths(v).collect()).unwrap_or_default();
    dirs.extend(fallback_dirs(home));
    dirs.into_iter().map(|d| d.join("python3")).find(|c| {
        is_exec(c) && !(macos && c.as_path() == stub && !*clt.get_or_init(clt_installed))
    })
}

const PYTHON_TIMEOUT: Duration = Duration::from_secs(3);
static PYTHON3: Mutex<Option<bool>> = Mutex::new(None);

/// python3 found (`pick_python3`) and its `--version` run by absolute path, checked once per
/// session; `recheck` ("Check again", and every setup) runs it again.
pub fn python3_cached(recheck: bool) -> bool {
    let mut g = PYTHON3.lock().unwrap_or_else(|p| p.into_inner());
    match *g {
        Some(found) if !recheck => found,
        _ => {
            let home = dirs::home_dir();
            let clt = || python3_available_with_args("/usr/bin/xcode-select", &["-p"], PYTHON_TIMEOUT);
            let path_var = std::env::var_os("PATH");
            let found = pick_python3(path_var.as_deref(), home.as_deref(), &is_executable, cfg!(target_os = "macos"), &clt)
                .is_some_and(|p| python3_available_with(&p.to_string_lossy(), PYTHON_TIMEOUT));
            *g = Some(found);
            found
        }
    }
}

pub fn install_error_text(e: &InstallError) -> String {
    match e {
        InstallError::AlreadySetUp(CaptureStatus::SetUpByExtension { .. }) => {
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
        adopted: r.adopted,
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

pub fn hook_replaced_view(r: &HookReplaced, capture: CaptureView) -> HookReplacedView {
    HookReplacedView {
        hook: show(&r.hook),
        backup: r.backup.as_deref().map(show),
        from: hook_view_of(&r.from),
        to: r.to.clone(),
        app_owns_now: r.app_owns_now,
        capture,
    }
}

/// "Update hook" minus Tauri: replaces `<home>/.claudegate/hook.py` with the bundled hook
/// (whoever installed it), backed up under the config file's folder (`backups/hook/`). Read-only
/// mode (or an unreadable config) blocks it; the core checks the rest. Never writes hook.sh, the
/// marker or settings.json. The status is read again right after a success.
pub fn update_capture_hook_with(home: &Path, config_path: &Path, python3: bool) -> Result<HookReplacedView, String> {
    check_writable_for(config_path, "update the hook")?;
    let desktop_home = crate::commands::desktop_home_for(config_path, &home.join(".claudegate"))?;
    let r = claudegate_core::capture::replace_hook(home, &desktop_home, BUNDLED_HOOK, false).map_err(|e| e.to_string())?;
    Ok(hook_replaced_view(&r, capture_view_for(home, python3)))
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

/// "Update hook" (after the UI's confirmation).
#[tauri::command]
pub async fn update_capture_hook() -> Result<HookReplacedView, String> {
    blocking(move || update_capture_hook_with(&home()?, &crate::config::config_path(), python3_cached(false))).await?
}

/// Sets capture up (after the UI's confirmation).
#[tauri::command]
pub async fn set_up_capture() -> Result<InstallView, String> {
    blocking(move || set_up_capture_with(&home()?, &crate::config::config_path(), python3_cached(true))).await?
}
