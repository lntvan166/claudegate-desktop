//! The app's own settings, in `~/.claudegate-desktop/config.json`. This is the only file the app
//! writes; it is never under the ClaudeGate home.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const ZED: &str = "zed {path}:{line}";
pub const VSCODE: &str = "code -g {path}:{line}";
pub const DEFAULT_EDITOR: &str = ZED;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Command template for "Open in editor": `{path}` and `{line}` are substituted per argument.
    pub editor: String,
    /// A user-supplied template offered as "Custom…" in the Open-with menu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_editor: Option<String>,
    /// When true every decision is refused. Read from disk on each call, never cached.
    #[serde(default)]
    pub read_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { editor: DEFAULT_EDITOR.into(), custom_editor: None, read_only: false }
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

fn from_value(v: &serde_json::Value) -> Config {
    Config {
        editor: non_empty(v.get("editor").and_then(|e| e.as_str())).unwrap_or_else(|| DEFAULT_EDITOR.into()),
        custom_editor: non_empty(v.get("customEditor").and_then(|e| e.as_str())),
        read_only: v.get("readOnly").and_then(|e| e.as_bool()).unwrap_or(false),
    }
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

/// Reads the config file as a JSON object (missing reads as `{}`), applies `f`, writes it back.
/// Every other key is kept. `guard` is the ClaudeGate home: a config path under it is refused.
fn update_config(
    path: &Path,
    guard: Option<&Path>,
    f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>, &Config) -> Result<(), String>,
) -> Result<Config, String> {
    if let Some(g) = guard {
        if path.starts_with(g) {
            return Err("Refusing to write settings inside the ClaudeGate folder.".into());
        }
    }
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
    f(v.as_object_mut().expect("checked above"), &cfg)?;
    write_json(path, &v).map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
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

#[tauri::command]
pub async fn get_config() -> Result<Config, String> {
    tauri::async_runtime::spawn_blocking(|| load_or_create(&config_path()))
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
        assert_eq!(v, serde_json::json!({ "editor": "zed {path}:{line}", "readOnly": false }));
        let c = Config { editor: VSCODE.into(), custom_editor: Some("x".into()), read_only: false };
        assert_eq!(serde_json::to_value(c).unwrap()["customEditor"], "x");
    }
}
