//! Live watcher: notify events on `<home>/sessions` and `<home>` are coalesced for 150 ms and
//! turned into one `claudegate://changed` event. Nothing here creates or writes; a change of
//! `workspace-roots.json` also runs the app's re-add of its own dropped roots, and a change of
//! `excludes.json` or the app's config runs the exclude sync of its own roots (`commands`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const COALESCE_MS: u64 = 150;
pub const CHANGED_EVENT: &str = "claudegate://changed";
/// A project's capture-audit file changed (`<home>/audit/<hash>.jsonl`). Separate from
/// `CHANGED_EVENT` because it fires on every Bash call Claude makes and must not reload a project.
pub const AUDIT_EVENT: &str = "claudegate://audit-changed";

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

/// True for the event kinds that can change what a session file says: creation, a data or name
/// change, removal. Opening, reading, closing and metadata-only changes are not: the app reads
/// these files itself, and inotify reports every one of those opens.
pub fn is_content_change(kind: &notify::EventKind) -> bool {
    use notify::event::ModifyKind;
    use notify::EventKind;
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_) | ModifyKind::Any) => true,
        _ => false,
    }
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

/// The project roots whose audit file is in `changed`. The audit file is named after the same
/// hash as its scope's session file, so the stem finds the project in `known`. Rotation
/// (`.jsonl.1`) counts; `.lock`/`.tmp` files and unknown hashes do not.
pub fn audit_roots(changed: &[PathBuf], known: &[(String, PathBuf)]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for p in changed {
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        if is_noise(name) || !p.parent().and_then(|d| d.file_name()).is_some_and(|d| d == "audit") {
            continue;
        }
        let Some(stem) = name.strip_suffix(".jsonl.1").or_else(|| name.strip_suffix(".jsonl")) else { continue };
        for (root, f) in known {
            if f.file_stem().and_then(|s| s.to_str()) == Some(stem) && !out.contains(root) {
                out.push(root.clone());
            }
        }
    }
    out
}

/// True when attaching `dirs` added the audit directory.
pub fn audit_dir_added(home: &Path, dirs: &[PathBuf]) -> bool {
    let a = home.join("audit");
    dirs.iter().any(|d| *d == a)
}

