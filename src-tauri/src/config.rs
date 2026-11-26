//! The app's own settings, in `~/.claudegate-desktop/config.json`. This is the only file the app
//! writes; it is never under the ClaudeGate home.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ZED: &str = "zed {path}:{line}:{col}";
pub const VSCODE: &str = "code -g {path}:{line}:{col}";
pub const DEFAULT_EDITOR: &str = ZED;

/// The presets before `{col}`: a stored `editor` equal to one is read as its successor, so
/// "Open at cursor" lands on the column. The file itself is not rewritten.
const UPGRADES: [(&str, &str); 2] = [("zed {path}:{line}", ZED), ("code -g {path}:{line}", VSCODE)];

fn upgraded(editor: String) -> String {
    let n = norm(&editor);
    UPGRADES.iter().find(|(old, _)| *old == n).map_or(editor, |(_, new)| (*new).to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Command template for "Open in editor": `{path}`, `{line}` and `{col}` are substituted per argument.
    pub editor: String,
    /// A user-supplied template offered as "Custom…" in the Open-with menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_editor: Option<String>,
    /// When true every decision is refused. Read from disk on each call, never cached.
    #[serde(default)]
    pub read_only: bool,
    /// The roots the user added to the rail, in their order. Only these count in the header total
    /// and the tray; `~/.claudegate/workspace-roots.json` (what is captured) is never touched by
    /// it. `None` until the first load derives it (`ensure_workspace`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Vec<String>>,
    /// The roots the app added (`projects[root].addedByApp`), for the rail's marker and Remove.
    /// Derived on read; config.json never stores this list itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_roots: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: DEFAULT_EDITOR.into(),
            custom_editor: None,
            read_only: false,
            workspace: None,
            app_roots: vec![],
        }
    }
}

/// `<home>/.claudegate-desktop/config.json`.
pub fn config_path_in(home: &Path) -> PathBuf {
    home.join(".claudegate-desktop").join("config.json")
}

/// `$CLAUDEGATE_DESKTOP_CONFIG` if set (dev and sandbox runs), else `~/.claudegate-desktop/config.json`.
pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CLAUDEGATE_DESKTOP_CONFIG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    config_path_in(&dirs::home_dir().unwrap_or_default())
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// `workspace` as stored: `None` when absent or not a list.
/// Each root is reported resolved (`resolve_root`), as the rail's projects are spelled, so the UI
/// matches them the way the backend does; spellings of one folder collapse to one.
fn workspace_in(v: &serde_json::Value) -> Option<Vec<String>> {
    let ws = workspace_of(v.as_object()?)?;
    let resolved = ws.iter().map(|r| claudegate_core::paths::resolve_root(Path::new(r)).to_string_lossy().into_owned());
    let resolved: Vec<String> = resolved.collect();
    Some(clean_roots(resolved.iter().map(String::as_str)))
}

fn workspace_of(obj: &serde_json::Map<String, serde_json::Value>) -> Option<Vec<String>> {
    obj.get("workspace").and_then(|w| w.as_array()).map(|a| clean_roots(a.iter().filter_map(|r| r.as_str())))
}

fn from_value(v: &serde_json::Value) -> Config {
    Config {
        editor: upgraded(non_empty(v.get("editor").and_then(|e| e.as_str())).unwrap_or_else(|| DEFAULT_EDITOR.into())),
        custom_editor: non_empty(v.get("customEditor").and_then(|e| e.as_str())),
        read_only: v.get("readOnly").and_then(|e| e.as_bool()).unwrap_or(false),
        workspace: workspace_in(v),
        app_roots: app_roots_in(v),
    }
}

