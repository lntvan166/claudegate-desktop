use crate::excludes::{read_excludes, ExcludeMatcher};
use crate::model::{has_real_change, parse_session, FileEntry, ReviewRecord, Session, Shape};
use crate::paths::{resolve_root, session_file_for};
use crate::roots::read_roots;
use crate::time::{now_ms, parse_iso_ms};
use crate::worktrees::{nested_worktrees_under_skip, MAX_WORKTREES};
use serde::Serialize;
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub root: String,
    pub name: String,
    pub missing: bool,
    pub pending: usize,
    /// See `ProjectView::next_recheck_at`.
    pub next_recheck_at: Option<i64>,
    /// The registered roots folded into this project as scopes (`ScopeKind::Nested`).
    pub nested: Vec<String>,
    /// Epoch ms of the newest `capturedAt` among the counted pending rows that are not waiting
    /// (`newest_capture_ms`); `None` when none has one. A hidden project with a capture newer than
    /// when it was hidden comes back on its own, so a capture whose write has not landed (and may
    /// settle as a no-op) never counts.
    pub newest_capture_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub path: String,
    pub rel: String,
    pub scope: String,
    pub new_file: bool,
    pub captured_at: Option<String>,
    /// The Claude session that captured it; `None` when the hook had none (group "Unknown session").
    pub session_id: Option<String>,
    pub added: usize,
    pub removed: usize,
    /// No change on disk yet, inside the settle window: the write may still be on its way.
    pub waiting: bool,
    /// What rejecting this file does to it on disk; the reject prompts count and name these.
    pub reject_outcome: RejectOutcome,
}

/// What a reject does to a pending file: restore its baseline, delete the file Claude created, or
/// leave it alone (a null baseline not known to be Claude's new file). `decide::apply_reject` acts
/// on exactly this, so the prompt can never promise something the reject won't do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RejectOutcome {
    Restore,
    Delete,
    Leave,
}

