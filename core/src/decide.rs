//! Decisions across a project's scopes: a port of the extension's `sessionManager.ts` (v1.16.1)
//! `acceptFile/Folder/All`, `applyReject` + `rejectFile/Folder/All`, `revertAccepted*`,
//! `reapply*` and `clearAccepted/Rejected`, each persisted with the same `MergeGuards`.
//!
//! Every target scope's session is loaded and checked before any workspace file is touched: the
//! read-only switch, an Unknown shape or an unreadable session in any target scope refuses the
//! whole action. Inside a scope the TS order is kept: per file, read `after`, write the
//! workspace file, record; then one `persist` for the scope. Workspace files are written only
//! through `workspace_write`.
//!
//! Two guards the TS does not have keep a failed session write from losing anyone's text:
//! before a scope's first write of any kind, `store::preflight` makes the checks `persist` would
//! make and takes the session backup (a failure refuses the scope with nothing changed), and
//! every workspace file is copied raw into a `FileSnapshots` stamp dir just before it is
//! restored, deleted or overwritten (a failed copy refuses that file).

use crate::excludes::ExcludeMatcher;
use crate::merge::{accept_entry, reject_entry, MergeGuards};
use crate::model::{has_real_change, FileEntry, Session};
use crate::paths::{resolve_root, session_file_for};
use crate::project::{claims_of, project_matcher, read_disk, read_scope, review_scopes, shadow_for, Claims, ScopeRead, Shadow};
use crate::store::{persist, preflight, FileSnapshots, StoreError, StorePaths};
use crate::workspace_write;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR};

/// Spec, "Protecting production" rule 1.
pub const UNKNOWN_SHAPE_MESSAGE: &str =
    "This project was written by a newer ClaudeGate. Update the app to make decisions here.";
/// A key the user confirmed that is no longer eligible when the action runs.
pub const CHANGED_SINCE_CONFIRM_MESSAGE: &str = "Changed since you confirmed, so it was left alone";
/// Reapply of a path that is pending again.
pub const REEDITED_MESSAGE: &str = "Claude has edited this file again — review the new change first";
pub const READ_ONLY_MESSAGE: &str = "ClaudeGate Desktop is in read-only mode. Turn it off to make decisions.";

#[derive(Debug, Clone)]
pub struct DecidePaths {
    /// The ClaudeGate home (`~/.claudegate`).
    pub home: PathBuf,
    /// The app's own home (`~/.claudegate-desktop`); backups go under it.
    pub desktop_home: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    Reject,
    Revert,
    Reapply,
    ClearAccepted,
    ClearRejected,
}