/// Every `projects[root]` with `addedByApp: true`, in the file's order.
fn app_roots_in(v: &serde_json::Value) -> Vec<String> {
    v.get("projects")
        .and_then(|p| p.as_object())
        .map(|m| {
            m.iter()
                .filter(|(_, e)| e.get("addedByApp").and_then(|b| b.as_bool()) == Some(true))
                .map(|(r, _)| r.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Trimmed, without blanks or repeats, in the order given.
fn clean_roots<'a>(roots: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for r in roots.map(str::trim).filter(|r| !r.is_empty()) {
        if !out.iter().any(|o| o == r) {
            out.push(r.to_string());
        }
    }
    out
}

/// Reads the config. A missing file is created with the defaults (best effort); an unreadable or
/// malformed one reads as the defaults and is left alone.
pub fn load_or_create(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => from_value(&v),
            Err(e) => {
                eprintln!("claudegate: {} is not valid JSON ({e}); using defaults", path.display());
                Config::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let c = Config::default();
            let lock = config_guard();
            // Another writer may have created it since the read: never replace what it wrote.
            if path.exists() {
                drop(lock);
                return load_or_create(path);
            }
            if let Err(e) = write_json(path, &serde_json::to_value(&c).unwrap_or_default()) {
                eprintln!("claudegate: cannot create {}: {e}", path.display());
            }
            c
        }
        Err(e) => {
            eprintln!("claudegate: cannot read {}: {e}; using defaults", path.display());
            Config::default()
        }
    }
}

/// The read-only switch for `decide`: reads without creating anything and fails closed. A missing
/// file means the defaults (`false`); an unreadable or malformed one is an error.
pub fn read_only_strict(path: &Path) -> Result<bool, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("not a JSON object")?;
    match obj.get("readOnly") {
        None => Ok(false),
        Some(serde_json::Value::Bool(b)) => Ok(*b),
        Some(_) => Err("readOnly is not a boolean".into()),
    }
}

fn write_json(path: &Path, v: &serde_json::Value) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("config path has no parent"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".config.json.{}.tmp", std::process::id()));
    let text = serde_json::to_string_pretty(v).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, text + "\n")?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn norm(t: &str) -> String {
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The webview may only choose among commands the user already trusts: the two presets, the
/// `customEditor` and the `editor` written by hand into config.json. Anything else is refused, so a
/// compromised page cannot run or persist an arbitrary command. Returns the normalised template.
pub fn allowed_template(template: &str, cfg: &Config) -> Result<String, String> {
    let t = norm(template);
    let ok = !t.is_empty()
        && [Some(ZED), Some(VSCODE), cfg.custom_editor.as_deref(), Some(cfg.editor.as_str())]
            .into_iter()
            .flatten()
            .any(|a| norm(a) == t);
    if ok {
        Ok(t)
    } else {
        Err("That editor command isn't one of the choices. Pick Zed, VS Code, or set customEditor in ~/.claudegate-desktop/config.json.".into())
    }
}

/// Serializes every write of config.json in this process. Each write is a read-modify-write of
/// the whole file (and shares one temp file name), so without it two concurrent writers (a
/// setting saved while the watcher's exclude sync records `excludeWritten`, say) would lose one
/// another's change. Never held across anything but one config read and write.
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn config_guard() -> std::sync::MutexGuard<'static, ()> {
    CONFIG_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Reads the config file as a JSON object (missing reads as `{}`), applies `f`, writes it back.
/// Every other key is kept. `guard` is the ClaudeGate home: a config path under it is refused.
/// Runs under `CONFIG_LOCK`, so `save_setting`, the workspace, read-only, the editor,
/// `mark_added` and the exclude-sync fields cannot lose each other's writes.
fn update_config(
    path: &Path,
    guard: Option<&Path>,
    f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>, &Config) -> Result<(), String>,
) -> Result<Config, String> {
    update_config_if(path, guard, |obj, cfg| f(obj, cfg).map(|()| true))
}

