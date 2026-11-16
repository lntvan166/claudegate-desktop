//! Decisions across a project's scopes: a port of the extension's `sessionManager.ts` (v1.16.1)
//! `acceptFile/Folder/All`, `applyReject` + `rejectFile/Folder/All`, `revertAccepted*`,
//! `reapply*` and `clearAccepted/Rejected`, each persisted with the same `MergeGuards`.
//!
//! Every target scope's session is loaded and checked before any workspace file is touched: the
//! read-only switch, an Unknown shape or an unreadable session in any target scope refuses the
//! whole action. Inside a scope the TS order is kept: per file, read `after`, write the
//! workspace file, record; then one `persist` for the scope. Workspace files are written only
//! through `workspace_write`.

use crate::excludes::ExcludeMatcher;
use crate::merge::{accept_entry, reject_entry, MergeGuards};
use crate::model::{has_real_change, parse_session, FileEntry, Session, Shape};
use crate::paths::{resolve_root, session_file_for};
use crate::project::{project_matcher, project_scopes, read_disk};
use crate::store::{persist, StoreError, StorePaths};
use crate::workspace_write;
use serde::Serialize;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

/// Spec, "Protecting production" rule 1.
pub const UNKNOWN_SHAPE_MESSAGE: &str =
    "This project was written by a newer ClaudeGate. Update the app to make decisions here.";
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
    /// The session write failed. `changed` lists the workspace files already restored, deleted
    /// or rewritten in that scope, which the review log therefore does not reflect.
    Store { error: StoreError, changed: Vec<String> },
}

impl std::fmt::Display for DecideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecideError::ReadOnly(msg) => f.write_str(msg),
            DecideError::NotFound => f.write_str("nothing to decide: the file or record is no longer there"),
            DecideError::Store { error, changed } if changed.is_empty() => write!(f, "{error}"),
            DecideError::Store { error, changed } => write!(
                f,
                "{error}. These files were changed on disk but the review log was not updated — retry: {}",
                changed.join(", ")
            ),
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
    if read_only {
        return Err(DecideError::ReadOnly(READ_ONLY_MESSAGE.to_owned()));
    }
    let primary = resolve_root(root);
    if !primary.is_dir() {
        return Err(DecideError::NotFound);
    }
    let scopes = project_scopes(&primary);
    let targets: Vec<PathBuf> = match &target {
        Target::All => scopes,
        Target::File { scope, .. } | Target::Folder { scope, .. } => {
            vec![scopes.into_iter().find(|s| s.to_string_lossy() == scope.as_str()).ok_or(DecideError::NotFound)?]
        }
    };

    // Load and check every target scope before anything is written.
    let mut loaded: Vec<(PathBuf, PathBuf, Session)> = Vec::new();
    for scope in targets {
        let file = session_file_for(&paths.home, &scope);
        if let Some(session) = load_checked(&scope, &file)? {
            loaded.push((scope, file, session));
        }
    }
    if matches!(target, Target::File { .. }) && loaded.is_empty() {
        return Err(DecideError::NotFound);
    }

    let matcher = project_matcher(&paths.home, &primary);
    let mut report = DecideReport::default();
    for (scope, file, mut session) in loaded {
        let mut guards = MergeGuards::default();
        let mut changed = Vec::new();
        let ctx = Ctx { scope: &scope, matcher: &matcher, now_iso };
        let mutated = match action {
            Action::Accept => accept(&ctx, &target, &mut session, &mut guards, &mut report)?,
            Action::Reject => reject(&ctx, &target, &mut session, &mut report, &mut changed)?,
            Action::Revert => revert(&ctx, &target, &mut session, &mut guards, &mut report)?,
            Action::Reapply => reapply(&ctx, &target, &mut session, &mut guards, &mut report, &mut changed)?,
            Action::ClearAccepted => clear_accepted(&target, &mut session, &mut guards, &mut report)?,
            Action::ClearRejected => clear_rejected(&target, &mut session, &mut guards, &mut report)?,
        };
        // The TS returns early when nothing was decided; a scope with no change is not written.
        if !mutated {
            continue;
        }
        before_persist(&file);
        let sp = StorePaths { home: paths.home.clone(), desktop_home: paths.desktop_home.clone(), session_file: file };
        persist(&sp, session, &guards).map_err(|error| DecideError::Store { error, changed })?;
    }
    Ok(report)
}