/// `scope` is a scope root as `ProjectView` reports it. `key` is a path for Accept, Reject,
/// Reapply and ClearRejected, and a record id for Revert and ClearAccepted. `prefix` is a
/// folder path; it matches paths under `prefix + "/"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    File { scope: String, key: String },
    Folder { scope: String, prefix: String },
    All,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecideReport {
    pub done: usize,
    /// Rejected files left on disk: a null baseline not known to be Claude's new file.
    pub left: Vec<String>,
    /// `(path, message)` of files whose workspace write failed; they were not decided.
    pub errors: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum DecideError {
    ReadOnly(String),
    NotFound,
    /// The target cannot be acted on safely, e.g. a folder prefix that is not strictly inside
    /// the scope root. Nothing was changed.
    BadTarget(String),
    /// A scope's session write, or its preflight, failed. `changed` lists the workspace files
    /// already restored, deleted or rewritten in that scope, which its review log therefore does
    /// not reflect (empty when the preflight refused: nothing was changed). `snapshot` is the dir
    /// holding those files' bytes from just before they were written. `partial` aggregates the
    /// scopes that completed before it (persisted, or with nothing to persist); scopes after it
    /// were not touched.
    Store { error: StoreError, changed: Vec<String>, partial: DecideReport, snapshot: Option<String> },
}

impl std::fmt::Display for DecideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecideError::ReadOnly(msg) => f.write_str(msg),
            DecideError::NotFound => f.write_str("nothing to decide: the file or record is no longer there"),
            DecideError::BadTarget(msg) => f.write_str(msg),
            DecideError::Store { error, changed, partial, .. } => {
                write!(f, "{error}")?;
                if partial.done > 0 || !partial.left.is_empty() || !partial.errors.is_empty() {
                    write!(
                        f,
                        ". Earlier scopes completed: {} file(s) decided, {} left on disk, {} error(s)",
                        partial.done,
                        partial.left.len(),
                        partial.errors.len()
                    )?;
                }
                if !changed.is_empty() {
                    write!(
                        f,
                        ". These files were changed on disk but the review log was not updated: {}",
                        changed.join(", ")
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for DecideError {}

pub fn decide(
    paths: &DecidePaths,
    root: &Path,
    action: Action,
    target: Target,
    read_only: bool,
    now_iso: &str,
) -> Result<DecideReport, DecideError> {
    decide_with(paths, root, action, target, read_only, now_iso, &mut |_| {})
}

/// `decide` limited to the keys the user confirmed (paths; record ids for Revert and
/// ClearAccepted). For a folder or all target it acts only on keys that are both confirmed and
/// still eligible, and reports each confirmed key that no longer is. Never acts on a key the user
/// was not shown, such as a capture that landed after the count. Ignored for a file target.
pub fn decide_confirmed(
    paths: &DecidePaths,
    root: &Path,
    action: Action,
    target: Target,
    keys: Option<&[String]>,
    read_only: bool,
    now_iso: &str,
) -> Result<DecideReport, DecideError> {
    let hooks = Hooks { before_preflight: &mut || {}, before_persist: &mut |_| {} };
    run(paths, root, action, target, keys.map(Confirm::Bare), read_only, now_iso, hooks)
}

/// `decide_confirmed` with each confirmed key paired with the scope root its row was listed in:
/// the same path can be a row in two scopes (a rejected record in a parent and a nested root), and
/// confirming one must never act on the other.
pub fn decide_confirmed_scoped(
    paths: &DecidePaths,
    root: &Path,
    action: Action,
    target: Target,
    keys: &[(String, String)],
    read_only: bool,
    now_iso: &str,
) -> Result<DecideReport, DecideError> {
    let hooks = Hooks { before_preflight: &mut || {}, before_persist: &mut |_| {} };
    run(paths, root, action, target, Some(Confirm::Scoped(keys)), read_only, now_iso, hooks)
}

/// The keys the user confirmed: bare keys, or `(scope root, key)` pairs.
#[derive(Clone, Copy)]
enum Confirm<'a> {
    Bare(&'a [String]),
    Scoped(&'a [(String, String)]),
}

/// How a confirmed key is held: the key, or `scope \0 key`.
fn confirm_id(scope: Option<&str>, key: &str) -> String {
    match scope {
        Some(s) => format!("{s}\0{key}"),
        None => key.to_owned(),
    }
}

/// `decide` with a hook run just before each scope's `persist`, given the session file. Tests use
/// it to write the disk copy between our load and our write, as the hook or VS Code can.
#[doc(hidden)]
pub fn decide_with(
    paths: &DecidePaths,
    root: &Path,
    action: Action,
    target: Target,
    read_only: bool,
    now_iso: &str,
    before_persist: &mut dyn FnMut(&Path),
) -> Result<DecideReport, DecideError> {
    run(paths, root, action, target, None, read_only, now_iso, Hooks { before_preflight: &mut || {}, before_persist })
}

/// `decide` with a hook run just before each scope's preflight (after the load). Tests use it to
/// change the disk copy in the window the preflight exists to close.
#[doc(hidden)]
pub fn decide_with_preflight_hook(
    paths: &DecidePaths,
    root: &Path,
    action: Action,
    target: Target,
    now_iso: &str,
    before_preflight: &mut dyn FnMut(),
) -> Result<DecideReport, DecideError> {
    run(paths, root, action, target, None, false, now_iso, Hooks { before_preflight, before_persist: &mut |_| {} })
}

struct Hooks<'a> {
    before_preflight: &'a mut dyn FnMut(),
    before_persist: &'a mut dyn FnMut(&Path),
}

/// One scope's writes: the preflight, taken once before the first write of any kind, and the
/// raw snapshots of workspace files about to be overwritten or deleted.
struct Writes<'a> {
    store: StorePaths,
    preflighted: bool,
    snaps: Option<FileSnapshots>,
    hook: &'a mut dyn FnMut(),
}

impl Writes<'_> {
    fn preflight(&mut self) -> Result<(), DecideError> {
        if !self.preflighted {
            (self.hook)();
            preflight(&self.store).map_err(|error| DecideError::Store {
                error,
                changed: Vec::new(),
                partial: DecideReport::default(),
                snapshot: None,
            })?;
            self.preflighted = true;
        }
        Ok(())
    }

    /// Copy `fp`'s current bytes into this decision's stamp dir before it is written.
    fn save(&mut self, scope: &Path, fp: &str) -> Result<(), String> {
        let rel = snapshot_rel(scope, fp).ok_or_else(|| format!("{fp} is not a plain absolute path"))?;
        if self.snaps.is_none() {
            let snaps = FileSnapshots::new(&self.store.desktop_home, &self.store.session_file).map_err(|e| e.to_string())?;
            self.snaps = Some(snaps);
        }
        let snaps = self.snaps.as_mut().expect("just set");
        snaps.save(&rel, Path::new(fp)).map(|_| ()).map_err(|e| e.to_string())
    }

    fn snapshot_dir(&self) -> Option<String> {
        self.snaps.as_ref().and_then(|s| s.dir()).map(|d| d.to_string_lossy().into_owned())
    }
}

/// Where a file's snapshot goes inside a stamp dir: its path relative to the scope root, or
/// `_outside/<its absolute path>` for a key outside the root. `None` for a key with `.`/`..`
/// segments or no name, which could not be placed safely.
fn snapshot_rel(scope: &Path, key: &str) -> Option<PathBuf> {
    let p = Path::new(key);
    if !p.is_absolute() {
        return None;
    }
    let rel = match p.strip_prefix(scope) {
        Ok(r) => r.to_path_buf(),
        Err(_) => {
            let mut out = PathBuf::from("_outside");
            for c in p.components() {
                match c {
                    Component::Prefix(px) => out.push(px.as_os_str().to_string_lossy().replace([':', '\\', '?'], "")),
                    Component::RootDir => {}
                    Component::Normal(n) => out.push(n),
                    Component::CurDir | Component::ParentDir => return None,
                }
            }
            out
        }
    };
    let plain = !rel.as_os_str().is_empty() && rel.components().all(|c| matches!(c, Component::Normal(_)));
    plain.then_some(rel)
}

#[allow(clippy::too_many_arguments)]
fn run(
    paths: &DecidePaths,
    root: &Path,
    action: Action,
    target: Target,
    keys: Option<Confirm>,
    read_only: bool,
    now_iso: &str,
    hooks: Hooks,
) -> Result<DecideReport, DecideError> {
    let Hooks { before_preflight, before_persist } = hooks;
    if read_only {
        return Err(DecideError::ReadOnly(READ_ONLY_MESSAGE.to_owned()));
    }
    let primary = resolve_root(root);
    if !primary.is_dir() {
        return Err(DecideError::NotFound);
    }
    // The primary, its worktrees, and the registered roots nested inside it (each with its own
    // session file and exclude), as the project view lists them.
    let scopes = review_scopes(&paths.home, &primary, crate::worktrees::MAX_WORKTREES);
    // Every scope's session is read once, before anything is written: the targets' for the
    // decision, every scope's for what it claims from a less specific one.
    let reads: Vec<ScopeRead> = scopes.iter().map(|s| read_scope(&paths.home, &s.root)).collect();
    // Each scope is filtered by the exclude of the root that owns it.
    let mut matchers: Vec<(PathBuf, ExcludeMatcher)> = Vec::new();
    for s in &scopes {
        if !matchers.iter().any(|(o, _)| *o == s.owner) {
            matchers.push((s.owner.clone(), project_matcher(&paths.home, &s.owner)));
        }
    }
    let matcher_of = |owner: &Path| &matchers.iter().find(|(o, _)| *o == owner).expect("built above").1;
    let now_ms = crate::time::parse_iso_ms(now_iso).unwrap_or_else(crate::time::now_ms);
    let claims: Vec<Claims> = scopes
        .iter()
        .zip(&reads)
        .map(|(s, r)| claims_of(r.session.as_ref(), matcher_of(&s.owner), now_ms))
        .collect();
    // Every path pending in any scope, with the scope (a reapply never writes over one), and the
    // paths with a row the user can see, with its scope (a revert never opens a second row).
    let mut pending_map: HashMap<String, Vec<EntryAt>> = HashMap::new();
    let mut visible_map: HashMap<String, String> = HashMap::new();
    for (k, (sc, r)) in scopes.iter().zip(&reads).enumerate() {
        let Some(sess) = r.session.as_ref() else { continue };
        let root = sc.root.to_string_lossy().into_owned();
        let shadow = shadow_for(&scopes, &claims, k);
        for (path, e) in &sess.files {
            let claimed = claims[k].pending().contains(path);
            pending_map.entry(path.clone()).or_default().push(EntryAt { scope: root.clone(), owner: sc.owner.clone(), claimed });
            if claims[k].pending().contains(path) && !shadow.hides(path, e) {
                visible_map.insert(path.clone(), root.clone());
            }
        }
    }
    let pending_at = Pending { all: RefCell::new(pending_map), visible: RefCell::new(visible_map) };
    let target_ix: Vec<usize> = match &target {
        Target::All => (0..scopes.len()).collect(),
        Target::File { scope, .. } | Target::Folder { scope, .. } => {
            vec![scopes.iter().position(|s| s.root.to_string_lossy() == scope.as_str()).ok_or(DecideError::NotFound)?]
        }
    };
    if let Target::Folder { prefix, .. } = &target {
        check_prefix(prefix, &scopes[target_ix[0]].root)?;
    }

    // Check every target scope before anything is written.
    let mut loaded: Vec<(usize, PathBuf, Session)> = Vec::new();
    for i in target_ix {
        let scope = &scopes[i].root;
        if let Some(session) = checked(scope, &reads[i])? {
            loaded.push((i, session_file_for(&paths.home, scope), session));
        }
    }
    if matches!(target, Target::File { .. }) && loaded.is_empty() {
        return Err(DecideError::NotFound);
    }

    // A file target names its key; confirmed keys only narrow folder and all targets.
    let ids: Option<Vec<String>> = keys.map(|k| match k {
        Confirm::Bare(k) => k.to_vec(),
        Confirm::Scoped(k) => k.iter().map(|(s, key)| confirm_id(Some(s), key)).collect(),
    });
    let scoped = matches!(keys, Some(Confirm::Scoped(_)));
    let confirmed: Option<HashSet<String>> =
        ids.as_ref().filter(|_| !matches!(target, Target::File { .. })).map(|k| k.iter().cloned().collect());
    let seen = RefCell::new(HashSet::new());
    // `report` holds only completed scopes; each scope works on its own and is folded in once
    // it is persisted (or had nothing to persist).
    let mut report = DecideReport::default();
    for (i, file, mut session) in loaded {
        let scope = scopes[i].root.clone();
        let matcher = matcher_of(&scopes[i].owner);
        let shadow = shadow_for(&scopes, &claims, i);
        let mut guards = MergeGuards::default();
        let mut changed = Vec::new();
        let mut scope_report = DecideReport::default();
        let ctx = Ctx { scope: &scope, owner: &scopes[i].owner, matcher, shadow: &shadow, pending_at: &pending_at, now_iso, confirmed: confirmed.as_ref(), scoped, seen: &seen };
        let store = StorePaths { home: paths.home.clone(), desktop_home: paths.desktop_home.clone(), session_file: file };
        let mut w = Writes { store, preflighted: false, snaps: None, hook: &mut *before_preflight };
        let r = &mut scope_report;
        let outcome = match action {
            Action::Accept => accept(&ctx, &target, &mut session, &mut guards, r),
            Action::Reject => reject(&ctx, &target, &mut session, r, &mut changed, &mut w),
            Action::Revert => revert(&ctx, &target, &mut session, &mut guards, r),
            Action::Reapply => reapply(&ctx, &target, &mut session, &mut guards, r, &mut changed, &mut w),
            Action::ClearAccepted => clear_accepted(&ctx, &target, &mut session, &mut guards, r),
            Action::ClearRejected => clear_rejected(&ctx, &target, &mut session, &mut guards, r),
        };
        // A refused preflight inside the action: nothing in this scope was written.
        let refused = |error| DecideError::Store { error, changed: changed.clone(), partial: report.clone(), snapshot: None };
        let mutated = match outcome {
            Ok(m) => m,
            Err(DecideError::Store { error, .. }) => return Err(refused(error)),
            Err(e) => return Err(e),
        };
        // The TS returns early when nothing was decided; a scope with no change is not written.
        if mutated {
            if let Err(DecideError::Store { error, .. }) = w.preflight() {
                return Err(refused(error));
            }
            before_persist(&w.store.session_file);
            if let Err(error) = persist(&w.store, session, &guards, false) {
                let snapshot = w.snapshot_dir();
                return Err(DecideError::Store { error, changed, partial: report, snapshot });
            }
        }
        report.done += scope_report.done;
        report.left.extend(scope_report.left);
        report.errors.extend(scope_report.errors);
    }
    if let (Some(keys), Some(_)) = (&ids, &confirmed) {
        let seen = seen.borrow();
        for k in keys.iter().filter(|k| !seen.contains(*k)) {
            let k = if scoped { k.split_once('\0').map_or(k.as_str(), |(_, key)| key) } else { k.as_str() };
            // A record id is `<decidedAt>::<path>`; report the path.
            let path = k.split_once("::").map_or(k, |(_, p)| p);
            report.errors.push((path.to_owned(), CHANGED_SINCE_CONFIRM_MESSAGE.to_owned()));
        }
    }
    Ok(report)
}

/// A folder prefix must be absolute, free of `.`/`..` segments, and strictly inside the scope
/// root. Otherwise `"/"` or `""` would act as an All that does not stay inside the scope.
fn check_prefix(prefix: &str, scope: &Path) -> Result<(), DecideError> {
    let p = Path::new(prefix);
    let lexical = p.components().all(|c| matches!(c, Component::RootDir | Component::Prefix(_) | Component::Normal(_)));
    if p.is_absolute() && lexical && is_under(prefix, &scope.to_string_lossy()) {
        Ok(())
    } else {
        Err(DecideError::BadTarget(format!("folder {prefix:?} is not inside {}", scope.display())))
    }
}

/// The scope's session as read, or `None` when it has no session file. Refuses an Unknown shape
/// and a session that cannot be read, parsed or decoded as strict UTF-8: deciding against a log we
/// cannot read could overwrite it (a lossy decode would be written back lossy).
fn checked(scope: &Path, read: &ScopeRead) -> Result<Option<Session>, DecideError> {
    let unreadable =
        |e: &str| DecideError::ReadOnly(format!("The review log for {} could not be read ({e}).", scope.display()));
    if let Some(e) = &read.error {
        return Err(unreadable(e));
    }
    if let Some(e) = &read.not_utf8 {
        return Err(unreadable(e));
    }
    if read.unknown {
        return Err(DecideError::ReadOnly(UNKNOWN_SHAPE_MESSAGE.to_owned()));
    }
    Ok(read.session.clone())
}

/// Where each path is pending across the project's scopes, kept current as a decision reopens one.
/// One scope's pending entry for a path.
#[derive(Debug, Clone)]
struct EntryAt {
    scope: String,
    /// The registered root owning that scope.
    owner: PathBuf,
    /// Its own scope would show it (or hold it as waiting): not excluded there, not a settled
    /// no-op. It may still be hidden by a deeper scope.
    claimed: bool,
}

impl EntryAt {
    /// Hidden only because a scope at or below `owner` claims the path: a pending entry or record
    /// reopened in that scope keeps hiding it, so it never resurfaces.
    fn held_down_by(&self, owner: &Path) -> bool {
        self.claimed && owner != self.owner && owner.starts_with(&self.owner)
    }
}

struct Pending {
    /// Every pending entry of every scope's session, per path.
    all: RefCell<HashMap<String, Vec<EntryAt>>>,
    /// The paths with a row the user sees (not excluded, not a settled no-op, not left to a deeper
    /// scope), with its scope's root.
    visible: RefCell<HashMap<String, String>>,
}

impl Pending {
    fn reopened(&self, path: &str, scope: &str, owner: &Path) {
        let e = EntryAt { scope: scope.to_owned(), owner: owner.to_path_buf(), claimed: true };
        self.all.borrow_mut().entry(path.to_owned()).or_default().push(e);
        self.visible.borrow_mut().insert(path.to_owned(), scope.to_owned());
    }
}

/// Why a revert left a record alone: a newer change to its file has a row in another scope.
pub fn pending_elsewhere_message(scope: &str) -> String {
    format!("Another change to this file is pending in {scope}; review it first, so this accept was kept")
}

/// Why a revert left a record alone: without it, an older change a less specific scope still
/// holds for the file would show again.
pub fn would_reveal_message(scope: &str) -> String {
    format!("Reverting would bring back an older change in {scope}; nothing was changed")
}

/// Why a reapply left a file alone: another scope's log has it pending, but shows no row for it
/// (excluded there, or a capture whose write never changed the file).
pub fn hidden_pending_message(scope: &str) -> String {
    format!("The review log of {scope} still has this file pending (excluded or unchanged there, so not listed), so it was left as is")
}

struct Ctx<'a> {
    scope: &'a Path,
    /// The registered root owning `scope`.
    owner: &'a Path,
    matcher: &'a ExcludeMatcher,
    /// What more specific scopes claim: a folder or all target leaves those entries to them.
    shadow: &'a Shadow<'a>,
    /// Every path pending in any of the project's scopes, kept current as reapply reopens one.
    pending_at: &'a Pending,
    now_iso: &'a str,
    /// The keys the user confirmed, for a folder or all target; `None` means no narrowing.
    confirmed: Option<&'a HashSet<String>>,
    /// `confirmed` holds `scope \0 key` ids.
    scoped: bool,
    /// Confirmed keys found eligible in some scope; the rest are reported at the end.
    seen: &'a RefCell<HashSet<String>>,
}

impl Ctx<'_> {
    /// Whether an eligible `key` of a folder or all target may be acted on; marks it seen.
    fn take(&self, key: &str) -> bool {
        let Some(c) = self.confirmed else { return true };
        let scope = self.scope.to_string_lossy();
        let id = confirm_id(self.scoped.then_some(scope.as_ref()), key);
        let ok = c.contains(&id);
        if ok {
            self.seen.borrow_mut().insert(id);
        }
        ok
    }
}