/// `update_config` where `f` says whether it changed anything: nothing is written when it did not.
fn update_config_if(
    path: &Path,
    guard: Option<&Path>,
    f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>, &Config) -> Result<bool, String>,
) -> Result<Config, String> {
    if let Some(g) = guard {
        if path.starts_with(g) {
            return Err("Refusing to write settings inside the ClaudeGate folder.".into());
        }
    }
    let _lock = config_guard();
    let mut v = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<serde_json::Value>(&text).map_err(|e| {
            format!("{} is not valid JSON ({e}). Fix or delete it, then try again.", path.display())
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
    };
    if !v.is_object() {
        return Err(format!("{} is not a JSON object.", path.display()));
    }
    let cfg = from_value(&v);
    if f(v.as_object_mut().expect("checked above"), &cfg)? {
        write_json(path, &v).map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
    }
    Ok(from_value(&v))
}

/// Persists `template` as the remembered editor, keeping every other key in the file.
pub fn save_editor(path: &Path, template: &str, guard: Option<&Path>) -> Result<Config, String> {
    let template = template.trim();
    if template.is_empty() {
        return Err("The editor command is empty.".into());
    }
    update_config(path, guard, |obj, cfg| {
        let t = allowed_template(template, cfg)?;
        obj.insert("editor".into(), serde_json::Value::String(t));
        Ok(())
    })
}

/// Persists the read-only switch, keeping every other key in the file.
pub fn save_read_only(path: &Path, on: bool, guard: Option<&Path>) -> Result<Config, String> {
    update_config(path, guard, |obj, _| {
        obj.insert("readOnly".into(), serde_json::Value::Bool(on));
        Ok(())
    })
}

/// The setting names the UI sends, mapped to the `settings` keys `claudegate_core::settings`
/// reads (the extension's names without `claudegate.`). Exclude is per project and the hook log
/// is a sentinel file, so neither is here.
fn setting_key(key: &str) -> Option<&'static str> {
    Some(match key {
        "autoAdvance" => "autoAdvance",
        "groupBySession" => "groupBySession",
        "pendingAgeMinDays" => "pendingAge.minDays",
        "maxAttached" => "worktrees.maxAttached",
        "protected" => "protected",
        // App-only: the extension has no such setting, so it never resolves through `claudegate_core`.
        "copyRefPathStyle" => "copyRefPathStyle",
        _ => return None,
    })
}

fn whole_number(v: &serde_json::Value, min: u64) -> Option<u64> {
    let n = v.as_u64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64))?;
    (n >= min && n <= u32::MAX as u64).then_some(n)
}

/// The value as it is stored, or why it is refused.
fn checked_setting(key: &str, v: &serde_json::Value) -> Result<serde_json::Value, String> {
    use serde_json::Value;
    let bad = |what: &str| Err(format!("{key} must be {what}."));
    match key {
        "autoAdvance" | "groupBySession" => match v {
            Value::Bool(_) => Ok(v.clone()),
            _ => bad("true or false"),
        },
        "copyRefPathStyle" => match v.as_str() {
            Some(s @ ("relative" | "absolute")) => Ok(Value::from(s)),
            _ => bad("\"relative\" or \"absolute\""),
        },
        "pendingAgeMinDays" => whole_number(v, 0).map(Value::from).map_or_else(|| bad("a whole number of days, 0 or more"), Ok),
        "maxAttached" => whole_number(v, 1).map(Value::from).map_or_else(|| bad("a whole number, 1 or more"), Ok),
        "protected" => match v.as_array() {
            Some(a) if a.iter().all(Value::is_string) => {
                Ok(serde_json::json!(clean_roots(a.iter().filter_map(Value::as_str))))
            }
            _ => bad("a list of glob patterns"),
        },
        _ => Err(format!("Unknown setting: {key}")),
    }
}

/// Sets one app override in `settings` (`null` drops it, so the value falls back to the
/// extension's, then the default). Every other key in the file is kept.
pub fn save_setting(path: &Path, key: &str, value: serde_json::Value, guard: Option<&Path>) -> Result<(), String> {
    let stored = setting_key(key).ok_or_else(|| format!("Unknown setting: {key}"))?;
    let value = if value.is_null() { None } else { Some(checked_setting(key, &value)?) };
    update_config(path, guard, |obj, _| {
        let settings = obj.entry("settings").or_insert_with(|| serde_json::json!({}));
        let map = settings.as_object_mut().ok_or("The settings entry in config.json is not an object.")?;
        match value {
            Some(v) => map.insert(stored.into(), v),
            None => map.remove(stored),
        };
        Ok(())
    })
    .map(|_| ())
}