/// The scope's session, or `None` when it has no session file. Refuses an Unknown shape and a
/// session that cannot be read or parsed: deciding against a log we cannot read could overwrite it.
fn load_checked(scope: &Path, file: &Path) -> Result<Option<Session>, DecideError> {
    let unreadable =
        |e: String| DecideError::ReadOnly(format!("The review log for {} could not be read ({e}).", scope.display()));
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(unreadable(e.to_string())),
    };
    // Strict UTF-8, as `persist` reads it: a lossy decode here would be written back lossy.
    let text = String::from_utf8(bytes).map_err(|e| unreadable(e.to_string()))?;
    match parse_session(&text) {
        Ok((_, Shape::Unknown(_))) => Err(DecideError::ReadOnly(UNKNOWN_SHAPE_MESSAGE.to_owned())),
        Ok((s, Shape::Known)) => Ok(Some(s)),
        Err(e) => Err(unreadable(e.to_string())),
    }
}

struct Ctx<'a> {
    scope: &'a Path,
    matcher: &'a ExcludeMatcher,
    now_iso: &'a str,
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
        !ctx.matcher.is_excluded(fp) && has_real_change(e.original_content.as_deref(), read_disk(fp).as_deref())
    };
    Ok(match target {
        Target::File { key, .. } => {
            if !s.files.contains_key(key) {
                return Err(DecideError::NotFound);
            }
            vec![key.clone()]
        }
        Target::Folder { prefix, .. } => {
            s.files.iter().filter(|(fp, e)| is_under(fp, prefix) && keep(fp, e)).map(|(fp, _)| fp.clone()).collect()
        }
        Target::All => {
            s.files.iter().filter(|(fp, e)| is_under(fp, &scope) && keep(fp, e)).map(|(fp, _)| fp.clone()).collect()
        }
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
fn apply_reject(s: &Session, fp: &str, entry: &FileEntry) -> std::io::Result<RejectOutcome> {
    if let Some(original) = &entry.original_content {
        workspace_write::restore(s, Path::new(fp), original)?;
        return Ok(RejectOutcome::Restored);
    }
    if entry.new_file == Some(true) {
        workspace_write::delete(s, Path::new(fp))?;
        return Ok(RejectOutcome::Deleted);
    }
    Ok(RejectOutcome::Left)
}

/// `rejectFile` / `rejectFolder` / `rejectAll`. Persisted with no guards, as the TS does.
fn reject(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    report: &mut DecideReport,
    changed: &mut Vec<String>,
) -> Result<bool, DecideError> {
    let mut mutated = false;
    for fp in pending_targets(ctx, target, s)? {
        let Some(entry) = s.files.get(&fp).cloned() else { continue };
        let after = read_disk(&fp); // Claude's discarded version
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
        Target::Folder { prefix, .. } => s.accepted.iter().filter(|r| is_under(&r.path, prefix)).cloned().collect(),
        Target::All => s.accepted.clone(),
    };
    for rec in &recs {
        if let Some(i) = s.accepted.iter().position(|r| r.id == rec.id) {
            s.accepted.remove(i);
        }
        if !s.files.contains_key(&rec.path) && read_disk(&rec.path) == rec.after {
            let entry = reopened(rec.before.clone(), rec.new_file, rec.session_id.clone(), ctx.now_iso);
            s.files.insert(rec.path.clone(), entry);
        }
        guards.dropped_accepted_ids.insert(rec.id.clone());
        report.done += 1;
    }
    Ok(!recs.is_empty())
}

/// `reapplyRejected` / `reapplyFolder` / `reapplyAll`: write the record's `after` back and reopen
/// the entry as pending. Folder and all skip excluded paths.
fn reapply(
    ctx: &Ctx,
    target: &Target,
    s: &mut Session,
    guards: &mut MergeGuards,
    report: &mut DecideReport,
    changed: &mut Vec<String>,
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
            .filter(|fp| is_under(fp, prefix) && !ctx.matcher.is_excluded(fp))
            .cloned()
            .collect(),
        Target::All => s.rejected.keys().filter(|fp| !ctx.matcher.is_excluded(fp)).cloned().collect(),
    };
    let mut mutated = false;
    for fp in paths {
        let Some(rec) = s.rejected.get(&fp).cloned() else { continue };
        let Some(after) = rec.after.as_deref() else {
            let name = Path::new(&fp).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            report.errors.push((fp, format!("Claude's version of \"{name}\" was not saved")));
            continue;
        };
        if let Err(e) = workspace_write::restore(s, Path::new(&fp), after) {
            report.errors.push((fp, e.to_string()));
            continue;
        }
        changed.push(fp.clone());
        s.rejected.remove(&fp);
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
            Target::Folder { prefix, .. } => is_under(&r.path, prefix),
            Target::All => true,
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
            Target::Folder { prefix, .. } => is_under(fp, prefix),
            Target::All => true,
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
    use super::is_under;

    #[test]
    fn is_under_needs_a_separator_after_the_dir() {
        assert!(is_under("/ws/src/a.ts", "/ws/src"));
        assert!(is_under("/ws/src/a.ts", "/ws/src/"));
        assert!(!is_under("/ws/srcx/a.ts", "/ws/src"));
        assert!(!is_under("/ws/src", "/ws/src"));
    }
}