pub fn reject_outcome(original: Option<&str>, new_file: Option<bool>) -> RejectOutcome {
    match (original, new_file) {
        (Some(_), _) => RejectOutcome::Restore,
        (None, Some(true)) => RejectOutcome::Delete,
        (None, _) => RejectOutcome::Leave,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordItem {
    pub id: String,
    pub path: String,
    pub rel: String,
    pub scope: String,
    pub decided_at: String,
    pub session_id: Option<String>,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeView {
    pub root: String,
    pub is_worktree: bool,
    /// A registered root nested inside the project's root (its own repo, own session, own
    /// exclude), shown here instead of as a project of its own.
    pub nested: bool,
    pub shape_unknown: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub root: String,
    pub missing: bool,
    pub scopes: Vec<ScopeView>,
    pub pending: Vec<PendingItem>,
    pub accepted: Vec<RecordItem>,
    pub rejected: Vec<RecordItem>,
    /// Epoch ms of the earliest moment a `waiting` row may settle and disappear; the UI re-fetches
    /// then. `None` when nothing is waiting.
    pub next_recheck_at: Option<i64>,
    /// See `ProjectSummary::newest_capture_ms`, over every pending row (the app narrows it by the
    /// project's app-only hide patterns).
    pub newest_capture_ms: Option<i64>,
}

fn line_counts(before: &str, after: &str) -> (usize, usize) {
    let diff = TextDiff::from_lines(before, after);
    let (mut a, mut r) = (0, 0);
    for c in diff.iter_all_changes() {
        match c.tag() {
            ChangeTag::Insert => a += 1,
            ChangeTag::Delete => r += 1,
            ChangeTag::Equal => {}
        }
    }
    (a, r)
}

fn rel_of(root: &str, path: &str) -> String {
    let p = path.replace('\\', "/");
    let r = root.replace('\\', "/");
    match p.strip_prefix(&format!("{}/", r.trim_end_matches('/'))) {
        Some(rel) => rel.to_string(),
        None => path.to_string(),
    }
}

fn record_item(primary: &str, scope: &str, r: &ReviewRecord) -> RecordItem {
    let (added, removed) = line_counts(
        r.before.as_deref().unwrap_or(""),
        r.after.as_deref().unwrap_or(""),
    );
    RecordItem {
        id: r.id.clone(),
        path: r.path.clone(),
        rel: rel_of(primary, &r.path),
        scope: scope.to_string(),
        decided_at: r.decided_at.clone(),
        session_id: r.session_id.clone(),
        added,
        removed,
    }
}

/// The file as the extension's `readFileOrNull` sees it: `None` when it is missing, unreadable
/// or not valid UTF-8 (it decodes with `TextDecoder("utf-8", { fatal: true })`).
pub(crate) fn read_disk(path: &str) -> Option<String> {
    std::fs::read(path).ok().and_then(|b| String::from_utf8(b).ok())
}

/// One scope's session file, read and parsed once per load or decision.
pub(crate) struct ScopeRead {
    pub session: Option<Session>,
    /// Unknown fields, when the session was written by a newer ClaudeGate.
    pub shape_unknown: Vec<String>,
    /// The shape is Unknown (even with no field named).
    pub unknown: bool,
    /// Why it could not be read or parsed.
    pub error: Option<String>,
    /// Why it is not strict UTF-8 (decide refuses such a session; the view reads it lossy).
    pub not_utf8: Option<String>,
}

/// Reads and parses `scope`'s session file. A missing file is no session and no error.
pub(crate) fn read_scope(home: &Path, scope: &Path) -> ScopeRead {
    let mut out = ScopeRead { session: None, shape_unknown: vec![], unknown: false, error: None, not_utf8: None };
    match std::fs::read(session_file_for(home, scope)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => out.error = Some(format!("cannot read session file: {e}")),
        Ok(bytes) => {
            out.not_utf8 = std::str::from_utf8(&bytes).err().map(|e| e.to_string());
            match parse_session(&String::from_utf8_lossy(&bytes)) {
                Ok((s, shape)) => {
                    if let Shape::Unknown(f) = shape {
                        out.shape_unknown = f;
                        out.unknown = true;
                    }
                    out.session = Some(s);
                }
                Err(e) => out.error = Some(e.to_string()),
            }
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn load_scope(
    scope: &ReviewScope,
    read: &ScopeRead,
    primary: &str,
    matcher: &ExcludeMatcher,
    shadow: &Shadow,
    now_ms: i64,
    view: &mut ProjectView,
) {
    let scope_root = scope.root.as_path();
    let scope_str = scope_root.to_string_lossy().into_owned();
    let sv = ScopeView {
        root: scope_str.clone(),
        is_worktree: scope.kind == ScopeKind::Worktree,
        nested: scope.kind == ScopeKind::Nested,
        shape_unknown: read.shape_unknown.clone(),
        error: read.error.clone(),
    };
    let session = read.session.as_ref();
    if let Some(s) = session {
        for (path, e) in &s.files {
            // Claimed by a more specific (nested) scope: that scope owns the row.
            if matcher.is_excluded(path) || shadow.hides(path, e) {
                continue;
            }
            let disk = read_disk(path);
            let waiting = match pending_state(
                e.original_content.as_deref(),
                disk.as_deref(),
                e.captured_at.as_deref(),
                now_ms,
            ) {
                PendingState::Real => false,
                PendingState::Hidden => continue,
                PendingState::Waiting { recheck_at } => {
                    view.next_recheck_at =
                        Some(view.next_recheck_at.map_or(recheck_at, |n| n.min(recheck_at)));
                    true
                }
            };
            let (added, removed) = line_counts(
                e.original_content.as_deref().unwrap_or(""),
                disk.as_deref().unwrap_or(""),
            );
            view.pending.push(PendingItem {
                path: path.clone(),
                rel: rel_of(primary, path),
                scope: scope_str.clone(),
                new_file: e.new_file.unwrap_or(e.original_content.is_none()),
                captured_at: e.captured_at.clone(),
                session_id: e.session_id.clone(),
                added,
                removed,
                waiting,
                reject_outcome: reject_outcome(e.original_content.as_deref(), e.new_file),
            });
        }
        for r in &s.accepted {
            if !matcher.is_excluded(&r.path) {
                view.accepted.push(record_item(primary, &scope_str, r));
            }
        }
        for r in s.rejected.values() {
            if !matcher.is_excluded(&r.path) {
                view.rejected.push(record_item(primary, &scope_str, r));
            }
        }
    }
    view.scopes.push(sv);
}

/// Extension v1.16.1 `reviewModel.ts`: how long a no-op entry may wait for its write to land.
pub const NOOP_SETTLE_MS: i64 = 15_000;
/// The longer window for a promised new file (`originalContent: null`) that is not on disk yet.
pub const NEW_FILE_ABSENT_MS: i64 = 45_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingState {
    /// The file differs from its baseline.
    Real,
    /// No change yet, but the hook records before Claude writes: keep showing it until
    /// `recheck_at` (epoch ms), the first moment the extension would prune it.
    Waiting { recheck_at: i64 },
    /// A settled no-op: the extension prunes it, so it is not shown.
    Hidden,
}

/// Mirrors the extension's `shouldPruneNoOp`: a no-op is hidden only once it is older than its
/// window, measured from `capturedAt`; a missing or unparsable `capturedAt` is infinitely old.
pub fn pending_state(
    original: Option<&str>,
    disk: Option<&str>,
    captured_at: Option<&str>,
    now_ms: i64,
) -> PendingState {
    if has_real_change(original, disk) {
        return PendingState::Real;
    }
    let settle = if original.is_none() { NEW_FILE_ABSENT_MS } else { NOOP_SETTLE_MS };
    match captured_at.and_then(parse_iso_ms) {
        Some(t) if now_ms - t <= settle => PendingState::Waiting { recheck_at: t + settle + 1 },
        _ => PendingState::Hidden,
    }
}

/// Port of `shouldPruneNoOp`: the entry is a no-op that has out-aged its settle window.
pub fn should_prune_no_op(entry: &FileEntry, disk: Option<&str>, now_ms: i64) -> bool {
    pending_state(entry.original_content.as_deref(), disk, entry.captured_at.as_deref(), now_ms) == PendingState::Hidden
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Primary,
    Worktree,
    /// A registered root strictly inside the primary root.
    Nested,
}

/// One scope a project shows and decides on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewScope {
    pub root: PathBuf,
    pub kind: ScopeKind,
    /// The registered root whose exclude filters this scope: the primary for itself and its
    /// worktrees, a nested root for itself and its worktrees (the hook captures by that list).
    pub owner: PathBuf,
}

/// `inner` lies strictly inside `outer`, by whole path segments of the resolved forms.
fn strictly_inside(inner: &Path, outer: &Path) -> bool {
    inner != outer && inner.starts_with(outer)
}

/// The registered roots strictly inside `primary` (resolved, sorted, without repeats).
pub fn nested_roots_in(roots: &[PathBuf], primary: &Path) -> Vec<PathBuf> {
    let primary = resolve_root(primary);
    let mut out: Vec<PathBuf> =
        roots.iter().map(|r| resolve_root(r)).filter(|r| strictly_inside(r, &primary)).collect();
    out.sort();
    out.dedup();
    out
}

/// The project a registered root is shown in when it is not its own: the outermost registered
/// root strictly containing it that is a folder on disk. `None`: it is listed on its own.
pub fn fold_parent(roots: &[PathBuf], root: &Path, is_dir: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let root = resolve_root(root);
    roots
        .iter()
        .map(|r| resolve_root(r))
        .filter(|r| strictly_inside(&root, r) && is_dir(r))
        .min_by_key(|r| r.components().count())
}

/// The scopes a project shows and decides on, given the registered roots: the primary and its
/// worktrees, then each registered root nested inside it with its own worktrees. Worktrees are
/// found under the root that owns them (a scan never enters a nested root), at most
/// `max_attached` per root; nested roots do not count against that limit.
pub fn review_scopes_in(roots: &[PathBuf], primary: &Path, max_attached: usize) -> Vec<ReviewScope> {
    let primary = resolve_root(primary);
    // A nested root gone from disk stays a (missing) project of its own: see `listed_roots`.
    let nested: Vec<PathBuf> = nested_roots_in(roots, &primary).into_iter().filter(|n| n.is_dir()).collect();
    let mut out = vec![ReviewScope { root: primary.clone(), kind: ScopeKind::Primary, owner: primary.clone() }];
    for wt in nested_worktrees_under_skip(&primary, max_attached, &nested) {
        out.push(ReviewScope { root: wt, kind: ScopeKind::Worktree, owner: primary.clone() });
    }
    for n in &nested {
        out.push(ReviewScope { root: n.clone(), kind: ScopeKind::Nested, owner: n.clone() });
        let inner: Vec<PathBuf> = nested.iter().filter(|m| strictly_inside(m, n)).cloned().collect();
        for wt in nested_worktrees_under_skip(n, max_attached, &inner) {
            out.push(ReviewScope { root: wt, kind: ScopeKind::Worktree, owner: n.clone() });
        }
    }
    out
}

/// What a scope's session claims for each path, so a less specific scope leaves the path to it:
/// the pending entries it shows or holds as waiting (not settled no-ops, not excluded ones), and
/// the newest decision it recorded per path (epoch ms).
#[derive(Debug, Default)]
pub(crate) struct Claims {
    pending: std::collections::HashSet<String>,
    decided: std::collections::HashMap<String, i64>,
}

/// The claims of one scope's session, filtered by that scope's own exclude.
pub(crate) fn claims_of(session: Option<&Session>, matcher: &ExcludeMatcher, now_ms: i64) -> Claims {
    let mut c = Claims::default();
    let Some(s) = session else { return c };
    for (path, e) in &s.files {
        if matcher.is_excluded(path) {
            continue;
        }
        let state = pending_state(e.original_content.as_deref(), read_disk(path).as_deref(), e.captured_at.as_deref(), now_ms);
        if state != PendingState::Hidden {
            c.pending.insert(path.clone());
        }
    }
    let records = s.accepted.iter().chain(s.rejected.values());
    for r in records {
        if let Some(ms) = parse_iso_ms(&r.decided_at) {
            let d = c.decided.entry(r.path.clone()).or_insert(ms);
            *d = (*d).max(ms);
        }
    }
    c
}

/// The claims of the scopes more specific than scope `i`: those whose owning root lies strictly
/// inside scope `i`'s owner (a nested root, or one of its worktrees).
pub(crate) struct Shadow<'a> {
    deeper: Vec<&'a Claims>,
}

impl Shadow<'_> {
    /// Whether a less specific scope leaves its entry `e` for `path` to a deeper scope: one that
    /// has it pending (shown or waiting), or that decided it at or after `e` was captured (an
    /// unreadable `capturedAt` counts as older). Display and decisions only; nothing is pruned.
    pub(crate) fn hides(&self, path: &str, e: &FileEntry) -> bool {
        let captured = e.captured_at.as_deref().and_then(parse_iso_ms);
        self.deeper.iter().any(|c| {
            c.pending.contains(path) || c.decided.get(path).is_some_and(|d| captured.is_none_or(|t| *d >= t))
        })
    }
}

pub(crate) fn shadow_for<'a>(scopes: &[ReviewScope], claims: &'a [Claims], i: usize) -> Shadow<'a> {
    let deeper = scopes
        .iter()
        .enumerate()
        .filter(|(j, s)| *j != i && strictly_inside(&s.owner, &scopes[i].owner))
        .map(|(j, _)| &claims[j])
        .collect();
    Shadow { deeper }
}

/// `review_scopes_in` with the roots from `workspace-roots.json`.
pub fn review_scopes(home: &Path, primary: &Path, max_attached: usize) -> Vec<ReviewScope> {
    review_scopes_in(&read_roots(home), primary, max_attached)
}

/// The exclude matcher for a project: the primary root's globs, anchored at the primary root
/// for every scope (worktrees included), as the extension does. A nested root is a project of
/// its own here: call it with that root.
pub(crate) fn project_matcher(home: &Path, primary: &Path) -> ExcludeMatcher {
    let key = primary.to_string_lossy();
    let globs = read_excludes(home).get(key.as_ref()).cloned().unwrap_or_default();
    ExcludeMatcher::new(primary, &globs)
}

pub fn load_project(home: &Path, root: &Path) -> ProjectView {
    load_project_at(home, root, now_ms())
}

/// `load_project` attaching at most `max_attached` worktrees (`worktrees.maxAttached`).
pub fn load_project_max(home: &Path, root: &Path, max_attached: usize) -> ProjectView {
    load_project_at_max(home, root, now_ms(), max_attached)
}

/// `load_project_max`, filtering with `exclude` instead of `excludes.json[root]` when given: the
/// effective exclude of a root the app owns, which can be ahead of the file until the next sync.
/// Nested scopes are filtered by their own root's `excludes.json` key.
pub fn load_project_with(home: &Path, root: &Path, max_attached: usize, exclude: Option<&[String]>) -> ProjectView {
    let primary = resolve_root(root);
    let own = exclude.map(<[String]>::to_vec);
    load_project_full(home, root, now_ms(), max_attached, &|r| if r == primary { own.clone() } else { None })
}

/// `load_project_max` with `exclude_for(root)` giving the exclude of every registered root it
/// shows (the primary and each nested one): `Some` for a root the app owns, `None` to read
/// `excludes.json[root]`.
pub fn load_project_for(
    home: &Path,
    root: &Path,
    max_attached: usize,
    exclude_for: &dyn Fn(&Path) -> Option<Vec<String>>,
) -> ProjectView {
    load_project_full(home, root, now_ms(), max_attached, exclude_for)
}

/// `load_project` with the clock injected.
pub fn load_project_at(home: &Path, root: &Path, now_ms: i64) -> ProjectView {
    load_project_at_max(home, root, now_ms, MAX_WORKTREES)
}

fn load_project_at_max(home: &Path, root: &Path, now_ms: i64, max_attached: usize) -> ProjectView {
    load_project_full(home, root, now_ms, max_attached, &|_| None)
}

fn load_project_full(
    home: &Path,
    root: &Path,
    now_ms: i64,
    max_attached: usize,
    exclude_for: &dyn Fn(&Path) -> Option<Vec<String>>,
) -> ProjectView {
    let primary_path = resolve_root(root);
    let primary = primary_path.to_string_lossy().into_owned();
    let mut view = ProjectView {
        root: primary.clone(),
        missing: !primary_path.is_dir(),
        scopes: vec![],
        pending: vec![],
        accepted: vec![],
        rejected: vec![],
        next_recheck_at: None,
        newest_capture_ms: None,
    };
    if view.missing {
        return view;
    }
    let scopes = review_scopes(home, &primary_path, max_attached);
    // One matcher per owning root, one read per scope, reused for the claims and the rows.
    let mut matchers: Vec<(PathBuf, ExcludeMatcher)> = Vec::new();
    for scope in &scopes {
        if !matchers.iter().any(|(o, _)| *o == scope.owner) {
            let m = match exclude_for(&scope.owner) {
                Some(globs) => ExcludeMatcher::new(&scope.owner, &globs),
                None => project_matcher(home, &scope.owner),
            };
            matchers.push((scope.owner.clone(), m));
        }
    }
    let matcher_of = |owner: &Path| &matchers.iter().find(|(o, _)| *o == owner).expect("built above").1;
    let reads: Vec<ScopeRead> = scopes.iter().map(|s| read_scope(home, &s.root)).collect();
    let claims: Vec<Claims> = scopes
        .iter()
        .zip(&reads)
        .map(|(s, r)| claims_of(r.session.as_ref(), matcher_of(&s.owner), now_ms))
        .collect();
    for (i, scope) in scopes.iter().enumerate() {
        let shadow = shadow_for(&scopes, &claims, i);
        load_scope(scope, &reads[i], &primary, matcher_of(&scope.owner), &shadow, now_ms, &mut view);
    }
    view.pending.sort_by(|a, b| a.path.cmp(&b.path));
    view.newest_capture_ms = newest_capture_ms(view.pending.iter());
    view.accepted.sort_by(|a, b| b.decided_at.cmp(&a.decided_at));
    view.rejected.sort_by(|a, b| a.path.cmp(&b.path));
    view
}

/// Epoch ms of the newest `capturedAt` among `rows` that are not waiting; `None` when none has a
/// readable one.
pub fn newest_capture_ms<'a>(rows: impl Iterator<Item = &'a PendingItem>) -> Option<i64> {
    rows.filter(|p| !p.waiting).filter_map(|p| p.captured_at.as_deref().and_then(parse_iso_ms)).max()
}