/// Sets the app-only hide patterns for `root` (`projects[root].extraHide`). They only hide rows in
/// this app; capture and the extension never see them.
pub fn save_extra_hide(path: &Path, root: &str, patterns: &[String], guard: Option<&Path>) -> Result<(), String> {
    let root = root.trim();
    if root.is_empty() {
        return Err("No project given.".into());
    }
    let patterns = clean_roots(patterns.iter().map(String::as_str));
    update_config(path, guard, |obj, _| {
        let projects = obj.entry("projects").or_insert_with(|| serde_json::json!({}));
        let projects = projects.as_object_mut().ok_or("The projects entry in config.json is not an object.")?;
        let entry = projects.entry(root.to_string()).or_insert_with(|| serde_json::json!({}));
        let entry = entry.as_object_mut().ok_or("That project's entry in config.json is not an object.")?;
        if patterns.is_empty() {
            entry.remove("extraHide");
        } else {
            entry.insert("extraHide".into(), serde_json::json!(patterns));
        }
        Ok(())
    })
    .map(|_| ())
}

/// The roots the app added. Lenient and never creates the file: unreadable reads as none.
pub fn app_roots(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .map(|v| app_roots_in(&v))
        .unwrap_or_default()
}

/// Records `root` as added by the app (`projects[root].addedByApp = true`), keeping every other
/// key, including the project's own `extraHide` / `exclude`.
pub fn mark_added_by_app(path: &Path, root: &str, guard: Option<&Path>) -> Result<(), String> {
    update_config(path, guard, |obj, _| {
        let projects = obj.entry("projects").or_insert_with(|| serde_json::json!({}));
        let projects = projects.as_object_mut().ok_or("The projects entry in config.json is not an object.")?;
        let entry = projects.entry(root.to_string()).or_insert_with(|| serde_json::json!({}));
        let entry = entry.as_object_mut().ok_or("That project's entry in config.json is not an object.")?;
        entry.insert("addedByApp".into(), serde_json::Value::Bool(true));
        Ok(())
    })
    .map(|_| ())
}

fn same_root(a: &str, root: &str) -> bool {
    use claudegate_core::paths::resolve_root;
    resolve_root(Path::new(a)) == resolve_root(Path::new(root))
}

/// Drops the app's whole record of `root` after Remove: every `projects` key that resolves to it.
/// The workspace is the caller's (`remove_from_workspace`).
pub fn forget_project(path: &Path, root: &str, guard: Option<&Path>) -> Result<(), String> {
    update_config(path, guard, |obj, _| {
        if let Some(p) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
            p.retain(|k, _| !same_root(k, root));
        }
        Ok(())
    })
    .map(|_| ())
}

/// The roots the rail lists today (`workspace-roots.json`, a nested root folded into its parent),
/// in the file's order. Strict (`read_roots_strict`): the migration must never derive the
/// workspace from a file caught half-written, since it drops the hidden list it derives from.
pub fn listed_root_strings(home: &Path) -> Result<Vec<String>, String> {
    let roots = claudegate_core::roots::read_roots_strict(home)?;
    Ok(claudegate_core::project::listed_roots(&roots).into_iter().map(|r| r.to_string_lossy().into_owned()).collect())
}

/// The workspace in `obj`, or, when it has none yet, the one the migration derives: every listed
/// root not in the retired `hiddenProjects`, in the listed order. Deriving also drops
/// `hiddenProjects` and `hiddenAt`, in the same write as the caller's change. The bool says
/// whether it derived. When the listed roots cannot be read nothing is changed and it fails, so
/// the caller writes nothing and the next load retries.
fn workspace_or_migrated(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    listed: impl FnOnce() -> Result<Vec<String>, String>,
) -> Result<(Vec<String>, bool), String> {
    if let Some(w) = workspace_of(obj) {
        return Ok((w, false));
    }
    let listed = listed()?;
    let hidden: Vec<String> = obj
        .get("hiddenProjects")
        .and_then(|h| h.as_array())
        .map(|a| clean_roots(a.iter().filter_map(|r| r.as_str())))
        .unwrap_or_default();
    let ws = listed.into_iter().filter(|r| !hidden.iter().any(|h| same_root(h, r))).collect();
    obj.remove("hiddenProjects");
    obj.remove("hiddenAt");
    Ok((ws, true))
}

