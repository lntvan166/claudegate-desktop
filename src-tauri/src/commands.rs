//! Tauri commands: thin, read-only wrappers over `claudegate-core`. Core work runs on the
//! blocking pool; errors cross the boundary as `String`.

use crate::watch::KnownMap;
use claudegate_core::decide::{decide_confirmed, Action, DecideError, DecidePaths, DecideReport, Target};
use claudegate_core::model::{parse_session, ReviewRecord, Session};
use claudegate_core::paths::{claudegate_home, resolve_root, session_file_for};
use claudegate_core::project::{load_project_max, ProjectSummary, ProjectView};
use claudegate_core::settings::{default_ext_sources, effective, Effective, ExtSources};
use claudegate_core::roots::read_roots;
use claudegate_core::textfile::{read_text, TextFile};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Manager, State};

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
    list_projects_max(home, known, &|_| claudegate_core::worktrees::MAX_WORKTREES)
}

/// `list_projects_with`, attaching at most `max_for(root)` worktrees per project.
pub fn list_projects_max(home: &Path, known: &KnownMap, max_for: &dyn Fn(&Path) -> usize) -> Vec<ProjectSummary> {
    let roots: Vec<PathBuf> = read_roots(home);
    if let Ok(mut k) = known.lock() {
        k.clear();
    }
    roots
        .into_iter()
        .map(|r| {
            let v = load_project_max(home, &r, max_for(&r));
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
    get_project_max(home, known, root, claudegate_core::worktrees::MAX_WORKTREES)
}

/// `get_project_with`, attaching at most `max_attached` worktrees (`worktrees.maxAttached`).
pub fn get_project_max(home: &Path, known: &KnownMap, root: &str, max_attached: usize) -> ProjectView {
    let v = load_project_max(home, &resolve_root(Path::new(root)), max_attached);
    remember(known, home, &v);
    v
}

/// Where the extension's settings live on this machine. The two ClaudeGate-home files follow
/// `$CLAUDEGATE_HOME`, like everything else the app reads there.
pub fn ext_sources() -> ExtSources {
    let cg = claudegate_home();
    let mut s = default_ext_sources(&dirs::home_dir().unwrap_or_default());
    s.excludes_json = cg.join("excludes.json");
    s.hooklog_sentinel = cg.join(claudegate_core::store::HOOKLOG_SENTINEL);
    s
}

/// What the Settings screen shows: every effective setting with its source, and the selected
/// project's app-only hide patterns. Serializes flat: `Effective`'s fields plus `extraHide`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    pub effective: Effective,
    pub extra_hide: Vec<String>,
}

pub fn settings_view(config_path: &Path, root: Option<&str>, sources: &ExtSources) -> SettingsView {
    let (app, extra) = crate::config::read_app_settings(config_path);
    let root = root.map(|r| resolve_root(Path::new(r)));
    let extra_hide = root
        .as_ref()
        .and_then(|r| extra.get(r.to_string_lossy().as_ref()))
        .cloned()
        .unwrap_or_default();
    SettingsView { effective: effective(&app, root.as_deref(), sources), extra_hide }
}

/// `worktrees.maxAttached` for `root`, as the Settings screen resolves it.
pub fn max_attached_for(config_path: &Path, root: &Path, sources: &ExtSources) -> usize {
    let (app, _) = crate::config::read_app_settings(config_path);
    effective(&app, Some(root), sources).max_attached.value as usize
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_projects(known: State<'_, KnownMap>) -> Result<Vec<ProjectSummary>, String> {
    let known = known.inner().clone();
    blocking(move || {
        let (cfg, src) = (crate::config::config_path(), ext_sources());
        list_projects_max(&claudegate_home(), &known, &|r| max_attached_for(&cfg, r, &src))
    })
    .await
}

#[tauri::command]
pub async fn get_project(root: String, known: State<'_, KnownMap>) -> Result<ProjectView, String> {
    let known = known.inner().clone();
    blocking(move || {
        let max = max_attached_for(&crate::config::config_path(), &resolve_root(Path::new(&root)), &ext_sources());
        get_project_max(&claudegate_home(), &known, &root, max)
    })
    .await
}

#[tauri::command]
pub async fn get_settings(root: Option<String>) -> Result<SettingsView, String> {
    blocking(move || settings_view(&crate::config::config_path(), root.as_deref(), &ext_sources())).await
}

/// One app override; `value: null` drops it. Returns the settings as they now resolve.
#[tauri::command]
pub async fn set_setting(key: String, value: serde_json::Value, root: Option<String>) -> Result<SettingsView, String> {
    blocking(move || {
        let cfg = crate::config::config_path();
        crate::config::save_setting(&cfg, &key, value, Some(&claudegate_home()))?;
        Ok(settings_view(&cfg, root.as_deref(), &ext_sources()))
    })
    .await?
}

/// Creates or deletes `~/.claudegate/hooklog.enabled`, the switch the extension shares.
#[tauri::command]
pub async fn set_hook_log(on: bool, root: Option<String>) -> Result<SettingsView, String> {
    blocking(move || {
        claudegate_core::store::set_hook_log(on).map_err(|e| format!("Couldn't change the hook log: {e}"))?;
        Ok(settings_view(&crate::config::config_path(), root.as_deref(), &ext_sources()))
    })
    .await?
}

/// The app-only hide patterns for one project (`projects[root].extraHide` in config.json).
#[tauri::command]
pub async fn set_extra_hide(root: String, patterns: Vec<String>) -> Result<SettingsView, String> {
    blocking(move || {
        let cfg = crate::config::config_path();
        let key = resolve_root(Path::new(&root)).to_string_lossy().into_owned();
        crate::config::save_extra_hide(&cfg, &key, &patterns, Some(&claudegate_home()))?;
        Ok(settings_view(&cfg, Some(&key), &ext_sources()))
    })
    .await?
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

/// `{ kind: "file"|"folder"|"all", scope?, key?, prefix?, keys? }` as the UI sends it. `keys` are
/// the rows a folder or all action counted and the user confirmed (paths, or record ids for
/// revert and clearAccepted); the core acts on no other key.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDto {
    pub kind: String,
    pub scope: Option<String>,
    pub key: Option<String>,
    pub prefix: Option<String>,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
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
        DecideError::Store { changed, partial, snapshot, .. } => serde_json::json!({
            "kind": "store", "message": message, "changed": changed, "partial": partial, "snapshot": snapshot,
        }),
    };
    v.to_string()
}

fn invalid_json(message: &str) -> String {
    serde_json::json!({ "kind": "invalid", "message": message }).to_string()
}

pub const SETTINGS_UNREADABLE_MESSAGE: &str = "ClaudeGate Desktop couldn't read its settings file (~/.claudegate-desktop/config.json), so decisions are paused. Fix or delete that file.";

/// The app's own home for backups: the config file's parent. It must be absolute and outside
/// the ClaudeGate home, or backups would land under the cwd or inside `~/.claudegate`.
pub fn desktop_home_for(config_path: &Path, cg_home: &Path) -> Result<PathBuf, String> {
    let dh = config_path.parent().map(Path::to_path_buf).unwrap_or_default();
    check_desktop_home(&dh, cg_home)?;
    Ok(dh)
}

fn check_desktop_home(dh: &Path, cg_home: &Path) -> Result<(), String> {
    if !dh.is_absolute() {
        return Err("The settings folder is not an absolute path, so backups cannot be placed safely.".into());
    }
    if dh.starts_with(cg_home) {
        return Err("The settings folder is inside the ClaudeGate folder, so decisions are refused.".into());
    }
    Ok(())
}

pub struct DecideOutcome {
    pub result: Result<DecideReport, String>,
}

/// The whole `decide` command minus Tauri. It emits no change event: the UI reloads the project
/// after every decision (success or failure), which is the one guaranteed refresh, and the
/// watcher also sees the session file's rename. Validates, then takes `lock`, reads `readOnly` from
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
    let target_dto = target;
    let parsed = parse_action(action).and_then(|a| parse_target(target_dto).map(|t| (a, t)));
    let (action, target) = match parsed {
        Ok(p) => p,
        Err(m) => return DecideOutcome { result: Err(invalid_json(&m)) },
    };
    if let Err(m) = check_desktop_home(&paths.desktop_home, &paths.home) {
        return DecideOutcome { result: Err(invalid_json(&m)) };
    }
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let read_only = match crate::config::read_only_strict(config_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("claudegate: {}: {e}", config_path.display());
            let message = SETTINGS_UNREADABLE_MESSAGE;
            let j = serde_json::json!({ "kind": "readOnly", "message": message }).to_string();
            return DecideOutcome { result: Err(j) };
        }
    };
    let keys = target_dto.keys.as_deref();
    match decide_confirmed(paths, &resolve_root(Path::new(root)), action, target, keys, read_only, now_iso) {
        Ok(r) => DecideOutcome { result: Ok(r) },
        Err(e) => DecideOutcome { result: Err(error_json(&e)) },
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
    let handle = app;
    let out = blocking({
        move || {
            let lock = handle.state::<WriteLock>();
            let home = claudegate_home();
            let cfg = crate::config::config_path();
            let desktop_home = match desktop_home_for(&cfg, &home) {
                Ok(d) => d,
                Err(m) => return DecideOutcome { result: Err(invalid_json(&m)) },
            };
            let paths = DecidePaths { home, desktop_home };
            run_decide(
                &lock.0,
                &paths,
                &cfg,
                &root,
                &action,
                &target,
                &iso_from_ms(claudegate_core::time::now_ms()),
            )
        }
    })
    .await?;
    out.result
}