/// `path` lies strictly inside directory `dir` (`pathIsUnder`: `startsWith(dir + sep)`).
fn is_under(path: &str, dir: &str) -> bool {
    let dir = dir.strip_suffix(MAIN_SEPARATOR).unwrap_or(dir);
    path.strip_prefix(dir).is_some_and(|rest| rest.starts_with(MAIN_SEPARATOR))
}

/// The pending entries a pending-side action (accept, reject) applies to. A single file applies
/// to any pending entry; folder and all skip excluded entries and those with no real change, and
/// all also skips entries outside the scope root (`isInWorkspace`).
fn pending_targets(ctx: &Ctx, target: &Target, s: &Session) -> Result<Vec<String>, DecideError> {
    let scope = ctx.scope.to_string_lossy();
    let keep = |fp: &str, e: &FileEntry| {
        !ctx.matcher.is_excluded(fp) && !ctx.shadow.hides(fp, e) && has_real_change(e.original_content.as_deref(), read_disk(fp).as_deref())
    };
    Ok(match target {
        Target::File { key, .. } => {
            if !s.files.contains_key(key) {
                return Err(DecideError::NotFound);
            }
            vec![key.clone()]
        }
        Target::Folder { prefix, .. } => s
            .files
            .iter()
            .filter(|(fp, e)| is_under(fp, prefix) && keep(fp, e) && ctx.take(fp))
            .map(|(fp, _)| fp.clone())
            .collect(),
        Target::All => s
            .files
            .iter()
            .filter(|(fp, e)| is_under(fp, &scope) && keep(fp, e) && ctx.take(fp))
            .map(|(fp, _)| fp.clone())
            .collect(),
    })
}