/// Sets `workspace` on the first load (see `workspace_or_migrated`); `home` is the ClaudeGate home
/// the listed roots are read from. Writes nothing when the config already has one.
pub fn ensure_workspace(path: &Path, home: &Path, guard: Option<&Path>) -> Result<Config, String> {
    if let Some(g) = guard {
        if path.starts_with(g) {
            return Err("Refusing to write settings inside the ClaudeGate folder.".into());
        }
    }
    let c = load_or_create(path);
    if c.workspace.is_some() {
        return Ok(c);
    }
    update_config_if(path, guard, |obj, _| {
        let (ws, derived) = workspace_or_migrated(obj, || listed_root_strings(home))?;
        if derived {
            obj.insert("workspace".into(), serde_json::json!(ws));
        }
        Ok(derived)
    })
}

/// Adds only `root` to the workspace, at the end, under the config lock, so a stale list in the
/// UI can never add or drop anything else. Never touches `workspace-roots.json`: what is captured
/// does not change. Writes nothing when it is already there (under any spelling).
pub fn add_to_workspace(path: &Path, home: &Path, root: &str, guard: Option<&Path>) -> Result<Config, String> {
    let root = root.trim().to_owned();
    if root.is_empty() {
        return Err("No project to add.".into());
    }
    update_config_if(path, guard, |obj, _| {
        let (mut ws, derived) = workspace_or_migrated(obj, || listed_root_strings(home))?;
        let added = !ws.iter().any(|r| same_root(r, &root));
        if added {
            ws.push(root.clone());
        }
        if added || derived {
            obj.insert("workspace".into(), serde_json::json!(ws));
        }
        Ok(added || derived)
    })
}

/// Removes only `root` from the workspace (every spelling of it) and the `nested` roots the rail
/// folds into its project (the project's own folded list, so a workspace root that merely sits
/// inside the folder stays), under the config lock. View only: capture is unchanged.
pub fn remove_from_workspace(path: &Path, home: &Path, root: &str, nested: &[String], guard: Option<&Path>) -> Result<Config, String> {
    let gone: Vec<&str> = std::iter::once(root.trim()).chain(nested.iter().map(|n| n.trim())).collect();
    update_config_if(path, guard, |obj, _| {
        let (mut ws, derived) = workspace_or_migrated(obj, || listed_root_strings(home))?;
        let before = ws.len();
        ws.retain(|r| !gone.iter().any(|g| same_root(r, g)));
        let removed = ws.len() != before;
        if removed || derived {
            obj.insert("workspace".into(), serde_json::json!(ws));
        }
        Ok(removed || derived)
    })
}

/// `projects[root]` for `root`: the existing key that resolves to it, else a new one under the
/// resolved spelling. Creates `projects` when missing.
fn project_entry<'a>(
    obj: &'a mut serde_json::Map<String, serde_json::Value>,
    root: &str,
) -> Result<&'a mut serde_json::Map<String, serde_json::Value>, String> {
    let projects = obj.entry("projects").or_insert_with(|| serde_json::json!({}));
    let projects = projects.as_object_mut().ok_or("The projects entry in config.json is not an object.")?;
    let key = projects
        .keys()
        .find(|k| same_root(k, root))
        .cloned()
        .unwrap_or_else(|| claudegate_core::paths::resolve_root(Path::new(root)).to_string_lossy().into_owned());
    let entry = projects.entry(key).or_insert_with(|| serde_json::json!({}));
    entry.as_object_mut().ok_or_else(|| "That project's entry in config.json is not an object.".to_string())
}

/// The app's record of `root` (`projects[root]`: `addedByApp`, `exclude`, `excludeWritten`,
/// `managedByVsCode`). Lenient: unreadable or the wrong shape reads as no record.
pub fn project_record(path: &Path, root: &str) -> claudegate_core::projects::ProjectRecord {
    use claudegate_core::projects::ProjectRecord;
    let Some(v) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return ProjectRecord::default();
    };
    let Some(p) = v
        .get("projects")
        .and_then(|p| p.as_object())
        .and_then(|m| m.iter().find(|(k, _)| same_root(k, root)).map(|(_, p)| p))
    else {
        return ProjectRecord::default();
    };
    let strings = |k: &str| -> Option<Vec<String>> {
        Some(p.get(k)?.as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect())
    };
    ProjectRecord {
        added_by_app: p.get("addedByApp").and_then(|b| b.as_bool()) == Some(true),
        exclude: strings("exclude"),
        exclude_written: strings("excludeWritten"),
        exclude_written_prev: strings("excludeWrittenPrev"),
        exclude_written_at: p.get("excludeWrittenAt").and_then(|t| t.as_i64()),
        managed_by_vscode: p.get("managedByVsCode").and_then(|b| b.as_bool()) == Some(true),
    }
}