/// The registered roots listed as projects: every root, except one folded into a parent
/// (`fold_parent`), which shows as a scope of that parent instead. A nested root missing from
/// disk is not folded: it stays listed as missing, where it can be seen and removed.
pub fn listed_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let is_dir = |p: &Path| p.is_dir();
    roots.iter().filter(|r| !(is_dir(r) && fold_parent(roots, r, &is_dir).is_some())).cloned().collect()
}

/// The summary of a loaded project, counting the pending rows `counted` keeps. `name` is the
/// folder's name as `root` spells it.
pub fn summary_of(root: &Path, v: &ProjectView, counted: &dyn Fn(&PendingItem) -> bool) -> ProjectSummary {
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| v.root.clone());
    let rows: Vec<&PendingItem> = v.pending.iter().filter(|p| counted(p)).collect();
    ProjectSummary {
        root: v.root.clone(),
        name,
        missing: v.missing,
        pending: rows.len(),
        newest_capture_ms: newest_capture_ms(rows.iter().copied()),
        next_recheck_at: v.next_recheck_at,
        nested: v.scopes.iter().filter(|s| s.nested).map(|s| s.root.clone()).collect(),
    }
}

pub fn list_projects(home: &Path) -> Vec<ProjectSummary> {
    listed_roots(&read_roots(home))
        .into_iter()
        .map(|r: PathBuf| {
            let v = load_project(home, &r);
            summary_of(&r, &v, &|_| true)
        })
        .collect()
}
