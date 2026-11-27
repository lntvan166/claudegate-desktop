//! Tauri commands: thin, read-only wrappers over `claudegate-core`. Core work runs on the
//! blocking pool; errors cross the boundary as `String`.

use crate::watch::KnownMap;
use claudegate_core::decide::{decide_confirmed, decide_confirmed_scoped, Action, DecideError, DecidePaths, DecideReport, Target};
use claudegate_core::model::{parse_session, ReviewRecord, Session};
use claudegate_core::paths::{claudegate_home, resolve_root, session_file_for};
use claudegate_core::project::{listed_roots, load_project_for, load_project_max, summary_of, ProjectSummary, ProjectView};
use claudegate_core::settings::{default_ext_sources, effective, Effective, ExtSources, Resolved, Resolver, Source};
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
    list_projects_opts(home, known, &|r| LoadOpts { max_attached: max_for(r), exclude: None, extra_hide: vec![] })
}

/// How one project is loaded for the app: its worktree limit, the exclude to filter with when
/// the app owns the root (else `excludes.json[root]`), and its app-only hide patterns, which
/// the summary's count leaves out like the UI's lists do.
pub struct LoadOpts {
    pub max_attached: usize,
    pub exclude: Option<Vec<String>>,
    pub extra_hide: Vec<String>,
}

/// Every project's summary, loaded with `opts_for(root)`. A registered root nested inside another
/// listed one is not a project of its own: it is loaded as a scope of its parent, filtered by its
/// own exclude (`opts_for(nested).exclude`).
pub fn list_projects_opts(home: &Path, known: &KnownMap, opts_for: &dyn Fn(&Path) -> LoadOpts) -> Vec<ProjectSummary> {
    let roots: Vec<PathBuf> = read_roots(home);
    if let Ok(mut k) = known.lock() {
        k.clear();
    }
    listed_roots(&roots)
        .into_iter()
        .map(|r| {
            let o = opts_for(&r);
            let v = load_with_opts(home, &r, &o, opts_for);
            remember(known, home, &v);
            let hidden = ExtraHide::new(&v, &o.extra_hide, opts_for);
            summary_of(&r, &v, &|p| !hidden.hides(p))
        })
        .collect()
}

/// The app-only hide patterns of a loaded project, each anchored at the registered root that
/// owns them: the project's own for its rows, a nested root's own for that root's rows (its
/// worktrees included).
pub struct ExtraHide {
    /// `(owning root, its matcher)`, the project's root first.
    owners: Vec<(PathBuf, claudegate_core::excludes::ExcludeMatcher)>,
}

impl ExtraHide {
    pub fn new(v: &ProjectView, own: &[String], opts_for: &dyn Fn(&Path) -> LoadOpts) -> Self {
        let root = PathBuf::from(&v.root);
        let mut owners = vec![(root.clone(), claudegate_core::excludes::ExcludeMatcher::new(&root, own))];
        for s in v.scopes.iter().filter(|s| s.nested) {
            let n = PathBuf::from(&s.root);
            let m = claudegate_core::excludes::ExcludeMatcher::new(&n, &opts_for(&n).extra_hide);
            owners.push((n, m));
        }
        Self { owners }
    }

    /// Whether the patterns of the root owning `p`'s scope hide it.
    pub fn hides(&self, p: &claudegate_core::project::PendingItem) -> bool {
        let scope = Path::new(&p.scope);
        let owner = self
            .owners
            .iter()
            .filter(|(o, _)| scope.starts_with(o))
            .max_by_key(|(o, _)| o.components().count());
        owner.is_some_and(|(_, m)| m.is_excluded(&p.path))
    }
}

/// Loads `root` with its options `o`, each nested root's scopes with that root's own exclude.
fn load_with_opts(home: &Path, root: &Path, o: &LoadOpts, opts_for: &dyn Fn(&Path) -> LoadOpts) -> ProjectView {
    let primary = resolve_root(root);
    load_project_for(home, &primary, o.max_attached, &|r: &Path| {
        if r == primary {
            o.exclude.clone()
        } else {
            opts_for(r).exclude
        }
    })
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

/// Who writes the selected project's exclude for the hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExcludeOwner {
    /// The app added the root and still owns `excludes.json[root]`: the list is editable here.
    Owned,
    /// The app added the root, then VS Code wrote its own `excludes.json[root]`.
    ManagedByVsCode,
    /// A VS Code project (or no project selected).
    VsCode,
}

