//! upkeep: the extension's session housekeeping (`sessionManager.ts` v1.16.1: reconcilePending,
//! pruneOutOfWorkspaceEntries, cleanupExcludedOnce, healOversized) for the roots the app owns.
//! `fixtures/upkeep/*.json` carry the extension's own `capAcceptedBytes` / `shouldPruneNoOp`
//! answers (see xcheck/run.ts). Temp dirs only.

use claudegate_core::decide::DecidePaths;
use claudegate_core::model::{cap_accepted_bytes, parse_session, FileEntry};
use claudegate_core::paths::session_file_for;
use claudegate_core::project::should_prune_no_op;
use claudegate_core::projects::ProjectRecord;
use claudegate_core::upkeep::{upkeep, upkeep_with, Skip, UpkeepOpts, MAX_ACCEPTED_BYTES, SESSION_SIZE_WARN_BYTES};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const T0: i64 = 1_800_000_000_000; // "now" for every test

fn iso(ms: i64) -> String {
    claudegate_core_iso(ms)
}

/// `new Date(ms).toISOString()`.
fn claudegate_core_iso(ms: i64) -> String {
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let s = rem / 1000;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", s / 3600, s % 3600 / 60, s % 60, rem % 1000)
}

fn ago(ms: i64) -> String {
    iso(T0 - ms)
}

struct Fx {
    _tmp: tempfile::TempDir,
    paths: DecidePaths,
    ws: PathBuf,
}

fn owned() -> UpkeepOpts {
    UpkeepOpts {
        record: ProjectRecord { added_by_app: true, ..Default::default() },
        read_only: false,
        max_attached: 256,
    }
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("cg");
        let desktop_home = tmp.path().join("cgd");
        let ws = tmp.path().join("ws");
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::create_dir_all(&ws).unwrap();
        Fx { paths: DecidePaths { home, desktop_home }, ws, _tmp: tmp }
    }

    fn file(&self, rel: &str, content: &str) -> String {
        let p = self.ws.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn path(&self, rel: &str) -> String {
        self.ws.join(rel).to_string_lossy().into_owned()
    }

    fn worktree(&self, name: &str) -> PathBuf {
        let wt = self.ws.join(name);
        let gitdir = self.ws.join(".git").join("worktrees").join(name);
        fs::create_dir_all(&gitdir).unwrap();
        fs::create_dir_all(&wt).unwrap();
        fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        wt
    }

    fn session_file(&self, scope: &Path) -> PathBuf {
        session_file_for(&self.paths.home, scope)
    }

    fn write_session(&self, scope: &Path, v: &Value) {
        fs::write(self.session_file(scope), serde_json::to_string_pretty(v).unwrap()).unwrap();
    }

    fn raw(&self, scope: &Path) -> String {
        fs::read_to_string(self.session_file(scope)).unwrap()
    }

    fn session(&self, scope: &Path) -> Value {
        serde_json::from_str(&self.raw(scope)).unwrap()
    }

    fn backups(&self, scope: &Path) -> usize {
        let hash = self.session_file(scope).file_stem().unwrap().to_owned();
        fs::read_dir(self.paths.desktop_home.join("backups").join(hash))
            .map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).count())
            .unwrap_or(0)
    }
}

fn entry(original: Option<&str>, captured_at: Option<String>) -> Value {
    let mut e = json!({ "originalContent": original, "reviewStatus": "pending", "sessionId": "s1" });
    if let Some(c) = captured_at {
        e["capturedAt"] = json!(c);
    }
    e
}

fn sess(files: Value, accepted: Value) -> Value {
    json!({ "sessionId": "x", "status": "active", "files": files, "accepted": accepted, "rejected": {} })
}

fn files_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v["files"].as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

// ---------------------------------------------------------------- fixtures (extension answers)

fn fixtures() -> Vec<(String, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/upkeep");
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    v.sort();
    v.into_iter()
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap()))
        .collect()
}