/// `acceptFile` / `acceptFolder` / `acceptAll`.
fn accept(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    guards: &mut MergeGuards,
    report: &mut DecideReport,
) -> Result<bool, DecideError> {
    let paths = pending_targets(ctx, target, s)?;
    for fp in &paths {
        let after = read_disk(fp);
        accept_entry(s, fp, after, ctx.now_iso);
        // acceptEntry drops any prior reject; tombstone it so the disk copy can't restore it.
        guards.dropped_rejected_paths.insert(fp.clone());
        report.done += 1;
    }
    Ok(!paths.is_empty())
}

enum RejectOutcome {
    Restored,
    Deleted,
    Left,
}

/// `applyReject`: restore the baseline, delete a confident new file, or leave the file alone.
/// The choice is `project::reject_outcome`, the same function the UI's reject prompt is built from.
fn apply_reject(s: &Session, fp: &str, entry: &FileEntry) -> std::io::Result<RejectOutcome> {
    use crate::project::RejectOutcome as Planned;
    match crate::project::reject_outcome(entry.original_content.as_deref(), entry.new_file) {
        Planned::Restore => {
            // Restore is only planned for a baseline; never write an empty one in its place.
            let Some(original) = entry.original_content.as_deref() else { return Ok(RejectOutcome::Left) };
            workspace_write::restore(s, Path::new(fp), original)?;
            Ok(RejectOutcome::Restored)
        }
        Planned::Delete => {
            workspace_write::delete(s, Path::new(fp))?;
            Ok(RejectOutcome::Deleted)
        }
        Planned::Leave => Ok(RejectOutcome::Left),
    }
}