/// Records what the app last wrote to `excludes.json[root]` (`projects[root].excludeWritten`),
/// moving the value it replaces to `excludeWrittenPrev`. `excludeWrittenAt` is stamped only when
/// the value changes: it starts the window in which the previous value still counts as the app's,
/// and rewriting the same value (after a stale write-back) must not restart that window.
pub fn set_exclude_written(path: &Path, root: &str, globs: &[String], at_ms: i64, guard: Option<&Path>) -> Result<(), String> {
    update_config(path, guard, |obj, _| {
        let entry = project_entry(obj, root)?;
        let new = serde_json::json!(globs);
        if entry.get("excludeWritten") != Some(&new) {
            match entry.remove("excludeWritten") {
                Some(old) => entry.insert("excludeWrittenPrev".into(), old),
                None => None,
            };
            entry.insert("excludeWritten".into(), new);
            entry.insert("excludeWrittenAt".into(), serde_json::json!(at_ms));
        }
        Ok(())
    })
    .map(|_| ())
}

/// Records (`on`) that VS Code took `excludes.json[root]` over (`projects[root].managedByVsCode`),
/// or drops the mark (Take back).
pub fn set_managed_by_vscode(path: &Path, root: &str, on: bool, guard: Option<&Path>) -> Result<(), String> {
    update_config(path, guard, |obj, _| {
        let entry = project_entry(obj, root)?;
        if on {
            entry.insert("managedByVsCode".into(), serde_json::Value::Bool(true));
        } else {
            entry.remove("managedByVsCode");
        }
        Ok(())
    })
    .map(|_| ())
}

/// Sets the project's own exclude (`projects[root].exclude`); `None` drops it, back to the
/// defaults. The caller has validated the globs and checked the app owns the root.
pub fn save_project_exclude(path: &Path, root: &str, globs: Option<&[String]>, guard: Option<&Path>) -> Result<(), String> {
    update_config(path, guard, |obj, _| {
        let entry = project_entry(obj, root)?;
        match globs {
            Some(g) => entry.insert("exclude".into(), serde_json::json!(g)),
            None => entry.remove("exclude"),
        };
        Ok(())
    })
    .map(|_| ())
}

/// `projects[root]` in config.json, as the core's add/remove/ensure/sync read and write it.
pub struct ConfigProjects<'a> {
    pub path: &'a Path,
    pub guard: Option<&'a Path>,
}

impl claudegate_core::projects::AppProjects for ConfigProjects<'_> {
    fn app_roots(&self) -> Vec<String> {
        app_roots(self.path)
    }
    fn mark_added(&mut self, root: &str) -> Result<(), String> {
        mark_added_by_app(self.path, root, self.guard)
    }
    fn forget(&mut self, root: &str) -> Result<(), String> {
        forget_project(self.path, root, self.guard)
    }
    fn record(&self, root: &str) -> claudegate_core::projects::ProjectRecord {
        project_record(self.path, root)
    }
    fn set_exclude_written(&mut self, root: &str, globs: &[String], at_ms: i64) -> Result<(), String> {
        set_exclude_written(self.path, root, globs, at_ms, self.guard)
    }
    fn set_managed_by_vscode(&mut self, root: &str, on: bool) -> Result<(), String> {
        set_managed_by_vscode(self.path, root, on, self.guard)
    }
}

/// `auditDismissed`: path -> when the user dismissed it (epoch ms). A miss is hidden while its
/// own time is not after that. Lenient and never creates the file.
pub fn read_audit_dismissed(path: &Path) -> std::collections::HashMap<String, i64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            v.get("auditDismissed")?.as_object().map(|m| {
                m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_i64()?))).collect()
            })
        })
        .unwrap_or_default()
}

