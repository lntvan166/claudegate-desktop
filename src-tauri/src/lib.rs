pub mod commands;
pub mod config;
pub mod editor;
pub mod tray;
pub mod watch;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(watch::KnownMap::default())
        .manage(commands::WriteLock::default())
        .setup(|app| {
            let known = app.state::<watch::KnownMap>().inner().clone();
            // Re-add any of the app's projects the extension dropped while we were not running.
            commands::ensure_roots_with(&claudegate_core::paths::claudegate_home(), &config::config_path());
            // Hand the hook the exclude of the app's own projects; notice a VS Code takeover.
            commands::sync_excludes_with(&claudegate_core::paths::claudegate_home(), &config::config_path());
            watch::start(app.handle().clone(), claudegate_core::paths::claudegate_home(), known);
            tray::setup(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::get_project,
            commands::get_diff,
            commands::decide,
            commands::get_settings,
            commands::set_setting,
            commands::set_hook_log,
            commands::set_extra_hide,
            commands::add_project,
            commands::remove_project,
            commands::pick_folder,
            commands::project_in_vs_code,
            commands::set_project_exclude,
            config::get_config,
            config::set_editor,
            config::set_read_only,
            config::set_hidden_projects,
            editor::open_in_editor,
            tray::set_pending_count
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClaudeGate Desktop");
}
