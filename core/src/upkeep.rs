//! Session upkeep for the roots the app owns: the housekeeping the extension's `SessionManager`
//! (v1.16.1) runs on every load of its own workspaces, which nothing runs for a root only the app
//! has. Without it a no-op capture is never pruned and the accepted log only grows.
//!
//! Per scope (the primary root, then each nested worktree), on one load:
//! - `pruneOutOfWorkspaceEntries`: pending entries not under the primary root (the window's
//!   workspace folder in the extension, so a worktree session is judged by it too);
//! - `cleanupExcludedOnce`: pending entries matching the project's effective exclude, once, then
//!   `excludeCleanupDone` latches (written even when nothing matched);
//! - `reconcilePending`: settled no-ops (`shouldPruneNoOp`);
//! - `healOversized`: when the file was over `SESSION_SIZE_WARN_BYTES`, `capAcceptedBytes` to
//!   `MAX_ACCEPTED_BYTES`.
//!
//! All of it goes out in one `store::persist`, guarded with `pruned_files` and
//! `dropped_accepted_ids` so the disk copy cannot resurrect what was removed, and only when
//! something changed. So a run right after our own write is a no-op, which is what keeps the
//! watcher from looping. Records (accepted, rejected) are never touched except by the byte cap.
//!
//! Nothing happens unless the root is owned (`addedByApp && !managedByVsCode`) and read-only mode
//! is off. A scope whose session is missing, unreadable or of Unknown shape is left alone.

use crate::decide::DecidePaths;
use crate::excludes::ExcludeMatcher;
use crate::merge::MergeGuards;
use crate::model::{cap_accepted_bytes, parse_session, Session, Shape};
use crate::paths::{resolve_root, session_file_for};
use crate::project::{project_scopes_max, read_disk, should_prune_no_op};
use crate::projects::{owned_exclude, ProjectRecord};
use crate::store::{persist, StorePaths};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

/// The extension's `SESSION_SIZE_WARN_BYTES`: a session file larger than this gets its accepted
/// log trimmed.
pub const SESSION_SIZE_WARN_BYTES: u64 = 2_000_000;
/// The extension's `MAX_ACCEPTED_BYTES`: the byte budget the accepted log is trimmed to.
pub const MAX_ACCEPTED_BYTES: usize = 1_500_000;

#[derive(Debug, Clone)]
pub struct UpkeepOpts {
    /// The app's record of the root (`projects[root]`).
    pub record: ProjectRecord,
    /// The read-only switch; an unreadable config must be passed as `true`.
    pub read_only: bool,
    /// `worktrees.maxAttached` for the root.
    pub max_attached: usize,
}

/// Why a whole run did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Skip {
    /// A root the app did not add, or one VS Code took over: the extension keeps it.
    NotOwned,
    ReadOnly,
    /// The root folder is gone.
    Missing,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeUpkeep {
    pub scope: String,
    pub pruned_no_op: Vec<String>,
    pub pruned_outside: Vec<String>,
    pub pruned_excluded: Vec<String>,
    pub trimmed_accepted: usize,
    /// `excludeCleanupDone` was set by this run.
    pub latched: bool,
    pub written: bool,
    /// Why the scope was left alone (unreadable session, Unknown shape).
    pub skipped: Option<String>,
    /// The write failed; nothing was changed.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpkeepReport {
    pub skipped: Option<Skip>,
    pub scopes: Vec<ScopeUpkeep>,
}

impl UpkeepReport {
    /// Whether any session file was written.
    pub fn wrote(&self) -> bool {
        self.scopes.iter().any(|s| s.written)
    }
}

pub fn upkeep(paths: &DecidePaths, root: &Path, opts: &UpkeepOpts, now_ms: i64) -> UpkeepReport {
    upkeep_with(paths, root, opts, now_ms, &mut |_| {})
}

/// `upkeep` with a hook run just before each scope's `persist`, given the session file. Tests use
/// it to write the disk copy between our load and our write, as the hook can.
#[doc(hidden)]
pub fn upkeep_with(
    paths: &DecidePaths,
    root: &Path,
    opts: &UpkeepOpts,
    now_ms: i64,
    before_persist: &mut dyn FnMut(&Path),
) -> UpkeepReport {
    let mut report = UpkeepReport::default();
    if !opts.record.owned() {
        report.skipped = Some(Skip::NotOwned);
        return report;
    }
    if opts.read_only {
        report.skipped = Some(Skip::ReadOnly);
        return report;
    }
    let primary = resolve_root(root);
    if !primary.is_dir() {
        report.skipped = Some(Skip::Missing);
        return report;
    }
    let matcher = ExcludeMatcher::new(&primary, &owned_exclude(&opts.record));
    let primary_str = primary.to_string_lossy().into_owned();
    for scope in project_scopes_max(&primary, opts.max_attached) {
        let file = session_file_for(&paths.home, &scope);
        let mut su = ScopeUpkeep { scope: scope.to_string_lossy().into_owned(), ..Default::default() };
        match load(&file) {
            Loaded::None => continue,
            Loaded::Skip(why) => su.skipped = Some(why),
            Loaded::Session(session, size) => {
                let ctx = Ctx { primary: &primary_str, matcher: &matcher, now_ms };
                let (session, guards, changed) = tidy(session, size, &ctx, &mut su);
                if changed {
                    before_persist(&file);
                    let store = StorePaths { home: paths.home.clone(), desktop_home: paths.desktop_home.clone(), session_file: file };
                    match persist(&store, session, &guards, true) {
                        Ok(_) => su.written = true,
                        Err(e) => su.error = Some(e.to_string()),
                    }
                }
            }
        }
        report.scopes.push(su);
    }
    report
}

