//! Upkeep as the app runs it: only for roots it owns, under the write lock, with read-only read
//! afresh (fail closed), at most once per 5 s per root. Temp homes only.
use claudegate_core::decide::DecidePaths;
use claudegate_core::paths::session_file_for;
use claudegate_desktop_lib::config::{mark_added_by_app, save_read_only, set_managed_by_vscode};
use claudegate_desktop_lib::upkeep::{run_upkeep_with, Debouncer, Plan, UPKEEP_MIN_INTERVAL};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Fx {
    _tmp: tempfile::TempDir,
    paths: DecidePaths,
    ws: PathBuf,
    cfg: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("cg");
        let desktop_home = tmp.path().join("cgd");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::create_dir_all(&desktop_home).unwrap();
        fs::create_dir_all(&ws).unwrap();
        let cfg = desktop_home.join("config.json");
        Fx { paths: DecidePaths { home, desktop_home }, ws, cfg, _tmp: tmp }
    }
    fn root(&self) -> String {
        self.ws.to_string_lossy().into_owned()
    }
    fn session_file(&self) -> PathBuf {
        session_file_for(&self.paths.home, &self.ws)
    }
    /// One settled no-op (upkeep prunes it) and one real change.
    fn seed(&self) {
        let noop = self.ws.join("noop.ts");
        let real = self.ws.join("real.ts");
        fs::write(&noop, "same").unwrap();
        fs::write(&real, "new").unwrap();
        let e = |orig: &str| json!({ "originalContent": orig, "reviewStatus": "pending", "capturedAt": "2020-01-01T00:00:00.000Z" });
        let v = json!({ "sessionId": "s", "status": "active", "accepted": [], "rejected": {},
            "files": { noop.to_string_lossy(): e("same"), real.to_string_lossy(): e("old") } });
        fs::write(self.session_file(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    }
    fn raw(&self) -> String {
        fs::read_to_string(self.session_file()).unwrap()
    }
    fn pending(&self) -> usize {
        serde_json::from_str::<Value>(&self.raw()).unwrap()["files"].as_object().unwrap().len()
    }
    fn run(&self, lock: &Mutex<()>) -> bool {
        run_upkeep_with(lock, &self.paths, &self.cfg, &self.root(), 256, claudegate_core::time::now_ms())
            .is_some_and(|r| r.wrote())
    }
}

#[test]
fn an_owned_root_is_kept_up() {
    let fx = Fx::new();
    fx.seed();
    mark_added_by_app(&fx.cfg, &fx.root(), None).unwrap();
    assert!(fx.run(&Mutex::new(())));
    assert_eq!(fx.pending(), 1);
    // Its own write changes nothing the next run would act on.
    let after = fx.raw();
    assert!(!fx.run(&Mutex::new(())));
    assert_eq!(fx.raw(), after);
}

#[test]
fn a_vs_code_root_is_never_touched() {
    let fx = Fx::new();
    fx.seed();
    let before = fx.raw();
    assert!(!fx.run(&Mutex::new(())));
    assert_eq!(fx.raw(), before);
}

#[test]
fn a_root_vs_code_took_over_is_never_touched() {
    let fx = Fx::new();
    fx.seed();
    mark_added_by_app(&fx.cfg, &fx.root(), None).unwrap();
    set_managed_by_vscode(&fx.cfg, &fx.root(), true, None).unwrap();
    let before = fx.raw();
    assert!(!fx.run(&Mutex::new(())));
    assert_eq!(fx.raw(), before);
}

#[test]
fn read_only_mode_touches_nothing() {
    let fx = Fx::new();
    fx.seed();
    mark_added_by_app(&fx.cfg, &fx.root(), None).unwrap();
    save_read_only(&fx.cfg, true, None).unwrap();
    let before = fx.raw();
    assert!(!fx.run(&Mutex::new(())));
    assert_eq!(fx.raw(), before);
}

#[test]
fn an_unreadable_config_counts_as_read_only() {
    let fx = Fx::new();
    fx.seed();
    mark_added_by_app(&fx.cfg, &fx.root(), None).unwrap();
    // Still a record that says owned, but `readOnly` is malformed: fail closed.
    let mut v: Value = serde_json::from_str(&fs::read_to_string(&fx.cfg).unwrap()).unwrap();
    v["readOnly"] = json!("yes");
    fs::write(&fx.cfg, v.to_string()).unwrap();
    let before = fx.raw();
    assert!(!fx.run(&Mutex::new(())));
    assert_eq!(fx.raw(), before);
}

#[test]
fn it_waits_for_the_write_lock() {
    let fx = Arc::new(Fx::new());
    fx.seed();
    mark_added_by_app(&fx.cfg, &fx.root(), None).unwrap();
    let lock = Arc::new(Mutex::new(()));
    let held = lock.lock().unwrap();
    let before = fx.raw();
    let t = {
        let (fx, lock) = (fx.clone(), lock.clone());
        std::thread::spawn(move || fx.run(&lock))
    };
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(fx.raw(), before, "no write while a decision holds the lock");
    drop(held);
    assert!(t.join().unwrap());
    assert_eq!(fx.pending(), 1);
}

#[test]
fn at_most_once_per_interval_per_root() {
    let t0 = Instant::now();
    let mut d = Debouncer::default();
    assert_eq!(d.plan("/a", t0), Plan::Now);
    assert_eq!(d.plan("/b", t0), Plan::Now, "roots are independent");
    // Inside the window: one trailing run at the window's end, then nothing more is queued.
    let at = t0 + UPKEEP_MIN_INTERVAL;
    assert_eq!(d.plan("/a", t0 + Duration::from_secs(1)), Plan::At(at));
    assert_eq!(d.plan("/a", t0 + Duration::from_secs(2)), Plan::Skip);
    d.fired("/a", at);
    // The trailing run counts as a run: the event of its own write waits a full window again.
    assert_eq!(d.plan("/a", at + Duration::from_millis(200)), Plan::At(at + UPKEEP_MIN_INTERVAL));
    d.fired("/a", at + UPKEEP_MIN_INTERVAL);
    assert_eq!(d.plan("/a", at + UPKEEP_MIN_INTERVAL * 2), Plan::Now);
    assert_eq!(UPKEEP_MIN_INTERVAL, Duration::from_secs(5));
}
