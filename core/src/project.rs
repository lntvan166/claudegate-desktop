use crate::excludes::{read_excludes, ExcludeMatcher};
use crate::model::{has_real_change, parse_session, ReviewRecord, Session, Shape};
use crate::paths::{resolve_root, session_file_for};
use crate::roots::read_roots;
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

fn read_disk(path: &str) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn load_scope(
    home: &Path,
    scope_root: &Path,
    primary: &str,
    matcher: &ExcludeMatcher,
    is_worktree: bool,
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
            if !has_real_change(e.original_content.as_deref(), disk.as_deref()) {
                continue;
            }
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

pub fn load_project(home: &Path, root: &Path) -> ProjectView {
    let primary_path = resolve_root(root);
    let primary = primary_path.to_string_lossy().into_owned();
    let mut view = ProjectView {
        root: primary.clone(),
        missing: !primary_path.is_dir(),
        scopes: vec![],
        pending: vec![],
        accepted: vec![],
        rejected: vec![],
    };
    if view.missing {
        return view;
    }
    let globs = read_excludes(home).get(&primary).cloned().unwrap_or_default();
    let matcher = ExcludeMatcher::new(&primary_path, &globs);
    load_scope(home, &primary_path, &primary, &matcher, false, &mut view);
    for wt in nested_worktrees_under(&primary_path) {
        load_scope(home, &wt, &primary, &matcher, true, &mut view);
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
            }
        })
        .collect()
}
