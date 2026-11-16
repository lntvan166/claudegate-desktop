//! Tauri commands: thin, read-only wrappers over `claudegate-core`. Core work runs on the
//! blocking pool; errors cross the boundary as `String`.

use crate::watch::KnownMap;
use claudegate_core::decide::{decide as core_decide, Action, DecideError, DecidePaths, DecideReport, Target};
use claudegate_core::model::{parse_session, ReviewRecord, Session};
use claudegate_core::paths::{claudegate_home, resolve_root, session_file_for};
use claudegate_core::project::{load_project, ProjectSummary, ProjectView};
use claudegate_core::roots::read_roots;
use claudegate_core::textfile::{read_text, TextFile};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffKind {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TextSide {
    Missing,
    Binary,
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPayload {
    pub path: String,
    pub before: TextSide,
    pub after: TextSide,
}

fn side_of(s: &Option<String>) -> TextSide {
    match s {
        None => TextSide::Missing,
        Some(t) => TextSide::Text { text: t.clone() },
    }
}

fn side_of_file(f: TextFile) -> TextSide {
    match f {
        TextFile::Missing => TextSide::Missing,
        TextFile::Binary => TextSide::Binary,
        TextFile::Text(t) => TextSide::Text { text: t },
    }
}

fn record_payload(r: &ReviewRecord) -> DiffPayload {
    DiffPayload { path: r.path.clone(), before: side_of(&r.before), after: side_of(&r.after) }
}

fn load_session(home: &Path, scope: &str) -> Result<Session, String> {
    let file = session_file_for(home, Path::new(scope));
    let bytes = std::fs::read(&file).map_err(|e| format!("cannot read session file: {e}"))?;
    parse_session(&String::from_utf8_lossy(&bytes))
        .map(|(s, _)| s)
        .map_err(|e| e.to_string())
}

/// Builds the before/after pair for one row. Never writes.
pub fn diff_for(home: &Path, scope: &str, kind: DiffKind, key: &str) -> Result<DiffPayload, String> {
    let session = load_session(home, scope).map_err(|_| "not found".to_string())?;
    match kind {
        DiffKind::Pending => {
            let e = session.files.get(key).ok_or("not found")?;
            Ok(DiffPayload {
                path: key.to_string(),
                before: side_of(&e.original_content),
                after: side_of_file(read_text(Path::new(key))),
            })
        }
        DiffKind::Accepted => session
            .accepted
            .iter()
            .find(|r| r.id == key)
            .map(record_payload)
            .ok_or_else(|| "not found".to_string()),
        DiffKind::Rejected => session
            .rejected
            .get(key)
            .map(record_payload)
            .ok_or_else(|| "not found".to_string()),
    }
}

/// Records each scope's session file against its project root so the watcher can map events.
fn remember(known: &KnownMap, home: &Path, view: &ProjectView) {
    let Ok(mut k) = known.lock() else { return };
    k.retain(|(r, _)| r != &view.root);
    for s in &view.scopes {
        k.push((view.root.clone(), session_file_for(home, Path::new(&s.root))));
    }
}

pub fn list_projects_with(home: &Path, known: &KnownMap) -> Vec<ProjectSummary> {
    let roots: Vec<PathBuf> = read_roots(home);
    if let Ok(mut k) = known.lock() {
        k.clear();
    }
    roots
        .into_iter()
        .map(|r| {
            let v = load_project(home, &r);
            remember(known, home, &v);
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

pub fn get_project_with(home: &Path, known: &KnownMap, root: &str) -> ProjectView {
    let v = load_project(home, &resolve_root(Path::new(root)));
    remember(known, home, &v);
    v
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_projects(known: State<'_, KnownMap>) -> Result<Vec<ProjectSummary>, String> {
    let known = known.inner().clone();
    blocking(move || list_projects_with(&claudegate_home(), &known)).await
}

#[tauri::command]
pub async fn get_project(root: String, known: State<'_, KnownMap>) -> Result<ProjectView, String> {
    let known = known.inner().clone();
    blocking(move || get_project_with(&claudegate_home(), &known, &root)).await
}

#[tauri::command]
pub async fn get_diff(
    root: String,
    scope: String,
    kind: DiffKind,
    key: String,
) -> Result<DiffPayload, String> {
    let _ = root; // the scope's own session file is authoritative
    blocking(move || diff_for(&claudegate_home(), &scope, kind, &key)).await?
}

/// The one write lock: every decision runs on the blocking pool holding it, so the app never
/// races itself. (Other writers, the hook and the extension, are handled by the store's file lock.)
#[derive(Default)]
pub struct WriteLock(pub Mutex<()>);

/// `{ kind: "file"|"folder"|"all", scope?, key?, prefix? }` as the UI sends it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDto {
    pub kind: String,
    pub scope: Option<String>,
    pub key: Option<String>,
    pub prefix: Option<String>,
}

pub fn parse_action(s: &str) -> Result<Action, String> {
    Ok(match s {
        "accept" => Action::Accept,
        "reject" => Action::Reject,
        "revert" => Action::Revert,
        "reapply" => Action::Reapply,
        "clearAccepted" => Action::ClearAccepted,
        "clearRejected" => Action::ClearRejected,
        other => return Err(format!("unknown action: {other}")),
    })
}

pub fn parse_target(t: &TargetDto) -> Result<Target, String> {
    let need = |v: &Option<String>, what: &str| {
        v.clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("a {} target needs a {what}", t.kind))
    };
    match t.kind.as_str() {
        "file" => Ok(Target::File { scope: need(&t.scope, "scope")?, key: need(&t.key, "key")? }),
        "folder" => Ok(Target::Folder { scope: need(&t.scope, "scope")?, prefix: need(&t.prefix, "prefix")? }),
        "all" => Ok(Target::All),
        other => Err(format!("unknown target kind: {other}")),
    }
}

/// UTC time as JS `toISOString()` writes it: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub fn iso_from_ms(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// What the UI receives on failure: JSON in the `Err` string. `Store` carries the structured data.
pub fn error_json(e: &DecideError) -> String {
    let message = e.to_string();
    let v = match e {
        DecideError::ReadOnly(_) => serde_json::json!({ "kind": "readOnly", "message": message }),
        DecideError::NotFound => serde_json::json!({ "kind": "notFound", "message": message }),
        DecideError::BadTarget(_) => serde_json::json!({ "kind": "badTarget", "message": message }),
        DecideError::Store { changed, partial, .. } => serde_json::json!({
            "kind": "store", "message": message, "changed": changed, "partial": partial,
        }),
    };
    v.to_string()
}

fn invalid_json(message: &str) -> String {
    serde_json::json!({ "kind": "invalid", "message": message }).to_string()
}

pub struct DecideOutcome {
    pub result: Result<DecideReport, String>,
    /// Files may have changed (Ok, or a `Store` error): the caller should emit `changed` now.
    pub changed: bool,
}

/// The whole `decide` command minus Tauri. Validates, then takes `lock`, reads `readOnly` from
/// the config file afresh and runs the core decision.
pub fn run_decide(
    lock: &Mutex<()>,
    paths: &DecidePaths,
    config_path: &Path,
    root: &str,
    action: &str,
    target: &TargetDto,
    now_iso: &str,
) -> DecideOutcome {
    let parsed = parse_action(action).and_then(|a| parse_target(target).map(|t| (a, t)));
    let (action, target) = match parsed {
        Ok(p) => p,
        Err(m) => return DecideOutcome { result: Err(invalid_json(&m)), changed: false },
    };
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let read_only = crate::config::load_or_create(config_path).read_only;
    match core_decide(paths, &resolve_root(Path::new(root)), action, target, read_only, now_iso) {
        Ok(r) => DecideOutcome { result: Ok(r), changed: true },
        Err(e) => {
            let changed = matches!(e, DecideError::Store { .. });
            DecideOutcome { result: Err(error_json(&e)), changed }
        }
    }
}

#[tauri::command]
pub async fn decide(
    root: String,
    action: String,
    target: TargetDto,
    app: tauri::AppHandle,
) -> Result<DecideReport, String> {
    // The blocking task needs an owned handle; the lock is reached through it.
    let handle = app.clone();
    let out = blocking({
        let root = root.clone();
        move || {
            let lock = handle.state::<WriteLock>();
            let paths = DecidePaths {
                home: claudegate_home(),
                desktop_home: crate::config::config_path()
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default(),
            };
            run_decide(
                &lock.0,
                &paths,
                &crate::config::config_path(),
                &root,
                &action,
                &target,
                &iso_from_ms(claudegate_core::time::now_ms()),
            )
        }
    })
    .await?;
    if out.changed {
        let _ = app.emit(crate::watch::CHANGED_EVENT, crate::watch::ChangedPayload { roots: vec![root] });
    }
    out.result
}
