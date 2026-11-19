//! `claudegate-desktop [PATH]`: the path a launch (first or second) asks the window to open.

use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const LAUNCH_EVENT: &str = "claudegate://launch";

/// What the UI is told: the absolute path, and whether it is a directory (the UI cannot stat).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTarget {
    pub path: String,
    pub is_dir: bool,
}

/// The first launch's path, held until the UI asks: the event can fire before it listens.
#[derive(Default)]
pub struct LaunchState(Mutex<Option<LaunchTarget>>);

/// Lexical clean-up (`.`, `..`, trailing slash); nothing is read from disk.
fn clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The path in `args` (argv without the program name): the first one that is not a flag, made
/// absolute against `cwd`. `None` when there is none.
pub fn target_from_args(args: &[String], cwd: &Path) -> Option<LaunchTarget> {
    let arg = args.iter().skip(1).find(|a| !a.is_empty() && !a.starts_with('-'))?;
    let p = Path::new(arg);
    let abs = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    let abs = clean(&abs);
    Some(LaunchTarget { is_dir: abs.is_dir(), path: abs.to_string_lossy().into_owned() })
}

/// Remembers the first launch's path.
pub fn remember(app: &AppHandle, args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let target = target_from_args(args, &cwd);
    *app.state::<LaunchState>().0.lock().unwrap() = target.clone();
    if let Some(t) = target {
        let _ = app.emit(LAUNCH_EVENT, t);
    }
}

/// A second launch: focus the window and hand it the path.
pub fn second_instance(app: &AppHandle, args: &[String], cwd: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(t) = target_from_args(args, Path::new(cwd)) {
        let _ = app.emit(LAUNCH_EVENT, t);
    }
}

/// The path the app was launched with, once; the UI asks on mount.
#[tauri::command]
pub fn launch_path(state: tauri::State<LaunchState>) -> Option<LaunchTarget> {
    state.0.lock().unwrap().take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_argument_is_no_target() {
        assert_eq!(target_from_args(&args(&["claudegate-desktop"]), Path::new("/w")), None);
    }

    #[test]
    fn a_relative_path_resolves_against_the_launch_cwd() {
        let t = target_from_args(&args(&["x", "sub/../a.ts"]), Path::new("/w/p")).unwrap();
        assert_eq!(t.path, "/w/p/a.ts");
    }

    #[test]
    fn an_absolute_path_loses_its_trailing_slash_and_flags_are_skipped() {
        let d = tempfile::tempdir().unwrap();
        let s = format!("{}/", d.path().display());
        let t = target_from_args(&args(&["x", "--flag", &s]), Path::new("/w")).unwrap();
        assert_eq!(t.path, d.path().to_string_lossy());
        assert!(t.is_dir);
    }
}
