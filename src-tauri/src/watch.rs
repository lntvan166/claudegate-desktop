//! Live watcher: notify events on `<home>/sessions` and `<home>` are coalesced for 150 ms and
//! turned into one `claudegate://changed` event. Read-only: nothing here creates or writes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const COALESCE_MS: u64 = 150;
pub const CHANGED_EVENT: &str = "claudegate://changed";

/// (project root, one of its scopes' session file). Rebuilt on every relist.
pub type KnownMap = Arc<Mutex<Vec<(String, PathBuf)>>>;

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Affected {
    pub roots: Vec<String>,
    /// roots list / excludes changed, or a session file we do not know yet: re-list everything.
    pub relist: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChangedPayload {
    pub roots: Vec<String>,
}

fn is_noise(name: &str) -> bool {
    name.ends_with(".lock") || name.ends_with(".tmp") || name.contains(".tmp.")
}

/// Which project views may have changed. `.lock`/`.tmp` files are ignored.
pub fn affected(changed: &[PathBuf], known: &[(String, PathBuf)]) -> Affected {
    let mut out = Affected::default();
    for p in changed {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if is_noise(name) {
            continue;
        }
        if name == "workspace-roots.json" || name == "excludes.json" {
            out.relist = true;
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let in_sessions = p
            .parent()
            .and_then(|d| d.file_name())
            .is_some_and(|d| d == "sessions");
        if !in_sessions {
            continue;
        }
        match known.iter().find(|(_, f)| f == p) {
            Some((root, _)) => {
                if !out.roots.contains(root) {
                    out.roots.push(root.clone());
                }
            }
            None => out.relist = true,
        }
    }
    out
}

pub fn affected_roots(changed: &[PathBuf], known: &[(String, PathBuf)]) -> Vec<String> {
    affected(changed, known).roots
}

/// Collects paths and releases them as one batch once `window` has passed since the first.
pub struct Coalescer {
    window: Duration,
    first: Option<Instant>,
    paths: Vec<PathBuf>,
}

impl Coalescer {
    pub fn new(window: Duration) -> Self {
        Self { window, first: None, paths: vec![] }
    }

    pub fn push(&mut self, now: Instant, paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            if !self.paths.contains(&p) {
                self.paths.push(p);
            }
        }
        if self.first.is_none() && !self.paths.is_empty() {
            self.first = Some(now);
        }
    }

    /// Time until the pending batch is due; `None` when nothing is pending.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.first
            .map(|f| (f + self.window).saturating_duration_since(now))
    }

    pub fn take_if_due(&mut self, now: Instant) -> Option<Vec<PathBuf>> {
        let f = self.first?;
        if now.duration_since(f) < self.window {
            return None;
        }
        self.first = None;
        Some(std::mem::take(&mut self.paths))
    }
}

/// Turns a settled batch into the payload to emit, or `None` when it was all noise.
pub fn payload_for(batch: &[PathBuf], known: &[(String, PathBuf)]) -> Option<ChangedPayload> {
    let a = affected(batch, known);
    if a.relist {
        return Some(ChangedPayload { roots: vec![] });
    }
    if a.roots.is_empty() {
        return None;
    }
    Some(ChangedPayload { roots: a.roots })
}

/// Directories we want watched right now: `home` and `home/sessions` when they exist, else the
/// nearest existing ancestor of `home` (so their creation is seen).
pub fn desired_dirs(home: &Path, exists: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    if exists(home) {
        let mut v = vec![home.to_path_buf()];
        let s = home.join("sessions");
        if exists(&s) {
            v.push(s);
        }
        return v;
    }
    home.ancestors()
        .skip(1)
        .find(|a| exists(a))
        .map(|a| vec![a.to_path_buf()])
        .unwrap_or_default()
}

/// Directories to attach now, given the set already watched.
pub fn dirs_to_add(home: &Path, watched: &[PathBuf], exists: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    desired_dirs(home, exists).into_iter().filter(|d| !watched.contains(d)).collect()
}

/// True when attaching `dirs` means the data we show may have appeared (home or sessions).
pub fn needs_relist(home: &Path, dirs: &[PathBuf]) -> bool {
    let s = home.join("sessions");
    dirs.iter().any(|d| d == home || *d == s)
}

fn attach(w: &mut notify::RecommendedWatcher, home: &Path, watched: &mut Vec<PathBuf>) -> Vec<PathBuf> {
    use notify::{RecursiveMode, Watcher};
    let mut done = vec![];
    for d in dirs_to_add(home, watched, &|p: &Path| p.is_dir()) {
        match w.watch(&d, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched.push(d.clone());
                done.push(d);
            }
            Err(e) => eprintln!("claudegate: cannot watch {}: {e}", d.display()),
        }
    }
    done
}