/// Every distinct project root in `known`, in order.
pub fn all_roots(known: &[(String, PathBuf)]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for (r, _) in known {
        if !out.contains(r) {
            out.push(r.clone());
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

/// Directories we want watched right now: `home`, `home/sessions` and `home/audit` when they exist, else the
/// nearest existing ancestor of `home` (so their creation is seen).
pub fn desired_dirs(home: &Path, exists: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    if exists(home) {
        let mut v = vec![home.to_path_buf()];
        let s = home.join("sessions");
        if exists(&s) {
            v.push(s);
        }
        let a = home.join("audit");
        if exists(&a) {
            v.push(a);
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

fn attach(w: &mut notify::RecommendedWatcher, home: &Path, extra: Option<&Path>, watched: &mut Vec<PathBuf>) -> Vec<PathBuf> {
    use notify::{RecursiveMode, Watcher};
    let mut done = vec![];
    let mut dirs = dirs_to_add(home, watched, &|p: &Path| p.is_dir());
    // The app's own config folder, for the exclude sync (attached once it exists).
    if let Some(x) = extra.filter(|x| x.is_dir() && !watched.iter().any(|w| w == x) && !dirs.iter().any(|d| d == x)) {
        dirs.push(x.to_path_buf());
    }
    for d in dirs {
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
    let cg = home.clone();
    let cg2 = home.clone();
    let config = crate::config::config_path();
    let app_audit = app.clone();
    run_with_hooks(
        home,
        known,
        Some(config.clone()),
        move |p| {
            // A project's session changed: keep it up if the app owns it (rate-limited).
            for r in &p.roots {
                crate::upkeep::trigger(&app, r);
            }
            let _ = app.emit(CHANGED_EVENT, p);
        },
        // The extension rewrites workspace-roots.json without a lock and can drop a root the app
        // added; put it back. Writes nothing when nothing is missing, so our own write's event
        // does not loop.
        move || {
            crate::commands::ensure_roots_with(&cg, &crate::config::config_path());
        },
        // excludes.json or the config changed: write the owned roots' exclude, or notice that
        // VS Code took one over. Compares before writing, so its own writes do not loop.
        move || {
            crate::commands::sync_excludes_with(&cg2, &config);
        },
        // The audit file of a project changed: tell the window, which re-reads it (throttled there).
        move |p| {
            let _ = app_audit.emit(AUDIT_EVENT, p);
        },
    );
}

/// True when a settled batch touched `workspace-roots.json`.
pub fn touches_roots_file(batch: &[PathBuf]) -> bool {
    batch.iter().any(|p| p.file_name().is_some_and(|n| n == "workspace-roots.json"))
}

/// True when a settled batch touched `excludes.json` or the app's config file (`config`).
pub fn touches_excludes_or_config(batch: &[PathBuf], config: Option<&Path>) -> bool {
    batch.iter().any(|p| p.file_name().is_some_and(|n| n == "excludes.json") || Some(p.as_path()) == config)
}

/// The watcher loop behind `start`, with the emitter injected so it can be driven by a test.
/// The thread lives as long as the process.
pub fn run(home: PathBuf, known: KnownMap, emit: impl Fn(ChangedPayload) + Send + 'static) {
    run_with_hooks(home, known, None, emit, || {}, || {}, |_| {});
}

/// `run`, also calling (before the emit, once per settled batch; the 150 ms coalescing is the
/// debounce) `on_roots` when the batch touched `workspace-roots.json`, and `on_sync` when it
/// touched `excludes.json` or `config`, whose folder is watched too, and `on_audit` with the
/// project roots whose audit file changed (watch only; the audit files are never written).
pub fn run_with_hooks(
    home: PathBuf,
    known: KnownMap,
    config: Option<PathBuf>,
    emit: impl Fn(ChangedPayload) + Send + 'static,
    on_roots: impl Fn() + Send + 'static,
    on_sync: impl Fn() + Send + 'static,
    on_audit: impl Fn(ChangedPayload) + Send + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<PathBuf>>();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if is_content_change(&ev.kind) {
                let _ = tx.send(ev.paths);
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("claudegate: watcher unavailable: {e}");
            return;
        }
    };
    let mut watched: Vec<PathBuf> = vec![];
    let config_dir = config.as_ref().and_then(|c| c.parent()).map(Path::to_path_buf);
    attach(&mut watcher, &home, config_dir.as_deref(), &mut watched);
    let home2 = home.clone();
    std::thread::spawn(move || {
        let mut co = Coalescer::new(Duration::from_millis(COALESCE_MS));
        loop {
            let wait = co.remaining(Instant::now()).unwrap_or(Duration::from_secs(3600));
            match rx.recv_timeout(wait) {
                Ok(paths) => {
                    let added = attach(&mut watcher, &home2, config_dir.as_deref(), &mut watched);
                    if audit_dir_added(&home2, &added) {
                        // A write can land before the attach sees the new dir: say every project
                        // may have changed once (the window throttles its reads).
                        let roots = all_roots(&known.lock().map(|k| k.clone()).unwrap_or_default());
                        if !roots.is_empty() {
                            on_audit(ChangedPayload { roots });
                        }
                    }
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
                if touches_roots_file(&batch) {
                    on_roots();
                }
                if touches_excludes_or_config(&batch, config.as_deref()) {
                    on_sync();
                }
                let snapshot = known.lock().map(|k| k.clone()).unwrap_or_default();
                let ar = audit_roots(&batch, &snapshot);
                if !ar.is_empty() {
                    on_audit(ChangedPayload { roots: ar });
                }
                if let Some(p) = payload_for(&batch, &snapshot) {
                    emit(p);
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
    fn only_content_changes_count() {
        use notify::event::{
            AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind,
            RenameMode,
        };
        use notify::EventKind as K;
        for k in [
            K::Create(CreateKind::File),
            K::Create(CreateKind::Any),
            K::Modify(ModifyKind::Data(DataChange::Any)),
            K::Modify(ModifyKind::Data(DataChange::Content)),
            K::Modify(ModifyKind::Name(RenameMode::To)),
            K::Modify(ModifyKind::Name(RenameMode::Both)),
            K::Modify(ModifyKind::Any),
            K::Remove(RemoveKind::File),
        ] {
            assert!(is_content_change(&k), "{k:?} should count");
        }
        for k in [
            K::Access(AccessKind::Open(AccessMode::Any)),
            K::Access(AccessKind::Read),
            K::Access(AccessKind::Close(AccessMode::Read)),
            K::Access(AccessKind::Close(AccessMode::Write)),
            K::Modify(ModifyKind::Metadata(MetadataKind::Any)),
            K::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)),
            K::Any,
            K::Other,
        ] {
            assert!(!is_content_change(&k), "{k:?} should not count");
        }
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
    fn only_the_roots_file_runs_the_re_add() {
        assert!(touches_roots_file(&[PathBuf::from("/h/sessions/a.json"), PathBuf::from("/h/workspace-roots.json")]));
        assert!(!touches_roots_file(&[PathBuf::from("/h/excludes.json"), PathBuf::from("/h/sessions/a.json")]));
        assert!(!touches_roots_file(&[PathBuf::from("/h/workspace-roots.json.0123456789ab.tmp")]));
    }

    #[test]
    fn excludes_and_the_config_run_the_sync() {
        let cfg = PathBuf::from("/u/.claudegate-desktop/config.json");
        assert!(touches_excludes_or_config(&[PathBuf::from("/h/excludes.json")], Some(&cfg)));
        assert!(touches_excludes_or_config(&[cfg.clone()], Some(&cfg)));
        assert!(!touches_excludes_or_config(&[PathBuf::from("/h/workspace-roots.json")], Some(&cfg)));
        assert!(!touches_excludes_or_config(&[PathBuf::from("/h/excludes.json.0123456789ab.tmp")], Some(&cfg)));
        assert!(!touches_excludes_or_config(&[PathBuf::from("/u/.claudegate-desktop/.config.json.9.tmp")], Some(&cfg)));
        assert!(!touches_excludes_or_config(&[cfg.clone()], None));
    }

    #[test]
    fn audit_files_map_to_their_project_and_never_relist() {
        let k = known();
        let a = [PathBuf::from("/h/audit/bbb.jsonl"), PathBuf::from("/h/audit/a-wt.jsonl.1"), PathBuf::from("/h/audit/zzz.jsonl")];
        assert_eq!(audit_roots(&a, &k), vec!["/ws/b".to_string(), "/ws/a".to_string()]);
        // an audit write is not a session change: it must not reload the project or relist
        assert_eq!(payload_for(&a, &k), None);
        assert!(audit_roots(&[PathBuf::from("/h/audit/bbb.jsonl.lock"), PathBuf::from("/h/sessions/bbb.jsonl")], &k).is_empty());
    }

    #[test]
    fn a_new_audit_dir_reports_every_known_root_once() {
        let home = Path::new("/h");
        assert!(audit_dir_added(home, &[PathBuf::from("/h/audit")]));
        assert!(!audit_dir_added(home, &[PathBuf::from("/h/sessions"), PathBuf::from("/h")]));
        assert_eq!(all_roots(&known()), vec!["/ws/a".to_string(), "/ws/b".to_string()]);
    }

    #[test]
    fn audit_dir_is_watched_once_it_exists() {
        let home = Path::new("/u/.claudegate");
        let e = ex(&["/u", "/u/.claudegate", "/u/.claudegate/sessions", "/u/.claudegate/audit"]);
        assert_eq!(desired_dirs(home, &e), vec![home.to_path_buf(), home.join("sessions"), home.join("audit")]);
        // attaching it is no reason to relist the projects
        assert!(!needs_relist(home, &[home.join("audit")]));
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
