//! Runs the core's session upkeep for the projects the app owns: after the app loads a project and
//! on that project's session events. At most once per `UPKEEP_MIN_INTERVAL` per root; a trigger
//! inside the window queues one trailing run at its end. Each run happens on the blocking pool
//! holding the app's write lock, with read-only mode read afresh (an unreadable config counts as
//! read-only).
//!
//! Upkeep writes only when it changed something, so the watch event of its own write leads to
//! one trailing run that writes nothing, and the chain stops there.

use claudegate_core::decide::DecidePaths;
use claudegate_core::upkeep::{upkeep, UpkeepOpts, UpkeepReport};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const UPKEEP_MIN_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Run now (recorded as the root's last run).
    Now,
    /// Run at this moment; call `fired` when it starts.
    At(Instant),
    /// A run is already queued.
    Skip,
}

#[derive(Debug, Default)]
struct RootState {
    last: Option<Instant>,
    queued: bool,
}

/// Per-root rate limit: leading run, then at most one trailing run per window.
#[derive(Debug, Default)]
pub struct Debouncer {
    roots: HashMap<String, RootState>,
}

impl Debouncer {
    pub fn plan(&mut self, root: &str, now: Instant) -> Plan {
        let st = self.roots.entry(root.to_owned()).or_default();
        if st.queued {
            return Plan::Skip;
        }
        match st.last {
            Some(last) if now.saturating_duration_since(last) < UPKEEP_MIN_INTERVAL => {
                st.queued = true;
                Plan::At(last + UPKEEP_MIN_INTERVAL)
            }
            _ => {
                st.last = Some(now);
                Plan::Now
            }
        }
    }

    /// A queued run starts at `now`.
    pub fn fired(&mut self, root: &str, now: Instant) {
        let st = self.roots.entry(root.to_owned()).or_default();
        st.queued = false;
        st.last = Some(now);
    }
}

/// The app's debouncer, shared by the commands and the watcher.
#[derive(Default, Clone)]
pub struct UpkeepState(pub Arc<Mutex<Debouncer>>);

/// One upkeep of `root` minus Tauri. `None` when the root is not owned or the app's home is not
/// usable for backups. Ownership is checked before taking `lock` (cheap, and most roots are VS
/// Code's), then again under it with read-only mode read afresh.
pub fn run_upkeep_with(
    lock: &Mutex<()>,
    paths: &DecidePaths,
    config_path: &Path,
    root: &str,
    max_attached: usize,
    now_ms: i64,
) -> Option<UpkeepReport> {
    let root = claudegate_core::paths::resolve_root(Path::new(root));
    let key = root.to_string_lossy();
    if !crate::config::project_record(config_path, &key).owned() {
        return None;
    }
    crate::commands::desktop_home_for(config_path, &paths.home).ok()?;
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let record = crate::config::project_record(config_path, &key);
    let read_only = crate::config::read_only_strict(config_path).unwrap_or(true);
    let report = upkeep(paths, &root, &UpkeepOpts { record, read_only, max_attached }, now_ms);
    for s in &report.scopes {
        if let Some(e) = &s.error {
            eprintln!("claudegate: upkeep of {}: {e}", s.scope);
        }
    }
    Some(report)
}

/// `run_upkeep_with` for the running app.
fn run_now(app: &tauri::AppHandle, root: &str) {
    use tauri::Manager;
    let home = claudegate_core::paths::claudegate_home();
    let cfg = crate::config::config_path();
    let Ok(desktop_home) = crate::commands::desktop_home_for(&cfg, &home) else { return };
    let max = crate::commands::max_attached_for(&cfg, Path::new(root), &crate::commands::ext_sources());
    let lock = app.state::<crate::commands::WriteLock>();
    let paths = DecidePaths { home, desktop_home };
    run_upkeep_with(&lock.0, &paths, &cfg, root, max, claudegate_core::time::now_ms());
}

/// Upkeep `root` soon, if the app owns it, within the rate limit. Never blocks the caller.
pub fn trigger(app: &tauri::AppHandle, root: &str) {
    use tauri::Manager;
    let cfg = crate::config::config_path();
    if !crate::config::project_record(&cfg, root).owned() {
        return;
    }
    let state = app.state::<UpkeepState>().inner().clone();
    let plan = state.0.lock().map(|mut d| d.plan(root, Instant::now())).unwrap_or(Plan::Skip);
    let (app, root) = (app.clone(), root.to_owned());
    match plan {
        Plan::Skip => {}
        Plan::Now => {
            tauri::async_runtime::spawn_blocking(move || run_now(&app, &root));
        }
        Plan::At(at) => {
            std::thread::spawn(move || {
                std::thread::sleep(at.saturating_duration_since(Instant::now()));
                if let Ok(mut d) = state.0.lock() {
                    d.fired(&root, Instant::now());
                }
                tauri::async_runtime::spawn_blocking(move || run_now(&app, &root));
            });
        }
    }
}