/// What the Settings screen shows: every effective setting with its source, the selected
/// project's app-only hide patterns, and who owns its exclude. Serializes flat: `Effective`'s
/// fields plus `extraHide` and `excludeOwner`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    pub effective: Effective,
    pub extra_hide: Vec<String>,
    /// The app-only hide patterns of each registered root nested in the project, shown and edited
    /// here: those roots are not listed on their own. Each hides only its own root's rows.
    pub nested_extra_hide: Vec<NestedHide>,
    pub exclude_owner: ExcludeOwner,
    /// App-only: how "Copy reference for Claude" writes the path, `relative` (default) or `absolute`.
    pub copy_ref_path_style: String,
    /// The globs this project's `.vscode` adds back to an app `protected` list (not in the app's
    /// own list). Shown, but not removable here: removing one would never stick.
    pub protected_workspace: Vec<String>,
    /// The change was saved, but something after it failed (the exclude sync): what to tell the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// One nested root's app-only hide patterns.
#[derive(Debug, Clone, Serialize)]
pub struct NestedHide {
    pub root: String,
    pub patterns: Vec<String>,
}

/// The app's `copyRefPathStyle`, `relative` unless it is exactly `absolute`.
fn copy_ref_path_style(app: &claudegate_core::settings::AppSettings) -> String {
    match app.settings.get("copyRefPathStyle").and_then(|v| v.as_str()) {
        Some("absolute") => "absolute".into(),
        _ => "relative".into(),
    }
}

/// The exclude the app writes for `root`, when it owns it: the project's own list, else the
/// defaults. `None` for a VS Code root or one VS Code took over.
pub fn owned_exclude_for(config_path: &Path, root: &Path) -> Option<Vec<String>> {
    let rec = crate::config::project_record(config_path, &root.to_string_lossy());
    rec.owned().then(|| claudegate_core::projects::owned_exclude(&rec))
}

pub fn settings_view(config_path: &Path, root: Option<&str>, sources: &ExtSources) -> SettingsView {
    let (app, extra) = crate::config::read_app_settings(config_path);
    let root = root.map(|r| resolve_root(Path::new(r)));
    let extra_hide = root
        .as_ref()
        .and_then(|r| extra.get(r.to_string_lossy().as_ref()))
        .cloned()
        .unwrap_or_default();
    // The registered roots nested in it; the roots file sits beside `excludes.json`.
    let nested_extra_hide = match (&root, sources.excludes_json.parent()) {
        (Some(r), Some(cg)) => claudegate_core::project::nested_roots_in(&read_roots(cg), r)
            .into_iter()
            .filter(|n| n.is_dir())
            .map(|n| {
                let key = n.to_string_lossy().into_owned();
                NestedHide { patterns: extra.get(&key).cloned().unwrap_or_default(), root: key }
            })
            .collect(),
        _ => Vec::new(),
    };
    let mut effective = effective(&app, root.as_deref(), sources);
    let mut exclude_owner = ExcludeOwner::VsCode;
    if let Some(r) = &root {
        let rec = crate::config::project_record(config_path, &r.to_string_lossy());
        if rec.owned() {
            exclude_owner = ExcludeOwner::Owned;
            let source = if rec.exclude.is_some() { Source::App } else { Source::Default };
            effective.exclude = Resolved { value: claudegate_core::projects::owned_exclude(&rec), source };
        } else if rec.added_by_app && rec.managed_by_vscode {
            exclude_owner = ExcludeOwner::ManagedByVsCode;
            // What the hook uses now is VS Code's key, whatever the app had saved.
            let key = r.to_string_lossy().into_owned();
            let file = std::fs::read_to_string(&sources.excludes_json).ok();
            let theirs = file.and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| {
                Some(v.get(&key)?.as_array()?.iter().filter_map(|g| g.as_str().map(String::from)).collect::<Vec<_>>())
            });
            if let Some(g) = theirs {
                effective.exclude = Resolved { value: g, source: Source::VsCode };
            }
        }
    }
    // What the project's `.vscode` added back to an app list (core `resolve_all`).
    let protected_workspace = match (effective.protected.source, claudegate_core::settings::app_protected(&app)) {
        (Source::App, Some(own)) => effective.protected.value.iter().filter(|g| !own.contains(g)).cloned().collect(),
        _ => Vec::new(),
    };
    SettingsView {
        effective,
        extra_hide,
        nested_extra_hide,
        exclude_owner,
        copy_ref_path_style: copy_ref_path_style(&app),
        protected_workspace,
        warning: None,
    }
}