enum Loaded {
    /// No session file: nothing to keep up, and none is created.
    None,
    Skip(String),
    /// The session and the file's size in bytes.
    Session(Session, u64),
}

/// The scope's session as `decide` reads it: strict UTF-8, Known shape only.
fn load(file: &PathBuf) -> Loaded {
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::None,
        Err(e) => return Loaded::Skip(format!("cannot read the session file: {e}")),
    };
    let size = bytes.len() as u64;
    let Ok(text) = String::from_utf8(bytes) else { return Loaded::Skip("the session file is not UTF-8".into()) };
    match parse_session(&text) {
        Ok((s, Shape::Known)) => Loaded::Session(s, size),
        Ok((_, Shape::Unknown(_))) => Loaded::Skip("written by a newer ClaudeGate".into()),
        Err(e) => Loaded::Skip(e.to_string()),
    }
}

struct Ctx<'a> {
    primary: &'a str,
    matcher: &'a ExcludeMatcher,
    now_ms: i64,
}

/// Every upkeep step on one loaded session, in the extension's order. Returns the session, the
/// guards its persist needs, and whether anything changed.
fn tidy(mut s: Session, size: u64, ctx: &Ctx, su: &mut ScopeUpkeep) -> (Session, MergeGuards, bool) {
    let mut guards = MergeGuards::default();
    let mut changed = false;
    let prune = |s: &mut Session, guards: &mut MergeGuards, paths: Vec<String>| {
        for p in &paths {
            if let Some(e) = s.files.remove(p) {
                guards.pruned_files.insert(p.clone(), e.captured_at);
            }
        }
        paths
    };

    // pruneOutOfWorkspaceEntries
    let outside: Vec<String> = s.files.keys().filter(|fp| !path_is_under(fp, ctx.primary)).cloned().collect();
    su.pruned_outside = prune(&mut s, &mut guards, outside);

    // cleanupExcludedOnce
    if !s.exclude_cleanup_done {
        let excluded: Vec<String> = s.files.keys().filter(|fp| ctx.matcher.is_excluded(fp)).cloned().collect();
        su.pruned_excluded = prune(&mut s, &mut guards, excluded);
        s.exclude_cleanup_done = true;
        su.latched = true;
        changed = true;
    }

    // reconcilePending
    let no_ops: Vec<String> = s
        .files
        .iter()
        .filter(|(fp, e)| should_prune_no_op(e, read_disk(fp).as_deref(), ctx.now_ms))
        .map(|(fp, _)| fp.clone())
        .collect();
    su.pruned_no_op = prune(&mut s, &mut guards, no_ops);

    // healOversized
    if size > SESSION_SIZE_WARN_BYTES {
        let before: HashSet<String> = s.accepted.iter().map(|r| r.id.clone()).collect();
        let dropped = cap_accepted_bytes(&mut s, MAX_ACCEPTED_BYTES);
        if dropped > 0 {
            let kept: HashSet<&String> = s.accepted.iter().map(|r| &r.id).collect();
            guards.dropped_accepted_ids.extend(before.into_iter().filter(|id| !kept.contains(id)));
            su.trimmed_accepted = dropped;
        }
    }

    changed |= !su.pruned_outside.is_empty()
        || !su.pruned_excluded.is_empty()
        || !su.pruned_no_op.is_empty()
        || su.trimmed_accepted > 0;
    (s, guards, changed)
}

/// `pathIsUnder`: `child` starts with `parent + sep`, case-folded on Windows.
fn path_is_under(child: &str, parent: &str) -> bool {
    let fold = |p: &str| if cfg!(windows) { p.to_lowercase() } else { p.to_owned() };
    fold(child).starts_with(&format!("{}{MAIN_SEPARATOR}", fold(parent)))
}

#[cfg(test)]
mod tests {
    use super::path_is_under;

    #[cfg(unix)]
    #[test]
    fn under_needs_the_separator() {
        assert!(path_is_under("/ws/a.ts", "/ws"));
        assert!(!path_is_under("/wsx/a.ts", "/ws"));
        assert!(!path_is_under("/ws", "/ws"));
    }
}
