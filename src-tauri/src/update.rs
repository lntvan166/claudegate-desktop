//! Checking GitHub Releases for a newer app, and installing it where the updater can.
//!
//! The updater replaces a macOS `.app` or a Linux AppImage in place. A `.deb` install lives in
//! `/usr/bin`, owned by dpkg, so the app never touches it: it shows the command that installs the
//! new package instead, for the user to run. Updating the app writes no shared ClaudeGate state,
//! so read-only mode does not apply.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

const RELEASES: &str = "https://github.com/lntvan166/claudegate-desktop/releases/download";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub version: String,
    pub current: String,
    pub notes: Option<String>,
    /// The updater can replace this install in place (macOS, AppImage).
    pub can_install: bool,
    /// For a `.deb` install: a shell command that downloads and installs the new package.
    pub command: Option<String>,
}

/// Whether the updater can replace this install. On Linux only an AppImage can be, and the
/// AppImage runtime marks itself with `$APPIMAGE`.
pub fn self_updates(linux: bool, appimage: Option<&str>) -> bool {
    !linux || appimage.is_some_and(|p| !p.is_empty())
}

/// The command that installs `version`'s `.deb` from the release.
pub fn deb_command(version: &str) -> String {
    let file = format!("ClaudeGate_{version}_amd64.deb");
    format!("curl -fL -o ~/Downloads/{file} {RELEASES}/v{version}/{file} && pkexec apt install -y ~/Downloads/{file}")
}

pub fn view(version: &str, current: &str, notes: Option<String>, can_install: bool) -> UpdateView {
    UpdateView {
        version: version.to_string(),
        current: current.to_string(),
        notes: notes.filter(|n| !n.trim().is_empty()),
        can_install,
        command: (!can_install).then(|| deb_command(version)),
    }
}

fn this_install_self_updates() -> bool {
    self_updates(cfg!(target_os = "linux"), std::env::var("APPIMAGE").ok().as_deref())
}

/// The newer release, if there is one. `None` when this is the latest.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateView>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    Ok(update.map(|u| view(&u.version, &u.current_version, u.body.clone(), this_install_self_updates())))
}

/// Download, verify the signature, install, and restart. Refused where the updater cannot
/// replace the install.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    if !this_install_self_updates() {
        return Err("This install came from a .deb package; run the command shown to update it.".into());
    }
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("You already have the latest version.")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_appimage_self_updates_on_linux() {
        assert!(!self_updates(true, None));
        assert!(!self_updates(true, Some("")));
        assert!(self_updates(true, Some("/home/u/ClaudeGate.AppImage")));
        assert!(self_updates(false, None));
    }

    #[test]
    fn deb_install_gets_a_command_not_a_button() {
        let v = view("0.3.0", "0.2.0", Some("  ".into()), false);
        assert_eq!(v.notes, None);
        assert_eq!(
            v.command.as_deref(),
            Some(
                "curl -fL -o ~/Downloads/ClaudeGate_0.3.0_amd64.deb \
                 https://github.com/lntvan166/claudegate-desktop/releases/download/v0.3.0/ClaudeGate_0.3.0_amd64.deb \
                 && pkexec apt install -y ~/Downloads/ClaudeGate_0.3.0_amd64.deb"
            )
        );
        let mac = view("0.3.0", "0.2.0", Some("Fixes".into()), true);
        assert_eq!(mac.command, None);
        assert_eq!(mac.notes.as_deref(), Some("Fixes"));
    }
}
