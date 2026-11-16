use crate::excludes::{read_excludes, ExcludeMatcher};
use crate::model::{has_real_change, parse_session, ReviewRecord, Session, Shape};
use crate::paths::{resolve_root, session_file_for};
use crate::roots::read_roots;
use crate::time::{now_ms, parse_iso_ms};
use crate::worktrees::nested_worktrees_under;
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
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub path: String,
    pub rel: String,
    pub scope: String,
    pub new_file: bool,
    pub captured_at: Option<String>,
    pub added: usize,
    pub removed: usize,
    /// No change on disk yet, inside the settle window: the write may still be on its way.
    pub waiting: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordItem {
    pub id: String,
    pub path: String,
    pub rel: String,
    pub scope: String,
    pub decided_at: String,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeView {
    pub root: String,
    pub is_worktree: bool,
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
        added,
        removed,
    }
}

/// The file as the extension's `readFileOrNull` sees it: `None` when it is missing, unreadable
/// or not valid UTF-8 (it decodes with `TextDecoder("utf-8", { fatal: true })`).
pub(crate) fn read_disk(path: &str) -> Option<String> {
    std::fs::read(path).ok().and_then(|b| String::from_utf8(b).ok())
}

fn load_scope(
    home: &Path,
    scope_root: &Path,
    primary: &str,
    matcher: &ExcludeMatcher,
    is_worktree: bool,
    now_ms: i64,
    view: &mut ProjectView,
) {
    let scope_str = scope_root.to_string_lossy().into_owned();
    let mut sv = ScopeView {
        root: scope_str.clone(),
        is_worktree,
        shape_unknown: vec![],
        error: None,
    };
    let file = session_file_for(home, scope_root);
    let session: Option<Session> = match std::fs::read(&file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            sv.error = Some(format!("cannot read session file: {e}"));
            None
        }
        Ok(bytes) => match parse_session(&String::from_utf8_lossy(&bytes)) {
            Ok((s, shape)) => {
                if let Shape::Unknown(f) = shape {
                    sv.shape_unknown = f;
                }
                Some(s)
            }
            Err(e) => {
                sv.error = Some(e.to_string());
                None
            }
        },
    };
    if let Some(s) = session {
        for (path, e) in &s.files {
            if matcher.is_excluded(path) {
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
                added,
                removed,
                waiting,
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

/// The project's scopes: the primary root first, then every nested worktree (sorted).
/// `primary` must already be resolved (`resolve_root`).
pub(crate) fn project_scopes(primary: &Path) -> Vec<PathBuf> {
    let mut scopes = vec![primary.to_path_buf()];
    scopes.extend(nested_worktrees_under(primary));
    scopes
}

/// The exclude matcher for a project: the primary root's globs, anchored at the primary root
/// for every scope (worktrees included), as the extension does.
pub(crate) fn project_matcher(home: &Path, primary: &Path) -> ExcludeMatcher {
    let key = primary.to_string_lossy();
    let globs = read_excludes(home).get(key.as_ref()).cloned().unwrap_or_default();
    ExcludeMatcher::new(primary, &globs)
}

pub fn load_project(home: &Path, root: &Path) -> ProjectView {
    load_project_at(home, root, now_ms())
}

/// `load_project` with the clock injected.
pub fn load_project_at(home: &Path, root: &Path, now_ms: i64) -> ProjectView {
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
    };
    if view.missing {
        return view;
    }
    let matcher = project_matcher(home, &primary_path);
    for (i, scope) in project_scopes(&primary_path).iter().enumerate() {
        load_scope(home, scope, &primary, &matcher, i > 0, now_ms, &mut view);
    }
    view.pending.sort_by(|a, b| a.path.cmp(&b.path));
    view.accepted.sort_by(|a, b| b.decided_at.cmp(&a.decided_at));
    view.rejected.sort_by(|a, b| a.path.cmp(&b.path));
    view
}

pub fn list_projects(home: &Path) -> Vec<ProjectSummary> {
    read_roots(home)
        .into_iter()
        .map(|r: PathBuf| {
            let v = load_project(home, &r);
            let name = r
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| v.root.clone());
            ProjectSummary {
                root: v.root.clone(),
                name,
                missing: v.missing,
                pending: v.pending.len(),
                next_recheck_at: v.next_recheck_at,
            }
        })
        .collect()
}