/// `rejectFile` / `rejectFolder` / `rejectAll`. Persisted with no guards, as the TS does.
/// Before a file is restored or deleted, the scope is preflighted and the file's bytes (Claude's
/// version, whatever its encoding) are saved; a file that cannot be saved is left alone.
fn reject(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    report: &mut DecideReport,
    changed: &mut Vec<String>,
    w: &mut Writes,
) -> Result<bool, DecideError> {
    let mut mutated = false;
    for fp in pending_targets(ctx, target, s)? {
        let Some(entry) = s.files.get(&fp).cloned() else { continue };
        let after = read_disk(&fp); // Claude's discarded version
        if entry.original_content.is_some() || entry.new_file == Some(true) {
            w.preflight()?;
            if let Err(e) = w.save(ctx.scope, &fp) {
                report.errors.push((fp, format!("Couldn't save Claude's version before rejecting, so the file was left as is: {e}")));
                continue;
            }
        }
        match apply_reject(s, &fp, &entry) {
            Err(e) => {
                report.errors.push((fp, e.to_string()));
                continue;
            }
            Ok(RejectOutcome::Left) => report.left.push(fp.clone()),
            Ok(RejectOutcome::Restored | RejectOutcome::Deleted) => changed.push(fp.clone()),
        }
        reject_entry(s, &fp, after, ctx.now_iso);
        report.done += 1;
        mutated = true;
    }
    Ok(mutated)
}