#[test]
fn cap_accepted_bytes_matches_the_extension() {
    let mut n = 0;
    for (name, fx) in fixtures().into_iter().filter(|(_, f)| f["op"] == "capAcceptedBytes") {
        let (mut s, _) = parse_session(&fx["session"].to_string()).unwrap();
        let dropped = cap_accepted_bytes(&mut s, fx["maxBytes"].as_u64().unwrap() as usize);
        let ids: Vec<Value> = s.accepted.iter().map(|r| json!(r.id)).collect();
        assert_eq!(dropped as u64, fx["expected"]["dropped"].as_u64().unwrap(), "{name}: dropped");
        assert_eq!(Value::Array(ids), fx["expected"]["acceptedIds"], "{name}: kept ids");
        n += 1;
    }
    assert!(n >= 8, "expected every cap fixture, found {n}");
}

#[test]
fn should_prune_no_op_matches_the_extension() {
    let mut n = 0;
    for (_, fx) in fixtures().into_iter().filter(|(_, f)| f["op"] == "shouldPruneNoOp") {
        for c in fx["cases"].as_array().unwrap() {
            let e = FileEntry {
                original_content: c["entry"]["originalContent"].as_str().map(str::to_owned),
                new_file: None,
                session_id: None,
                captured_at: c["entry"]["capturedAt"].as_str().map(str::to_owned),
            };
            let got = should_prune_no_op(&e, c["disk"].as_str(), c["nowMs"].as_i64().unwrap());
            assert_eq!(got, c["prune"].as_bool().unwrap(), "{}", c["name"]);
            n += 1;
        }
    }
    assert!(n >= 16, "expected every prune case, found {n}");
}

// ---------------------------------------------------------------- reconcilePending

#[test]
fn settled_no_ops_are_pruned_and_young_ones_kept() {
    let fx = Fx::new();
    let same_young = fx.file("young.ts", "a");
    let same_old = fx.file("old.ts", "a");
    let real = fx.file("real.ts", "changed");
    let new_young = fx.path("new-young.ts"); // promised new file, not on disk yet
    let new_old = fx.path("new-old.ts");
    let new_mid = fx.path("new-mid.ts"); // older than 15 s but younger than 45 s
    fx.write_session(
        &fx.ws,
        &sess(
            json!({
                same_young.clone(): entry(Some("a"), Some(ago(14_000))),
                same_old.clone(): entry(Some("a"), Some(ago(16_000))),
                real.clone(): entry(Some("a"), Some(ago(1_000_000))),
                new_young.clone(): entry(None, Some(ago(1_000))),
                new_old.clone(): entry(None, Some(ago(46_000))),
                new_mid.clone(): entry(None, Some(ago(30_000))),
            }),
            json!([]),
        ),
    );
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert_eq!(r.skipped, None);
    let s = fx.session(&fx.ws);
    let mut want = vec![same_young, real, new_young, new_mid];
    want.sort();
    assert_eq!(files_of(&s), want);
    assert_eq!(s["excludeCleanupDone"], json!(true));
    assert_eq!(fx.backups(&fx.ws), 1, "one write, one backup");
}

#[test]
fn a_second_run_writes_nothing() {
    let fx = Fx::new();
    let old = fx.file("old.ts", "a");
    let young = fx.file("young.ts", "a");
    fx.write_session(
        &fx.ws,
        &sess(json!({ old: entry(Some("a"), Some(ago(20_000))), young: entry(Some("a"), Some(ago(1_000))) }), json!([])),
    );
    upkeep(&fx.paths, &fx.ws, &owned(), T0);
    let after_first = fx.raw(&fx.ws);
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0 + 1_000);
    assert_eq!(fx.raw(&fx.ws), after_first);
    assert!(r.scopes.iter().all(|s| !s.written), "{r:?}");
    assert_eq!(fx.backups(&fx.ws), 1);
}