/// Starts the watcher thread. Missing directories are watched via an ancestor and attached when
/// created; nothing here creates or writes.
pub fn start(app: tauri::AppHandle, home: PathBuf, known: KnownMap) {
    use tauri::Emitter;

    let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            let _ = tx.send(ev.paths);
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("claudegate: watcher unavailable: {e}");
            return;
        }
    };
    let mut watched: Vec<PathBuf> = vec![];
    attach(&mut watcher, &home, &mut watched);
    let home2 = home.clone();
    std::thread::spawn(move || {
        let mut co = Coalescer::new(Duration::from_millis(COALESCE_MS));
        loop {
            let wait = co.remaining(Instant::now()).unwrap_or(Duration::from_secs(3600));
            match rx.recv_timeout(wait) {
                Ok(paths) => {
                    let added = attach(&mut watcher, &home2, &mut watched);
                    if needs_relist(&home2, &added) {
                        // a pseudo-change on a relist-trigger file: one empty-roots event
                        co.push(Instant::now(), [home2.join("workspace-roots.json")]);
                    }
                    co.push(Instant::now(), paths);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if let Some(batch) = co.take_if_due(Instant::now()) {
                let snapshot = known.lock().map(|k| k.clone()).unwrap_or_default();
                if let Some(p) = payload_for(&batch, &snapshot) {
                    let _ = app.emit(CHANGED_EVENT, p);
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Vec<(String, PathBuf)> {
        vec![
            ("/ws/a".into(), PathBuf::from("/h/sessions/aaa.json")),
            ("/ws/a".into(), PathBuf::from("/h/sessions/a-wt.json")),
            ("/ws/b".into(), PathBuf::from("/h/sessions/bbb.json")),
        ]
    }

    #[test]
    fn scope_b_session_maps_to_b_root() {
        let a = affected(&[PathBuf::from("/h/sessions/bbb.json")], &known());
        assert_eq!(a.roots, vec!["/ws/b".to_string()]);
        assert!(!a.relist);
    }

    #[test]
    fn worktree_scope_maps_to_primary_root_once() {
        let ch = [PathBuf::from("/h/sessions/aaa.json"), PathBuf::from("/h/sessions/a-wt.json")];
        assert_eq!(affected_roots(&ch, &known()), vec!["/ws/a".to_string()]);
    }

    #[test]
    fn roots_and_excludes_relist_without_roots() {
        for n in ["workspace-roots.json", "excludes.json"] {
            let a = affected(&[PathBuf::from("/h").join(n)], &known());
            assert!(a.roots.is_empty());
            assert!(a.relist, "{n}");
        }
    }

    #[test]
    fn lock_and_tmp_are_ignored() {
        let ch = [
            PathBuf::from("/h/sessions/bbb.json.lock"),
            PathBuf::from("/h/sessions/bbb.json.tmp"),
            PathBuf::from("/h/sessions/bbb.json.123.tmp"),
            PathBuf::from("/h/hook.log"),
        ];
        assert_eq!(affected(&ch, &known()), Affected::default());
    }

    #[test]
    fn unknown_session_file_relists() {
        let a = affected(&[PathBuf::from("/h/sessions/zzz.json")], &known());
        assert!(a.relist);
    }

    #[test]
    fn coalescer_batches_burst_into_one() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(150));
        assert_eq!(c.remaining(t0), None);
        c.push(t0, [PathBuf::from("/x")]);
        c.push(t0 + Duration::from_millis(100), [PathBuf::from("/x"), PathBuf::from("/y")]);
        assert_eq!(c.take_if_due(t0 + Duration::from_millis(149)), None);
        // window is anchored on the first event, not extended by later ones
        let batch = c.take_if_due(t0 + Duration::from_millis(150)).unwrap();
        assert_eq!(batch, vec![PathBuf::from("/x"), PathBuf::from("/y")]);
        assert_eq!(c.take_if_due(t0 + Duration::from_secs(1)), None);
        assert_eq!(c.remaining(t0 + Duration::from_secs(1)), None);
    }

    #[test]
    fn payload_is_none_for_noise_and_empty_for_relist() {
        let k = known();
        assert_eq!(payload_for(&[PathBuf::from("/h/sessions/x.lock")], &k), None);
        assert_eq!(
            payload_for(&[PathBuf::from("/h/excludes.json")], &k),
            Some(ChangedPayload { roots: vec![] })
        );
        assert_eq!(
            payload_for(&[PathBuf::from("/h/sessions/bbb.json")], &k),
            Some(ChangedPayload { roots: vec!["/ws/b".into()] })
        );
    }

    fn ex(set: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| set.iter().any(|s| Path::new(s) == p)
    }

    #[test]
    fn fresh_machine_watches_nearest_ancestor() {
        let home = Path::new("/u/.claudegate");
        assert_eq!(desired_dirs(home, &ex(&["/", "/u"])), vec![PathBuf::from("/u")]);
        assert_eq!(dirs_to_add(home, &[], &ex(&["/", "/u"])), vec![PathBuf::from("/u")]);
    }

    #[test]
    fn home_created_attaches_home_and_requests_relist() {
        let home = Path::new("/u/.claudegate");
        let watched = vec![PathBuf::from("/u")];
        let add = dirs_to_add(home, &watched, &ex(&["/", "/u", "/u/.claudegate"]));
        assert_eq!(add, vec![home.to_path_buf()]);
        assert!(needs_relist(home, &add));
    }

    #[test]
    fn sessions_created_later_is_attached_once() {
        let home = Path::new("/u/.claudegate");
        let e = ex(&["/u", "/u/.claudegate", "/u/.claudegate/sessions"]);
        let watched = vec![home.to_path_buf()];
        let add = dirs_to_add(home, &watched, &e);
        assert_eq!(add, vec![home.join("sessions")]);
        assert!(needs_relist(home, &add));
        let watched = vec![home.to_path_buf(), home.join("sessions")];
        assert!(dirs_to_add(home, &watched, &e).is_empty());
        assert!(!needs_relist(home, &[PathBuf::from("/u")]));
    }

    #[test]
    fn deep_missing_home_steps_through_intermediates() {
        let home = Path::new("/a/b/c");
        assert_eq!(desired_dirs(home, &ex(&["/a"])), vec![PathBuf::from("/a")]);
        assert_eq!(desired_dirs(home, &ex(&["/a", "/a/b"])), vec![PathBuf::from("/a/b")]);
    }
}