/// The options the app loads `root` with (see `LoadOpts`).
pub fn load_opts_for(config_path: &Path, root: &Path, resolver: &Resolver, extra: &std::collections::HashMap<String, Vec<String>>) -> LoadOpts {
    let root = resolve_root(root);
    LoadOpts {
        max_attached: resolver.effective(Some(&root)).max_attached.value as usize,
        exclude: owned_exclude_for(config_path, &root),
        extra_hide: extra.get(root.to_string_lossy().as_ref()).cloned().unwrap_or_default(),
    }
}

/// `list_projects` minus Tauri.
pub fn list_projects_for(home: &Path, known: &KnownMap, config_path: &Path, sources: &ExtSources) -> Vec<ProjectSummary> {
    // The config and the user-level settings files are read once for the whole list.
    let (app, extra) = crate::config::read_app_settings(config_path);
    let resolver = Resolver::new(&app, sources);
    list_projects_opts(home, known, &|r| load_opts_for(config_path, r, &resolver, &extra))
}

/// `get_project` minus Tauri.
pub fn get_project_for(home: &Path, known: &KnownMap, config_path: &Path, root: &str, sources: &ExtSources) -> ProjectView {
    let (app, extra) = crate::config::read_app_settings(config_path);
    let resolver = Resolver::new(&app, sources);
    let opts_for = |r: &Path| load_opts_for(config_path, r, &resolver, &extra);
    let o = opts_for(Path::new(root));
    let mut v = load_with_opts(home, Path::new(root), &o, &opts_for);
    // Counted like the summary: without the rows the app-only hide patterns hide.
    let hidden = ExtraHide::new(&v, &o.extra_hide, &opts_for);
    let counted: Vec<&claudegate_core::project::PendingItem> = v.pending.iter().filter(|p| !hidden.hides(p)).collect();
    (v.counted_pending, v.counted_ready) = (counted.len(), counted.iter().filter(|p| !p.waiting).count());
    remember(known, home, &v);
    v
}

/// `worktrees.maxAttached` for `root`, as the Settings screen resolves it.
pub fn max_attached_for(config_path: &Path, root: &Path, sources: &ExtSources) -> usize {
    let (app, _) = crate::config::read_app_settings(config_path);
    effective(&app, Some(root), sources).max_attached.value as usize
}

