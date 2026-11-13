//! "Open in editor": render the configured command template into argv and spawn it directly,
//! never through a shell. A desktop-launched app usually has a bare PATH, so the program is
//! looked up on PATH first and then in the usual install directories.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Splits `template` on whitespace **before** substituting `{path}` and `{line}`, so a path with
/// spaces stays one argument and nothing in it is re-read as a placeholder. A template that never
/// mentions `{path}` gets the path appended as the last argument.
pub fn render_editor_command(template: &str, path: &str, line: u32) -> Vec<String> {
    let line = line.max(1).to_string();
    let mut saw_path = false;
    let mut argv: Vec<String> = template
        .split_whitespace()
        .map(|tok| {
            saw_path |= tok.contains("{path}");
            // Substitute both in one left-to-right pass so text from `path` is never re-scanned.
            let mut out = String::with_capacity(tok.len() + path.len());
            let mut rest = tok;
            while let Some(i) = rest.find('{') {
                out.push_str(&rest[..i]);
                let tail = &rest[i..];
                if let Some(t) = tail.strip_prefix("{path}") {
                    out.push_str(path);
                    rest = t;
                } else if let Some(t) = tail.strip_prefix("{line}") {
                    out.push_str(&line);
                    rest = t;
                } else {
                    out.push('{');
                    rest = &tail[1..];
                }
            }
            out.push_str(rest);
            out
        })
        .collect();
    if !saw_path && !argv.is_empty() {
        argv.push(path.to_string());
    }
    argv
}

/// Install directories searched after PATH, in order.
pub fn fallback_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(h) = home {
        v.push(h.join(".local/bin"));
    }
    for d in ["/usr/local/bin", "/usr/bin", "/snap/bin", "/opt/homebrew/bin"] {
        v.push(PathBuf::from(d));
    }
    v
}

/// The CLIs bundled inside the macOS app bundles of Zed and VS Code, for `program`.
pub fn mac_bundled(program: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let rel: &[&str] = match program {
        "zed" => &["Zed.app/Contents/MacOS/cli", "Zed Preview.app/Contents/MacOS/cli"],
        "code" => &["Visual Studio Code.app/Contents/Resources/app/bin/code"],
        "code-insiders" => &["Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code-insiders"],
        _ => &[],
    };
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(h) = home {
        roots.push(h.join("Applications"));
    }
    roots
        .iter()
        .flat_map(|r| rel.iter().map(move |p| r.join(p)))
        .collect()
}

fn names_for(program: &str) -> Vec<String> {
    if cfg!(windows) && Path::new(program).extension().is_none() {
        [".exe", ".cmd", ".bat"].iter().map(|e| format!("{program}{e}")).collect()
    } else {
        vec![program.to_string()]
    }
}

/// Finds `program`: as given when it is a path, else in each PATH entry, then in each of `extra`.
pub fn resolve_program(
    program: &str,
    path_var: Option<&OsStr>,
    extra: &[PathBuf],
    is_exec: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    let p = Path::new(program);
    if p.components().count() > 1 || p.is_absolute() {
        return is_exec(p).then(|| p.to_path_buf());
    }
    let mut dirs: Vec<PathBuf> = path_var.map(|v| std::env::split_paths(v).collect()).unwrap_or_default();
    dirs.extend(extra.iter().cloned());
    for d in &dirs {
        for n in names_for(program) {
            let c = d.join(&n);
            if is_exec(&c) {
                return Some(c);
            }
        }
    }
    None
}

/// [`resolve_program`], then the first existing app-bundled CLI (macOS), for bare names only.
pub fn resolve_with_bundles(
    program: &str,
    path_var: Option<&OsStr>,
    extra: &[PathBuf],
    bundled: &[PathBuf],
    is_exec: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    resolve_program(program, path_var, extra, is_exec).or_else(|| bundled.iter().find(|b| is_exec(b)).cloned())
}

fn is_executable(p: &Path) -> bool {
    let Ok(m) = std::fs::metadata(p) else { return false };
    if !m.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn expand_home(program: &str, home: Option<&Path>) -> String {
    match (program.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest).to_string_lossy().into_owned(),
        _ => program.to_string(),
    }
}

fn label(program: &str) -> &str {
    match program {
        "zed" => "Zed",
        "code" => "VS Code",
        other => other,
    }
}

/// Resolves and spawns the editor for `template`. Returns once the process has started.
pub fn launch(template: &str, path: &str, line: u32) -> Result<(), String> {
    let mut argv = render_editor_command(template, path, line);
    if argv.is_empty() {
        return Err("The editor command is empty. Choose an editor from the Open with menu.".into());
    }
    let home = dirs::home_dir();
    argv[0] = expand_home(&argv[0], home.as_deref());
    let extra = fallback_dirs(home.as_deref());
    let bundled = if cfg!(target_os = "macos") { mac_bundled(&argv[0], home.as_deref()) } else { vec![] };
    let path_var = std::env::var_os("PATH");
    let bin = resolve_with_bundles(&argv[0], path_var.as_deref(), &extra, &bundled, &is_executable).ok_or_else(|| {
        format!(
            "Couldn't find {} (`{}`). Install its command-line tool, or pick another editor.",
            label(&argv[0]),
            argv[0]
        )
    })?;
    // Give the editor's own launcher script the same search path we used to find it.
    let mut search: Vec<PathBuf> = path_var.as_deref().map(|v| std::env::split_paths(v).collect()).unwrap_or_default();
    search.extend(extra.into_iter().filter(|d| d.is_dir()));
    let joined: OsString = std::env::join_paths(search).unwrap_or_default();
    let mut child = Command::new(&bin)
        .args(&argv[1..])
        .env("PATH", joined)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't start {}: {e}", bin.display()))?;
    // Reap it so it never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[tauri::command]
pub async fn open_in_editor(path: String, line: u32, template: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let t = match template.filter(|t| !t.trim().is_empty()) {
            Some(t) => t,
            None => crate::config::load_or_create(&crate::config::config_path()).editor,
        };
        launch(&t, &path, line)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_zero_is_clamped_to_one() {
        assert_eq!(render_editor_command("zed {path}:{line}", "/f", 0), vec!["zed", "/f:1"]);
    }

    #[test]
    fn unknown_braces_are_kept() {
        assert_eq!(render_editor_command("ed {x} {path}", "/f", 2), vec!["ed", "{x}", "/f"]);
    }

    #[test]
    fn fallback_dirs_cover_the_usual_installs() {
        let d = fallback_dirs(Some(Path::new("/home/u")));
        assert_eq!(d[0], PathBuf::from("/home/u/.local/bin"));
        for want in ["/usr/local/bin", "/usr/bin", "/snap/bin", "/opt/homebrew/bin"] {
            assert!(d.contains(&PathBuf::from(want)), "{want}");
        }
    }

    #[test]
    fn mac_bundles_are_named_for_zed_and_code() {
        let z = mac_bundled("zed", None);
        assert!(z.contains(&PathBuf::from("/Applications/Zed.app/Contents/MacOS/cli")));
        let c = mac_bundled("code", Some(Path::new("/Users/u")));
        assert!(c.contains(&PathBuf::from(
            "/Users/u/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
        )));
        assert!(mac_bundled("vim", None).is_empty());
    }

    #[test]
    fn home_is_expanded_only_for_a_leading_tilde() {
        assert_eq!(expand_home("~/bin/ed", Some(Path::new("/h"))), "/h/bin/ed");
        assert_eq!(expand_home("zed", Some(Path::new("/h"))), "zed");
    }
}
