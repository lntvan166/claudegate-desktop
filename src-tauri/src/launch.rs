//! `claudegate-desktop [PATH]`: the path a launch (first or second) asks the window to open.

use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const LAUNCH_EVENT: &str = "claudegate://launch";

/// What the UI is told: the absolute path, what the disk says about it (the UI cannot stat), and
/// its symlink-free form when there is one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTarget {
    pub path: String,
    pub exists: bool,
    pub is_dir: bool,
    pub canonical: Option<String>,
}

/// Every launch path reaches the UI through one channel. The first launch is only stored. A second
/// launch is emitted once the UI has pulled (`take`), and stored until then, so none is dropped
/// and none is applied twice.
#[derive(Default)]
pub struct Launches {
    queue: Vec<LaunchTarget>,
    ready: bool,
}

impl Launches {
    /// The first launch: stored, never emitted.
    pub fn first(&mut self, t: Option<LaunchTarget>) {
        self.queue.extend(t);
    }

    /// A second launch: `Some` when it should be emitted now, `None` when it was stored.
    pub fn second(&mut self, t: LaunchTarget) -> Option<LaunchTarget> {
        if self.ready {
            Some(t)
        } else {
            self.queue.push(t);
            None
        }
    }

    /// The UI's pull: everything stored, in order; from now on second launches are emitted.
    pub fn take(&mut self) -> Vec<LaunchTarget> {
        self.ready = true;
        std::mem::take(&mut self.queue)
    }
}

#[derive(Default)]
pub struct LaunchState(Mutex<Launches>);

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

/// The path in `args` (argv with the program name first): the first argument that is not a flag,
/// made absolute against `cwd`. A `--` ends flag parsing and the next argument is the path. This
/// is not a general flag parser: flags are skipped, never interpreted. `None` when there is no path.
pub fn target_from_args(args: &[String], cwd: &Path) -> Option<LaunchTarget> {
    let mut rest = args.iter().skip(1);
    let mut arg = None;
    while let Some(a) = rest.next() {
        if a == "--" {
            arg = rest.next();
            break;
        }
        if !a.is_empty() && !a.starts_with('-') {
            arg = Some(a);
            break;
        }
    }
    let arg = arg.filter(|a| !a.is_empty())?;
    let p = Path::new(arg);
    let abs = clean(&if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) });
    Some(LaunchTarget {
        exists: abs.exists(),
        is_dir: abs.is_dir(),
        canonical: std::fs::canonicalize(&abs).ok().map(|c| c.to_string_lossy().into_owned()),
        path: abs.to_string_lossy().into_owned(),
    })
}

/// Remembers the first launch's path; the UI pulls it.
pub fn remember(app: &AppHandle, args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_default();
    app.state::<LaunchState>().0.lock().unwrap().first(target_from_args(args, &cwd));
}

/// A second launch: focus the window and hand it the path.
pub fn second_instance(app: &AppHandle, args: &[String], cwd: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(t) = target_from_args(args, Path::new(cwd)) {
        let emit = app.state::<LaunchState>().0.lock().unwrap().second(t);
        if let Some(t) = emit {
            let _ = app.emit(LAUNCH_EVENT, t);
        }
    }
}

/// The launch paths stored so far, once; the UI pulls on mount. Later ones arrive as events.
#[tauri::command]
pub fn launch_path(state: tauri::State<LaunchState>) -> Vec<LaunchTarget> {
    state.0.lock().unwrap().take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn t(p: &str) -> LaunchTarget {
        LaunchTarget { path: p.into(), exists: true, is_dir: true, canonical: None }
    }

    #[test]
    fn the_first_launch_is_pulled_not_emitted() {
        let mut l = Launches::default();
        l.first(Some(t("/a")));
        assert_eq!(l.take(), vec![t("/a")]);
        assert!(l.take().is_empty(), "pulled once");
    }

    #[test]
    fn second_launches_wait_for_the_pull_then_emit() {
        let mut l = Launches::default();
        l.first(Some(t("/a")));
        assert_eq!(l.second(t("/b")), None);
        assert_eq!(l.second(t("/c")), None);
        assert_eq!(l.take(), vec![t("/a"), t("/b"), t("/c")]);
        assert_eq!(l.second(t("/d")), Some(t("/d")));
        assert!(l.take().is_empty(), "an emitted launch is not also stored");
    }

    #[test]
    fn double_dash_ends_flags() {
        let a = args(&["x", "--", "-weird"]);
        assert_eq!(target_from_args(&a, Path::new("/w")).unwrap().path, "/w/-weird");
        assert_eq!(target_from_args(&args(&["x", "--"]), Path::new("/w")), None);
    }

    #[test]
    fn a_missing_path_does_not_exist() {
        let t = target_from_args(&args(&["x", "/no/such/dir-xyz"]), Path::new("/w")).unwrap();
        assert!(!t.exists && !t.is_dir && t.canonical.is_none());
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
        assert!(t.is_dir && t.exists && t.canonical.is_some());
    }
}