#[test]
fn nothing_to_do_writes_nothing() {
    let fx = Fx::new();
    let real = fx.file("real.ts", "b");
    let v = json!({ "sessionId": "x", "status": "active", "files": { real: entry(Some("a"), Some(ago(1))) },
                    "accepted": [], "rejected": {}, "excludeCleanupDone": true });
    fx.write_session(&fx.ws, &v);
    let before = fx.raw(&fx.ws);
    upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert_eq!(fx.raw(&fx.ws), before);
    assert_eq!(fx.backups(&fx.ws), 0);
}

// ---------------------------------------------------------------- healOversized

fn big_record(i: usize, size: usize) -> Value {
    let at = iso(T0 - 1_000_000 + i as i64 * 1000);
    let p = format!("/elsewhere/f{i}.txt");
    json!({ "id": format!("{at}::{p}"), "path": p, "before": "", "after": "x".repeat(size), "decidedAt": at, "sessionId": "s" })
}

#[test]
fn an_oversized_session_is_trimmed_oldest_first() {
    let fx = Fx::new();
    let recs: Vec<Value> = (0..25).map(|i| big_record(i, 100_000)).collect();
    let v = json!({ "sessionId": "x", "status": "reviewed", "files": {}, "accepted": recs, "rejected": {}, "excludeCleanupDone": true });
    fx.write_session(&fx.ws, &v);
    assert!(fx.raw(&fx.ws).len() as u64 > SESSION_SIZE_WARN_BYTES);
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    let s = fx.session(&fx.ws);
    let kept: Vec<String> = s["accepted"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect();
    let all: Vec<String> = recs.iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect();
    assert!(kept.len() < all.len() && !kept.is_empty());
    assert_eq!(kept[..], all[all.len() - kept.len()..], "the newest are kept, in order");
    let bytes: usize = s["accepted"].as_array().unwrap().iter().map(|r| serde_json::to_string(r).unwrap().len()).sum();
    assert!(bytes <= MAX_ACCEPTED_BYTES, "{bytes}");
    assert_eq!(r.scopes[0].trimmed_accepted, all.len() - kept.len());
    // The trimmed ids were tombstoned: the still-large disk copy did not bring them back, and the
    // next run (now under the warning size) changes nothing.
    let after = fx.raw(&fx.ws);
    upkeep(&fx.paths, &fx.ws, &owned(), T0 + 10_000);
    assert_eq!(fx.raw(&fx.ws), after);
}

#[test]
fn a_session_under_the_warning_size_is_not_trimmed() {
    let fx = Fx::new();
    let recs: Vec<Value> = (0..18).map(|i| big_record(i, 100_000)).collect(); // ~1.8 MB of accepted
    let v = json!({ "sessionId": "x", "status": "reviewed", "files": {}, "accepted": recs, "rejected": {}, "excludeCleanupDone": true });
    fx.write_session(&fx.ws, &v);
    assert!((fx.raw(&fx.ws).len() as u64) < SESSION_SIZE_WARN_BYTES);
    let before = fx.raw(&fx.ws);
    upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert_eq!(fx.raw(&fx.ws), before);
}

// ---------------------------------------------------------------- cleanupExcludedOnce

#[test]
fn the_excluded_sweep_runs_once() {
    let fx = Fx::new();
    let lock = fx.file("package-lock.json", "{}2"); // default exclude
    let dep = fx.file("node_modules/x/index.js", "2");
    let custom = fx.file("gen/out.ts", "2");
    let kept = fx.file("src/a.ts", "2");
    fx.write_session(
        &fx.ws,
        &sess(
            json!({
                lock.clone(): entry(Some("1"), Some(ago(5))),
                dep.clone(): entry(Some("1"), Some(ago(5))),
                custom.clone(): entry(Some("1"), Some(ago(5))),
                kept.clone(): entry(Some("1"), Some(ago(5))),
            }),
            json!([]),
        ),
    );
    // Defaults: the lockfile and node_modules go; gen/ is not excluded.
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    let s = fx.session(&fx.ws);
    let mut want = vec![custom.clone(), kept.clone()];
    want.sort();
    assert_eq!(files_of(&s), want);
    assert_eq!(s["excludeCleanupDone"], json!(true));
    assert!(r.scopes[0].latched);
    // A pattern added later, and an excluded entry captured since: never swept again.
    let mut s2 = s.clone();
    s2["files"][lock.clone()] = entry(Some("1"), Some(ago(5)));
    fx.write_session(&fx.ws, &s2);
    let mut opts = owned();
    opts.record.exclude = Some(vec!["**/gen/**".into(), "**/package-lock.json".into()]);
    upkeep(&fx.paths, &fx.ws, &opts, T0 + 6_000);
    let s3 = fx.session(&fx.ws);
    let mut want = vec![lock, custom, kept];
    want.sort();
    assert_eq!(files_of(&s3), want);
}

#[test]
fn the_sweep_uses_the_projects_own_exclude_and_latches_even_with_no_match() {
    let fx = Fx::new();
    let custom = fx.file("gen/out.ts", "2");
    let lock = fx.file("package-lock.json", "{}2");
    fx.write_session(&fx.ws, &sess(json!({ custom.clone(): entry(Some("1"), Some(ago(5))), lock.clone(): entry(Some("1"), Some(ago(5))) }), json!([])));
    let mut opts = owned();
    opts.record.exclude = Some(vec!["**/gen/**".into()]);
    upkeep(&fx.paths, &fx.ws, &opts, T0);
    assert_eq!(files_of(&fx.session(&fx.ws)), vec![lock.clone()]);

    let fx2 = Fx::new();
    let a = fx2.file("a.ts", "2");
    fx2.write_session(&fx2.ws, &sess(json!({ a: entry(Some("1"), Some(ago(5))) }), json!([])));
    upkeep(&fx2.paths, &fx2.ws, &owned(), T0);
    assert_eq!(fx2.session(&fx2.ws)["excludeCleanupDone"], json!(true), "written so the latch sticks");
}

// ---------------------------------------------------------------- pruneOutOfWorkspaceEntries

#[test]
fn pending_entries_outside_the_root_are_pruned_records_are_not() {
    let fx = Fx::new();
    let inside = fx.file("a.ts", "2");
    let outside = fx._tmp.path().join("elsewhere.ts");
    fs::write(&outside, "2").unwrap();
    let outside = outside.to_string_lossy().into_owned();
    let sibling = format!("{}x/b.ts", fx.ws.display()); // `<ws>x/...`: a prefix, not inside
    let rec = json!({ "id": format!("{}::{outside}", ago(1)), "path": outside, "before": "1", "after": "2", "decidedAt": ago(1) });
    let v = json!({ "sessionId": "x", "status": "active", "excludeCleanupDone": true, "rejected": { outside.clone(): rec.clone() },
        "files": { inside.clone(): entry(Some("1"), Some(ago(5))), outside.clone(): entry(Some("1"), Some(ago(5))), sibling.clone(): entry(Some("1"), Some(ago(5))) },
        "accepted": [rec] });
    fx.write_session(&fx.ws, &v);
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    let s = fx.session(&fx.ws);
    assert_eq!(files_of(&s), vec![inside]);
    assert_eq!(s["accepted"].as_array().unwrap().len(), 1);
    assert_eq!(s["rejected"].as_object().unwrap().len(), 1);
    let mut out = r.scopes[0].pruned_outside.clone();
    out.sort();
    let mut want = vec![outside, sibling];
    want.sort();
    assert_eq!(out, want);
}

// ---------------------------------------------------------------- scopes

#[test]
fn every_worktree_scope_is_kept_up() {
    let fx = Fx::new();
    let wt = fx.worktree("wt1");
    let a = fx.file("wt1/a.ts", "a");
    fx.write_session(&fx.ws, &sess(json!({}), json!([])));
    fx.write_session(&wt, &sess(json!({ a: entry(Some("a"), Some(ago(20_000))) }), json!([])));
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert_eq!(r.scopes.len(), 2);
    assert!(files_of(&fx.session(&wt)).is_empty());
    assert_eq!(fx.session(&wt)["excludeCleanupDone"], json!(true));
}

// ---------------------------------------------------------------- never touched

fn untouched(fx: &Fx, opts: &UpkeepOpts) -> claudegate_core::upkeep::UpkeepReport {
    let old = fx.file("old.ts", "a");
    let dep = fx.file("node_modules/x.js", "2");
    fx.write_session(&fx.ws, &sess(json!({ old: entry(Some("a"), Some(ago(60_000))), dep: entry(Some("1"), Some(ago(5))) }), json!([])));
    let before = fx.raw(&fx.ws);
    let r = upkeep(&fx.paths, &fx.ws, opts, T0);
    assert_eq!(fx.raw(&fx.ws), before);
    assert_eq!(fx.backups(&fx.ws), 0);
    r
}

#[test]
fn a_vs_code_root_is_never_touched() {
    let fx = Fx::new();
    let opts = UpkeepOpts { record: ProjectRecord::default(), read_only: false, max_attached: 256 };
    assert_eq!(untouched(&fx, &opts).skipped, Some(Skip::NotOwned));
}

#[test]
fn a_root_vs_code_took_over_is_never_touched() {
    let fx = Fx::new();
    let opts = UpkeepOpts {
        record: ProjectRecord { added_by_app: true, managed_by_vscode: true, ..Default::default() },
        read_only: false,
        max_attached: 256,
    };
    assert_eq!(untouched(&fx, &opts).skipped, Some(Skip::NotOwned));
}

#[test]
fn read_only_mode_touches_nothing() {
    let fx = Fx::new();
    let mut opts = owned();
    opts.read_only = true;
    assert_eq!(untouched(&fx, &opts).skipped, Some(Skip::ReadOnly));
}

#[test]
fn an_unknown_shape_or_unreadable_session_is_left_alone() {
    let fx = Fx::new();
    let old = fx.file("old.ts", "a");
    let mut v = sess(json!({ old: entry(Some("a"), Some(ago(60_000))) }), json!([]));
    v["newerField"] = json!(1);
    fx.write_session(&fx.ws, &v);
    let before = fx.raw(&fx.ws);
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert_eq!(fx.raw(&fx.ws), before);
    assert!(r.scopes[0].skipped.is_some());

    fs::write(fx.session_file(&fx.ws), "{ not json").unwrap();
    let r = upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert_eq!(fx.raw(&fx.ws), "{ not json");
    assert!(r.scopes[0].skipped.is_some());
    assert_eq!(fx.backups(&fx.ws), 0);
}

#[test]
fn no_session_file_creates_none() {
    let fx = Fx::new();
    upkeep(&fx.paths, &fx.ws, &owned(), T0);
    assert!(!fx.session_file(&fx.ws).exists());
}

// ---------------------------------------------------------------- concurrency

#[test]
fn a_hook_capture_landing_mid_upkeep_survives() {
    let fx = Fx::new();
    let old = fx.file("old.ts", "a");
    let fresh = fx.file("fresh.ts", "1");
    let same_path_recaptured = old.clone();
    fx.write_session(&fx.ws, &sess(json!({ old.clone(): entry(Some("a"), Some(ago(60_000))) }), json!([])));
    let cap = ago(0);
    let mut hook = |file: &Path| {
        // hook.py rewrote the file between our load and our write: a new capture, and the pruned
        // entry left untouched (it is still pending there).
        let mut v: Value = serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap();
        v["files"][fresh.clone()] = entry(Some("0"), Some(cap.clone()));
        fs::write(file, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    };
    upkeep_with(&fx.paths, &fx.ws, &owned(), T0, &mut hook);
    let s = fx.session(&fx.ws);
    assert_eq!(files_of(&s), vec![fresh.clone()], "the capture survives; the pruned entry stays pruned");
    assert!(!files_of(&s).contains(&same_path_recaptured));
}
