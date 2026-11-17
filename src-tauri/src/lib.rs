pub mod commands;
pub mod config;
pub mod editor;
pub mod tray;
pub mod watch;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .manage(watch::KnownMap::default())
        .manage(commands::WriteLock::default())
        .setup(|app| {
            let known = app.state::<watch::KnownMap>().inner().clone();
            watch::start(app.handle().clone(), claudegate_core::paths::claudegate_home(), known);
            tray::setup(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::get_project,
            commands::get_diff,
            commands::decide,
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