/// A pending entry reopened from a record, captured now (`revertAcceptedRecord`,
/// `reapplyRejectedRecord`).
fn reopened(before: Option<String>, new_file: Option<bool>, session_id: Option<String>, now_iso: &str) -> FileEntry {
    FileEntry { original_content: before, new_file, session_id, captured_at: Some(now_iso.to_owned()) }
}

/// `revertAccepted` / `revertAcceptedFolder` / `revertAcceptedAll`: drop the record and reopen it
/// as pending only when nothing is pending for the path and the disk still holds its `after`.
/// Never writes a workspace file.
fn revert(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    guards: &mut MergeGuards,
    report: &mut DecideReport,
) -> Result<bool, DecideError> {
    let recs: Vec<_> = match target {
        Target::File { key, .. } => {
            vec![s.accepted.iter().find(|r| &r.id == key).cloned().ok_or(DecideError::NotFound)?]
        }
        Target::Folder { prefix, .. } => {
            s.accepted.iter().filter(|r| is_under(&r.path, prefix) && ctx.take(&r.id)).cloned().collect()
        }
        Target::All => s.accepted.iter().filter(|r| ctx.take(&r.id)).cloned().collect(),
    };
    let here = ctx.scope.to_string_lossy().into_owned();
    let mut mutated = false;
    for rec in &recs {
        // A newer change to the file has a row in another scope: dropping this record would lose
        // it with nothing reopened, so it is kept and reported.
        let elsewhere = ctx.pending_at.visible.borrow().get(&rec.path).filter(|sc| **sc != here).cloned();
        if let Some(sc) = elsewhere {
            report.errors.push((rec.path.clone(), pending_elsewhere_message(&sc)));
            continue;
        }
        // Pending in this scope already: the TS keeps that entry and does not reopen. An entry
        // elsewhere with no row (excluded, a no-op, left to this scope) does not block it.
        let reopen = !s.files.contains_key(&rec.path) && read_disk(&rec.path) == rec.after;
        if !reopen && !s.files.contains_key(&rec.path) {
            // Nothing will claim the path here any more: a less specific scope's entry that this
            // record hides would come back. Keep the record and say so.
            let revealed = ctx.pending_at.all.borrow().get(&rec.path).and_then(|v| {
                v.iter().find(|e| e.scope != here && e.held_down_by(ctx.owner)).map(|e| e.scope.clone())
            });
            if let Some(sc) = revealed {
                report.errors.push((rec.path.clone(), would_reveal_message(&sc)));
                continue;
            }
        }
        if let Some(i) = s.accepted.iter().position(|r| r.id == rec.id) {
            s.accepted.remove(i);
        }
        if reopen {
            let entry = reopened(rec.before.clone(), rec.new_file, rec.session_id.clone(), ctx.now_iso);
            s.files.insert(rec.path.clone(), entry);
            ctx.pending_at.reopened(&rec.path, &here, ctx.owner);
        }
        guards.dropped_accepted_ids.insert(rec.id.clone());
        report.done += 1;
        mutated = true;
    }
    Ok(mutated)
}