/// Records `paths` as dismissed at `now_ms` (`auditDismissed[path]`), keeping every other key.
/// Add-only except that entries older than `keep_ms` are pruned on the way, so the map cannot grow
/// without bound. Dismissing hides an audit item in this app only; the audit files are never touched.
pub fn dismiss_audit_paths(path: &Path, paths: &[String], now_ms: i64, keep_ms: i64, guard: Option<&Path>) -> Result<(), String> {
    let paths: Vec<&str> = paths.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect();
    if paths.is_empty() {
        return Ok(());
    }
    update_config(path, guard, |obj, _| {
        let entry = obj.entry("auditDismissed").or_insert_with(|| serde_json::json!({}));
        let map = entry.as_object_mut().ok_or("The auditDismissed entry in config.json is not an object.")?;
        map.retain(|_, v| v.as_i64().is_some_and(|t| t >= now_ms - keep_ms));
        for p in paths {
            map.insert(p.to_string(), serde_json::json!(now_ms));
        }
        Ok(())
    })
    .map(|_| ())
}

/// The app's overrides as the core resolver reads them, plus each project's `extraHide`. Lenient:
/// an unreadable file, or an entry of the wrong shape, reads as no override.
pub fn read_app_settings(path: &Path) -> (claudegate_core::settings::AppSettings, std::collections::HashMap<String, Vec<String>>) {
    use claudegate_core::settings::{AppSettings, ProjectSettings};
    let mut app = AppSettings::default();
    let mut extra = std::collections::HashMap::new();
    let Some(v) = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else {
        return (app, extra);
    };
    if let Some(m) = v.get("settings").and_then(|s| s.as_object()) {
        app.settings = m.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    }
    if let Some(m) = v.get("projects").and_then(|s| s.as_object()) {
        for (root, p) in m {
            let strings = |k: &str| -> Option<Vec<String>> {
                Some(p.get(k)?.as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            };
            app.projects.insert(root.clone(), ProjectSettings { exclude: strings("exclude") });
            if let Some(x) = strings("extraHide") {
                extra.insert(root.clone(), x);
            }
        }
    }
    (app, extra)
}

#[tauri::command]
pub async fn get_config() -> Result<Config, String> {
    // Derives the workspace on the first load; a failed write only delays that.
    tauri::async_runtime::spawn_blocking(|| {
        let p = config_path();
        let home = claudegate_core::paths::claudegate_home();
        ensure_workspace(&p, &home, Some(&home)).unwrap_or_else(|_| load_or_create(&p))
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_editor(template: String) -> Result<Config, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_editor(&config_path(), &template, Some(&claudegate_core::paths::claudegate_home()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_read_only(on: bool) -> Result<Config, String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_read_only(&config_path(), on, Some(&claudegate_core::paths::claudegate_home()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Adds one project to the workspace (`add_to_workspace`). View only, so read-only mode allows it.
#[tauri::command]
pub async fn add_to_workspace_cmd(root: String) -> Result<Config, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = claudegate_core::paths::claudegate_home();
        add_to_workspace(&config_path(), &home, &root, Some(&home))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_missing_fields_fall_back() {
        let c = from_value(&serde_json::json!({ "editor": "  ", "customEditor": "" }));
        assert_eq!(c, Config::default());
        let c = from_value(&serde_json::json!({ "customEditor": "hx {path}:{line}" }));
        assert_eq!(c.editor, DEFAULT_EDITOR);
        assert_eq!(c.custom_editor.as_deref(), Some("hx {path}:{line}"));
    }

    #[test]
    fn serializes_camel_case_without_absent_custom() {
        let v = serde_json::to_value(Config::default()).unwrap();
        assert_eq!(v, serde_json::json!({ "editor": "zed {path}:{line}:{col}", "readOnly": false }));
        let c = Config { editor: VSCODE.into(), custom_editor: Some("x".into()), ..Config::default() };
        assert_eq!(serde_json::to_value(c).unwrap()["customEditor"], "x");
    }
}
