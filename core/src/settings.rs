//! Read-only settings resolver: the app's own value, then the VS Code extension's, then the
//! extension's built-in default (copied from tag v1.16.1; `xcheck` asserts they still match).
//! The core never reads the app's config file: the caller passes it in as `AppSettings`.

use crate::jsonc::parse_jsonc;
use crate::paths::resolve_root;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/package-lock.json", "**/yarn.lock", "**/pnpm-lock.yaml", "**/npm-shrinkwrap.json",
    "**/bun.lockb", "**/Cargo.lock", "**/poetry.lock", "**/Pipfile.lock", "**/Gemfile.lock",
    "**/composer.lock", "**/go.sum", "**/*.min.js", "**/*.min.css", "**/*.map", "**/node_modules/**",
];
pub const DEFAULT_PROTECTED: &[&str] = &[
    "**/.env", "**/.env.*", "**/*.pem", "**/*.key", "**/*.p12", "**/*.pfx",
    "**/id_rsa", "**/id_ed25519", "**/.npmrc", "**/credentials",
];
pub const DEFAULT_AUTO_ADVANCE: bool = true;
pub const DEFAULT_GROUP_BY_SESSION: bool = false;
pub const DEFAULT_PENDING_AGE_MIN_DAYS: u32 = 0;
pub const DEFAULT_MAX_ATTACHED: u32 = 256;
pub const DEFAULT_HOOK_LOG: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    App,
    VsCode,
    Cursor,
    Workspace,
    Default,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved<T> {
    pub value: T,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effective {
    pub exclude: Resolved<Vec<String>>,
    pub protected: Resolved<Vec<String>>,
    pub auto_advance: Resolved<bool>,
    pub group_by_session: Resolved<bool>,
    pub pending_age_min_days: Resolved<u32>,
    pub max_attached: Resolved<u32>,
    pub hook_log: Resolved<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProjectSettings {
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

/// The app's own overrides. Keys of `settings` are the extension's names without the
/// `claudegate.` prefix (`autoAdvance`, `pendingAge.minDays`, ...). `projects` is keyed by root.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub settings: HashMap<String, Value>,
    #[serde(default)]
    pub projects: HashMap<String, ProjectSettings>,
}

#[derive(Debug, Clone)]
pub struct ExtSources {
    pub vscode_user: Option<PathBuf>,
    pub cursor_user: Option<PathBuf>,
    pub excludes_json: PathBuf,
    pub hooklog_sentinel: PathBuf,
}

pub fn default_ext_sources(home: &Path) -> ExtSources {
    let user = |app: &str| -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        let base = home.join("Library/Application Support");
        #[cfg(target_os = "windows")]
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Roaming"));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let base = home.join(".config");
        Some(base.join(app).join("User").join("settings.json"))
    };
    ExtSources {
        vscode_user: user("Code"),
        cursor_user: user("Cursor"),
        excludes_json: home.join(".claudegate").join("excludes.json"),
        hooklog_sentinel: home.join(".claudegate").join("hooklog.enabled"),
    }
}

/// A parsed settings file, only if it is a JSON(C) object.
fn read_object(path: &Path) -> Option<serde_json::Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    match parse_jsonc(&text)? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

/// `{glob: bool}` -> globs mapped to `true`. Globs come back sorted: serde_json's map is a
/// BTreeMap here, and glob order never affects matching.
fn globs_from_map(v: &Value) -> Option<Vec<String>> {
    let m = v.as_object()?;
    Some(m.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect())
}

fn as_bool(v: &Value) -> Option<bool> {
    v.as_bool()
}
fn as_u32(min: u32) -> impl Fn(&Value) -> Option<u32> {
    move |v| {
        let f = v.as_f64()?;
        if f.fract() != 0.0 || f < min as f64 || f > u32::MAX as f64 {
            return None;
        }
        Some(f as u32)
    }
}

struct Layers {
    workspace: Option<serde_json::Map<String, Value>>,
    vscode: Option<serde_json::Map<String, Value>>,
    cursor: Option<serde_json::Map<String, Value>>,
}

impl Layers {
    /// Object-valued settings merge key by key like VS Code: defaults, then Cursor user, VS Code
    /// user, workspace (later wins); keep keys whose final value is `true`. The source is the
    /// highest layer that contributed any key, else Default.
    fn merged_globs(&self, key: &str, defaults: &[&str]) -> Resolved<Vec<String>> {
        let mut map: std::collections::BTreeMap<String, bool> = defaults.iter().map(|d| (d.to_string(), true)).collect();
        let mut source = Source::Default;
        for (layer, src) in [(&self.cursor, Source::Cursor), (&self.vscode, Source::VsCode), (&self.workspace, Source::Workspace)] {
            if let Some(m) = layer.as_ref().and_then(|m| m.get(key)).and_then(Value::as_object) {
                for (k, v) in m {
                    if let Some(b) = v.as_bool() {
                        map.insert(k.clone(), b);
                        source = src;
                    }
                }
            }
        }
        Resolved { value: map.into_iter().filter(|(_, v)| *v).map(|(k, _)| k).collect(), source }
    }

    /// app -> workspace -> VS Code -> Cursor -> default, skipping values of the wrong type.
    fn resolve<T>(&self, app: &AppSettings, key: &str, parse: impl Fn(&Value) -> Option<T>, default: T) -> Resolved<T> {
        if let Some(v) = app.settings.get(key).and_then(&parse) {
            return Resolved { value: v, source: Source::App };
        }
        let full = format!("claudegate.{key}");
        for (layer, source) in [
            (&self.workspace, Source::Workspace),
            (&self.vscode, Source::VsCode),
            (&self.cursor, Source::Cursor),
        ] {
            if let Some(v) = layer.as_ref().and_then(|m| m.get(&full)).and_then(&parse) {
                return Resolved { value: v, source };
            }
        }
        Resolved { value: default, source: Source::Default }
    }
}

pub fn effective(app: &AppSettings, root: Option<&Path>, sources: &ExtSources) -> Effective {
    let layers = Layers {
        workspace: root.and_then(|r| read_object(&r.join(".vscode").join("settings.json"))),
        vscode: sources.vscode_user.as_deref().and_then(read_object),
        cursor: sources.cursor_user.as_deref().and_then(read_object),
    };

    let exclude = resolve_exclude(app, root, sources, &layers);
    let protected = match app.settings.get("protected").and_then(|v| globs_from_map(v).or_else(|| strings(v))) {
        Some(value) => Resolved { value, source: Source::App },
        None => layers.merged_globs("claudegate.protected", DEFAULT_PROTECTED),
    };

    // The sentinel is shared with the extension, so it is neither "from VS Code" nor a default:
    // the app owns toggling it, and its presence is the value.
    let hook_log = if sources.hooklog_sentinel.exists() {
        Resolved { value: true, source: Source::App }
    } else {
        Resolved { value: DEFAULT_HOOK_LOG, source: Source::Default }
    };

    Effective {
        exclude,
        protected,
        auto_advance: layers.resolve(app, "autoAdvance", as_bool, DEFAULT_AUTO_ADVANCE),
        group_by_session: layers.resolve(app, "groupBySession", as_bool, DEFAULT_GROUP_BY_SESSION),
        pending_age_min_days: layers.resolve(app, "pendingAge.minDays", as_u32(0), DEFAULT_PENDING_AGE_MIN_DAYS),
        max_attached: layers.resolve(app, "worktrees.maxAttached", as_u32(1), DEFAULT_MAX_ATTACHED),
        hook_log,
    }
}

fn strings(v: &Value) -> Option<Vec<String>> {
    // Non-string elements are dropped, not fatal, like the extension.
    Some(v.as_array()?.iter().filter_map(|x| x.as_str().map(String::from)).collect())
}

/// App project exclude (full replacement), else `excludes.json[root]`, else the merged settings
/// layers. The `excludes.json` hit wins over the settings files because the extension writes its
/// already-merged effective list there, so it is the more authoritative copy for that root.
fn resolve_exclude(app: &AppSettings, root: Option<&Path>, sources: &ExtSources, layers: &Layers) -> Resolved<Vec<String>> {
    if let Some(root) = root {
        let key = resolve_root(root).to_string_lossy().into_owned();
        let direct = root.to_string_lossy().into_owned();
        if let Some(g) = app.projects.get(&key).or_else(|| app.projects.get(&direct)).and_then(|p| p.exclude.clone()) {
            return Resolved { value: g, source: Source::App };
        }
        if let Some(Value::Object(m)) = fs::read_to_string(&sources.excludes_json).ok().and_then(|t| serde_json::from_str(&t).ok()) {
            if let Some(g) = m.get(&key).and_then(strings) {
                return Resolved { value: g, source: Source::VsCode };
            }
        }
    }
    layers.merged_globs("claudegate.exclude", DEFAULT_EXCLUDES)
}