/// `reapplyRejected` / `reapplyFolder` / `reapplyAll`: write the record's `after` back and reopen
/// the entry as pending. Folder and all skip excluded paths. Unlike the TS, a path that is pending
/// again (Claude re-edited it) is refused and reported, never overwritten.
fn reapply(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    guards: &mut MergeGuards,
    report: &mut DecideReport,
    changed: &mut Vec<String>,
    w: &mut Writes,
) -> Result<bool, DecideError> {
    let paths: Vec<String> = match target {
        Target::File { key, .. } => {
            if !s.rejected.contains_key(key) {
                return Err(DecideError::NotFound);
            }
            vec![key.clone()]
        }
        Target::Folder { prefix, .. } => s
            .rejected
            .keys()
            .filter(|fp| is_under(fp, prefix) && !ctx.matcher.is_excluded(fp) && ctx.take(fp))
            .cloned()
            .collect(),
        Target::All => s.rejected.keys().filter(|fp| !ctx.matcher.is_excluded(fp) && ctx.take(fp)).cloned().collect(),
    };
    let mut mutated = false;
    for fp in paths {
        let Some(rec) = s.rejected.get(&fp).cloned() else { continue };
        // Claude edited the file again since the reject: writing the old `after` would destroy
        // that edit and replace its frozen baseline. The new change is reviewed first.
        // So is a path pending in any other scope (the nested root's session, say): writing this
        // scope's old `after` would destroy Claude's current version there.
        let here = ctx.scope.to_string_lossy().into_owned();
        let shown_in = ctx.pending_at.visible.borrow().get(&fp).cloned();
        if s.files.contains_key(&fp) {
            // Pending here: with a row, Claude edited it again; without one, say so.
            let msg = if shown_in.as_deref() == Some(here.as_str()) { REEDITED_MESSAGE.to_owned() } else { hidden_pending_message(&here) };
            report.errors.push((fp, msg));
            continue;
        }
        if shown_in.is_some() {
            report.errors.push((fp, REEDITED_MESSAGE.to_owned()));
            continue;
        }
        // An entry elsewhere with no row blocks too (excluded or unchanged there), unless only a
        // scope at or below this one hides it: the entry reopened here keeps hiding it.
        let blocker = ctx.pending_at.all.borrow().get(&fp).and_then(|v| {
            v.iter().find(|e| e.scope != here && !e.held_down_by(ctx.owner)).map(|e| e.scope.clone())
        });
        if let Some(sc) = blocker {
            report.errors.push((fp, hidden_pending_message(&sc)));
            continue;
        }
        let Some(after) = rec.after.as_deref() else {
            let name = Path::new(&fp).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            report.errors.push((fp, format!("Claude's version of \"{name}\" was not saved")));
            continue;
        };
        w.preflight()?;
        if let Err(e) = w.save(ctx.scope, &fp) {
            report.errors.push((fp, format!("Couldn't save the file on disk before reapplying, so it was left as is: {e}")));
            continue;
        }
        if let Err(e) = workspace_write::restore(s, Path::new(&fp), after) {
            report.errors.push((fp, e.to_string()));
            continue;
        }
        changed.push(fp.clone());
        s.rejected.remove(&fp);
        ctx.pending_at.reopened(&fp, &ctx.scope.to_string_lossy(), ctx.owner);
        s.files.insert(fp.clone(), reopened(rec.before, rec.new_file, rec.session_id, ctx.now_iso));
        guards.dropped_rejected_paths.insert(fp);
        report.done += 1;
        mutated = true;
    }
    Ok(mutated)
}