/// `get_audit` minus Tauri: the project's scopes (primary, worktrees, nested roots) read for the
/// last 7 days before `now_ms`, without what the user dismissed since the miss and without files
/// that are pending now (the review panel already shows those). Stats are not filtered.
pub fn get_audit_at(home: &Path, config_path: &Path, root: &str, sources: &ExtSources, now_ms: i64) -> claudegate_core::audit::AuditView {
    use claudegate_core::audit::{read_audit, WINDOW_MS};
    let primary = resolve_root(Path::new(root));
    let view = load_project_max(home, &primary, max_attached_for(config_path, &primary, sources));
    let mut roots: Vec<PathBuf> = vec![primary];
    for s in &view.scopes {
        let r = PathBuf::from(&s.root);
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    let mut out = read_audit(home, &roots, now_ms - WINDOW_MS);
    let dismissed = crate::config::read_audit_dismissed(config_path);
    let pending: std::collections::HashSet<PathBuf> = view.pending.iter().map(|p| PathBuf::from(&p.path)).collect();
    let keep = |i: &claudegate_core::audit::AuditItem| {
        !pending.contains(Path::new(&i.path)) && !dismissed.get(&i.path).is_some_and(|d| *d >= i.at_ms)
    };
    out.missed.retain(keep);
    out.uncertain.retain(keep);
    out
}

#[tauri::command]
pub async fn get_audit(root: String) -> Result<claudegate_core::audit::AuditView, String> {
    blocking(move || get_audit_at(&claudegate_home(), &crate::config::config_path(), &root, &ext_sources(), claudegate_core::time::now_ms())).await
}

/// Hides audit items for these paths in this app (config.json only).
#[tauri::command]
pub async fn dismiss_audit(paths: Vec<String>) -> Result<(), String> {
    blocking(move || {
        crate::config::dismiss_audit_paths(
            &crate::config::config_path(),
            &paths,
            claudegate_core::time::now_ms(),
            claudegate_core::audit::WINDOW_MS,
            Some(&claudegate_home()),
        )
    })
    .await?
}

pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_projects(known: State<'_, KnownMap>, app: tauri::AppHandle) -> Result<Vec<ProjectSummary>, String> {
    let known = known.inner().clone();
    blocking(move || {
        let list = list_projects_for(&claudegate_home(), &known, &crate::config::config_path(), &ext_sources());
        // Keep up the sessions of the projects the app owns (a no-op for the rest). A nested root
        // folded into its parent is still a root of its own here.
        for p in &list {
            crate::upkeep::trigger(&app, &p.root);
            for n in &p.nested {
                crate::upkeep::trigger(&app, n);
            }
        }
        list
    })
    .await
}

#[tauri::command]
pub async fn get_project(root: String, known: State<'_, KnownMap>, app: tauri::AppHandle) -> Result<ProjectView, String> {
    let known = known.inner().clone();
    blocking(move || {
        let view = get_project_for(&claudegate_home(), &known, &crate::config::config_path(), &root, &ext_sources());
        crate::upkeep::trigger(&app, &view.root);
        for s in view.scopes.iter().filter(|s| s.nested) {
            crate::upkeep::trigger(&app, &s.root);
        }
        view
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

/// `set_hook_log` minus Tauri, with the sentinel write (`apply`) injected.
/// The sentinel is shared with the extension, so read-only mode blocks it like every other
/// shared-file write (fail closed on an unreadable config).
pub fn set_hook_log_with(config_path: &Path, on: bool, apply: impl FnOnce(bool) -> std::io::Result<()>) -> Result<(), String> {
    check_writable_for(config_path, "turn the hook log on or off")?;
    apply(on).map_err(|e| format!("Couldn't change the hook log: {e}"))
}

/// Creates or deletes `~/.claudegate/hooklog.enabled`, the switch the extension shares.
#[tauri::command]
pub async fn set_hook_log(on: bool, root: Option<String>) -> Result<SettingsView, String> {
    blocking(move || {
        let cfg = crate::config::config_path();
        set_hook_log_with(&cfg, on, claudegate_core::store::set_hook_log)?;
        Ok(settings_view(&cfg, root.as_deref(), &ext_sources()))
    })
    .await?
}

/// The app-only hide patterns for one project (`projects[root].extraHide` in config.json), or for
/// a root nested in it. Returns the settings of `view_root` (the project on screen) when given.
#[tauri::command]
pub async fn set_extra_hide(root: String, patterns: Vec<String>, view_root: Option<String>) -> Result<SettingsView, String> {
    blocking(move || {
        let cfg = crate::config::config_path();
        let key = resolve_root(Path::new(&root)).to_string_lossy().into_owned();
        crate::config::save_extra_hide(&cfg, &key, &patterns, Some(&claudegate_home()))?;
        let shown = view_root.map(|r| resolve_root(Path::new(&r)).to_string_lossy().into_owned()).unwrap_or(key);
        Ok(settings_view(&cfg, Some(&shown), &ext_sources()))
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
    /// `keys` with the scope each row was listed in. When given it is used instead of `keys`:
    /// the same path can be a row in two scopes (a parent and a repo nested in it).
    #[serde(default)]
    pub scoped_keys: Option<Vec<ScopedKey>>,
}

/// One confirmed row: its scope root and its key.
#[derive(Debug, Clone, Deserialize)]
pub struct ScopedKey {
    pub scope: String,
    pub key: String,
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
    let root = resolve_root(Path::new(root));
    let result = match &target_dto.scoped_keys {
        Some(sk) => {
            let pairs: Vec<(String, String)> = sk.iter().map(|k| (k.scope.clone(), k.key.clone())).collect();
            decide_confirmed_scoped(paths, &root, action, target, &pairs, read_only, now_iso)
        }
        None => decide_confirmed(paths, &root, action, target, keys, read_only, now_iso),
    };
    match result {
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

/// Serializes the app's own roots edits (add, remove, and the watcher's re-add) so they never
/// race each other. The extension's writes are merged by the store's re-read-before-rename.
static ROOTS_LOCK: Mutex<()> = Mutex::new(());

fn roots_guard() -> std::sync::MutexGuard<'static, ()> {
    ROOTS_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

pub const READ_ONLY_PROJECTS: &str =
    "Read-only mode is on. Turn it off in Settings to add or remove projects.";

/// Add and Remove write the shared roots file, so read-only mode blocks them (fail closed on an
/// unreadable config).
fn check_writable(config_path: &Path) -> Result<(), String> {
    match crate::config::read_only_strict(config_path) {
        Ok(false) => Ok(()),
        Ok(true) => Err(READ_ONLY_PROJECTS.into()),
        Err(_) => Err(SETTINGS_UNREADABLE_MESSAGE.into()),
    }
}

/// `add_project` minus Tauri: registers `path` in `<cg_home>/workspace-roots.json`, records it in
/// `config_path` as added by the app and adds it to the workspace (an existing root too). The
/// error is a message for the user (it names the conflicting root for a nested or containing
/// folder).
pub fn add_project_with(cg_home: &Path, config_path: &Path, path: &str) -> Result<claudegate_core::projects::Added, String> {
    check_writable(config_path)?;
    let _g = roots_guard();
    let mut cfg = crate::config::ConfigProjects { path: config_path, guard: Some(cg_home) };
    let added = claudegate_core::projects::add_project(cg_home, &mut cfg, path).map_err(|e| e.to_string())?;
    if let Err(e) = crate::config::add_to_workspace(config_path, cg_home, &added.root, Some(cg_home)) {
        eprintln!("claudegate: couldn't add {} to the workspace: {e}", added.root);
    }
    Ok(added)
}

/// What "Remove from workspace" did: the config as it now stands, and the capture side when the
/// user also asked to stop capturing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRemoved {
    pub config: crate::config::Config,
    pub removed: Option<claudegate_core::projects::Removed>,
    /// Capture was stopped but the project could not be taken off the rail: why. `config` is then
    /// the config as it stands.
    pub workspace_error: Option<String>,
}

/// `remove_from_workspace` minus Tauri. Takes `root` off the rail (view only). With `stop_capture`
/// it first runs the Remove flow (`remove_project_with`, every evidence rule included: only an
/// app-added root, forget-only when VS Code or Cursor uses it, blocked in read-only mode); when
/// that fails the workspace is left as it was. When capture was stopped but the workspace write
/// fails, that partial result comes back (`workspace_error`), never as a plain failure. `nested`
/// is the project's folded nested roots, which leave the workspace with it.
pub fn remove_from_workspace_with(
    cg_home: &Path,
    config_path: &Path,
    root: &str,
    nested: &[String],
    stop_capture: bool,
    storage: &[PathBuf],
) -> Result<WorkspaceRemoved, String> {
    let removed = if stop_capture { Some(remove_project_with(cg_home, config_path, root, storage)?) } else { None };
    match crate::config::remove_from_workspace(config_path, cg_home, root, nested, Some(cg_home)) {
        Ok(config) => Ok(WorkspaceRemoved { config, removed, workspace_error: None }),
        Err(e) if removed.is_some() => {
            Ok(WorkspaceRemoved { config: crate::config::load_or_create(config_path), removed, workspace_error: Some(e) })
        }
        Err(e) => Err(e),
    }
}

/// `remove_project` minus Tauri. Only roots the app added; the session file is kept. When VS Code
/// or Cursor also uses the folder (per `storage`, their `storage.json` files, and excludes.json)
/// only the app's record is dropped and capture stays on.
pub fn remove_project_with(
    cg_home: &Path,
    config_path: &Path,
    root: &str,
    storage: &[PathBuf],
) -> Result<claudegate_core::projects::Removed, String> {
    check_writable(config_path)?;
    let _g = roots_guard();
    let mut cfg = crate::config::ConfigProjects { path: config_path, guard: Some(cg_home) };
    claudegate_core::projects::remove_project(cg_home, &mut cfg, root, storage).map_err(|e| e.to_string())
}

/// Re-adds the app's roots that the extension's unlocked write dropped. Returns what it re-added;
/// a failure is logged, never raised (it runs from the watcher).
pub fn ensure_roots_with(cg_home: &Path, config_path: &Path) -> Vec<String> {
    let _g = roots_guard();
    let cfg = crate::config::ConfigProjects { path: config_path, guard: Some(cg_home) };
    match claudegate_core::projects::ensure_app_roots_present(cg_home, &cfg) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("claudegate: couldn't re-add the app's projects: {e}");
            Vec::new()
        }
    }
}

/// Writes the exclude of every root the app owns to `excludes.json` and detects a VS Code
/// takeover (`claudegate_core::projects::sync_excludes`). Runs on changes of excludes.json and
/// of the config, at startup and after an edit. Writes nothing when nothing changed, so its own
/// write's event cannot loop. A failure is logged, never raised (it runs from the watcher).
pub fn sync_excludes_with(cg_home: &Path, config_path: &Path) -> Vec<claudegate_core::projects::RootChange> {
    let _g = roots_guard();
    sync_excludes_locked(cg_home, config_path)
}

/// `sync_excludes_with` for a caller already holding `roots_guard`.
fn sync_excludes_locked(cg_home: &Path, config_path: &Path) -> Vec<claudegate_core::projects::RootChange> {
    let mut cfg = crate::config::ConfigProjects { path: config_path, guard: Some(cg_home) };
    match claudegate_core::projects::sync_excludes(cg_home, &mut cfg) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("claudegate: couldn't sync the projects' exclude: {e}");
            Vec::new()
        }
    }
}

pub const VS_CODE_EXCLUDE: &str =
    "This project comes from VS Code, which sets its exclude list. Change it there, or hide more files below.";
pub const MANAGED_EXCLUDE: &str =
    "Managed by VS Code: a VS Code window wrote its own exclude list for this project, so the app no longer changes it.";

pub(crate) fn check_writable_for(config_path: &Path, what: &str) -> Result<(), String> {
    check_writable(config_path).map_err(|e| e.replace("add or remove projects", what))
}

pub const SYNC_FAILED_PREFIX: &str = "Saved, but the hook still uses the old list: ";

/// `set_project_exclude` minus Tauri: only for a root the app added and still owns. Validates
/// the globs, saves `projects[root].exclude` (`None`: back to the defaults), then syncs so the
/// hook sees it. Read-only mode blocks it. The ownership check, the save and the sync run under
/// one `roots_guard`, so a watcher sync cannot mark the root managed in between. A sync that
/// fails after the save comes back as the view's `warning` ("Saved, but the hook still uses the
/// old list: <reason>"); the next sync (any change of excludes.json or the config) retries.
pub fn set_project_exclude_with(
    cg_home: &Path,
    config_path: &Path,
    root: &str,
    globs: Option<Vec<String>>,
    sources: &ExtSources,
) -> Result<SettingsView, String> {
    check_writable_for(config_path, "change a project's exclude list")?;
    let key = resolve_root(Path::new(root.trim())).to_string_lossy().into_owned();
    let globs = globs.map(|g| claudegate_core::projects::validate_globs(&g)).transpose()?;
    let synced = {
        let _g = roots_guard();
        let rec = crate::config::project_record(config_path, &key);
        if !rec.added_by_app {
            return Err(VS_CODE_EXCLUDE.into());
        }
        if rec.managed_by_vscode {
            return Err(MANAGED_EXCLUDE.into());
        }
        crate::config::save_project_exclude(config_path, &key, globs.as_deref(), Some(cg_home))?;
        let mut cfg = crate::config::ConfigProjects { path: config_path, guard: Some(cg_home) };
        claudegate_core::projects::sync_exclude_of(cg_home, &mut cfg, &key)
    };
    let mut view = settings_view(config_path, Some(&key), sources);
    if let Err(e) = synced {
        eprintln!("claudegate: couldn't sync the exclude of {key}: {e}");
        view.warning = Some(format!("{SYNC_FAILED_PREFIX}{e}"));
    }
    Ok(view)
}

/// `take_back_exclude` minus Tauri: for an app-added root VS Code took over, the app manages its
/// exclude again (it rewrites `excludes.json[root]` with its own list). Read-only mode blocks it.
pub fn take_back_exclude_with(cg_home: &Path, config_path: &Path, root: &str, sources: &ExtSources) -> Result<SettingsView, String> {
    check_writable_for(config_path, "take a project's exclude list back")?;
    let key = resolve_root(Path::new(root.trim())).to_string_lossy().into_owned();
    {
        let _g = roots_guard();
        let rec = crate::config::project_record(config_path, &key);
        if !rec.added_by_app {
            return Err(VS_CODE_EXCLUDE.into());
        }
        if rec.managed_by_vscode {
            let mut cfg = crate::config::ConfigProjects { path: config_path, guard: Some(cg_home) };
            claudegate_core::projects::take_back_exclude(cg_home, &mut cfg, &key)?;
        }
    }
    Ok(settings_view(config_path, Some(&key), sources))
}

#[tauri::command]
pub async fn take_back_exclude(root: String) -> Result<SettingsView, String> {
    blocking(move || take_back_exclude_with(&claudegate_home(), &crate::config::config_path(), &root, &ext_sources())).await?
}

#[tauri::command]
pub async fn set_project_exclude(root: String, globs: Option<Vec<String>>) -> Result<SettingsView, String> {
    blocking(move || set_project_exclude_with(&claudegate_home(), &crate::config::config_path(), &root, globs, &ext_sources()))
        .await?
}

#[tauri::command]
pub async fn add_project(path: String) -> Result<claudegate_core::projects::Added, String> {
    blocking(move || add_project_with(&claudegate_home(), &crate::config::config_path(), &path)).await?
}

#[tauri::command]
pub async fn remove_project(root: String) -> Result<claudegate_core::projects::Removed, String> {
    blocking(move || {
        let storage = claudegate_core::projects::storage_files(&ext_sources());
        remove_project_with(&claudegate_home(), &crate::config::config_path(), &root, &storage)
    })
    .await?
}

#[tauri::command]
pub async fn remove_from_workspace(root: String, nested: Vec<String>, stop_capture: bool) -> Result<WorkspaceRemoved, String> {
    blocking(move || {
        let storage = claudegate_core::projects::storage_files(&ext_sources());
        remove_from_workspace_with(&claudegate_home(), &crate::config::config_path(), &root, &nested, stop_capture, &storage)
    })
    .await?
}

/// Whether VS Code or Cursor also uses `root`, so Remove would leave capture on. Read only; the
/// confirmation words itself on it.
#[tauri::command]
pub async fn project_in_vs_code(root: String) -> Result<bool, String> {
    blocking(move || project_in_vs_code_with(&claudegate_home(), &crate::config::config_path(), &root, &ext_sources()))
        .await
}

/// `project_in_vs_code` minus Tauri: the same evidence `remove_project` weighs.
pub fn project_in_vs_code_with(cg_home: &Path, config_path: &Path, root: &str, sources: &ExtSources) -> bool {
    let storage = claudegate_core::projects::storage_files(sources);
    let rec = crate::config::project_record(config_path, root);
    rec.managed_by_vscode
        || claudegate_core::projects::vscode_uses_root(cg_home, Path::new(root), &rec.written_values(claudegate_core::time::now_ms()), &storage)
}

/// The native folder picker: one directory, or `None` when cancelled.
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .set_title("Add project")
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
}