/// `clearAccepted`, which clears every record; a file or folder target narrows it to one id or
/// to the records under a folder.
fn clear_accepted(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    guards: &mut MergeGuards,
    report: &mut DecideReport,
) -> Result<bool, DecideError> {
    let ids: Vec<String> = s
        .accepted
        .iter()
        .filter(|r| match target {
            Target::File { key, .. } => &r.id == key,
            Target::Folder { prefix, .. } => is_under(&r.path, prefix) && ctx.take(&r.id),
            Target::All => ctx.take(&r.id),
        })
        .map(|r| r.id.clone())
        .collect();
    if ids.is_empty() && matches!(target, Target::File { .. }) {
        return Err(DecideError::NotFound);
    }
    s.accepted.retain(|r| !ids.contains(&r.id));
    report.done += ids.len();
    let mutated = !ids.is_empty();
    // Tombstone every cleared id so the merge can't resurrect them from the disk copy.
    guards.dropped_accepted_ids.extend(ids);
    Ok(mutated)
}

/// `clearRejected`, narrowed the same way as `clear_accepted` (a file target is a path).
fn clear_rejected(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    guards: &mut MergeGuards,
    report: &mut DecideReport,
) -> Result<bool, DecideError> {
    let paths: Vec<String> = s
        .rejected
        .keys()
        .filter(|fp| match target {
            Target::File { key, .. } => *fp == key,
            Target::Folder { prefix, .. } => is_under(fp, prefix) && ctx.take(fp),
            Target::All => ctx.take(fp),
        })
        .cloned()
        .collect();
    if paths.is_empty() && matches!(target, Target::File { .. }) {
        return Err(DecideError::NotFound);
    }
    for fp in &paths {
        s.rejected.remove(fp);
    }
    report.done += paths.len();
    let mutated = !paths.is_empty();
    guards.dropped_rejected_paths.extend(paths);
    Ok(mutated)
}

#[cfg(test)]
mod tests {
    use super::{is_under, snapshot_rel};
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    #[test]
    fn snapshot_rel_is_relative_to_the_scope_or_under_outside() {
        let ws = Path::new("/ws");
        assert_eq!(snapshot_rel(ws, "/ws/src/a.ts"), Some(PathBuf::from("src/a.ts")));
        assert_eq!(snapshot_rel(ws, "/elsewhere/q.txt"), Some(PathBuf::from("_outside/elsewhere/q.txt")));
        assert_eq!(snapshot_rel(ws, "/ws/../etc/passwd"), None);
        assert_eq!(snapshot_rel(ws, "/ws"), None);
        assert_eq!(snapshot_rel(ws, "rel.txt"), None);
    }

    #[test]
    fn is_under_needs_a_separator_after_the_dir() {
        assert!(is_under("/ws/src/a.ts", "/ws/src"));
        assert!(is_under("/ws/src/a.ts", "/ws/src/"));
        assert!(!is_under("/ws/srcx/a.ts", "/ws/src"));
        assert!(!is_under("/ws/src", "/ws/src"));
    }
}
