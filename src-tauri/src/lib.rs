pub mod capture;
pub mod commands;
pub mod config;
pub mod editor;
pub mod launch;
pub mod tray;
pub mod update;
pub mod upkeep;
pub mod watch;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        // First: a second launch must hand over its argv and exit before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| launch::second_instance(app, &args, &cwd)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(launch::LaunchState::default())
        .manage(watch::KnownMap::default())
        .manage(commands::WriteLock::default())
        .manage(upkeep::UpkeepState::default())
        .setup(|app| {
            launch::remember(app.handle(), &std::env::args().collect::<Vec<_>>());
            let known = app.state::<watch::KnownMap>().inner().clone();
            // Re-add any of the app's projects the extension dropped while we were not running.
            commands::ensure_roots_with(&claudegate_core::paths::claudegate_home(), &config::config_path());
            // Hand the hook the exclude of the app's own projects; notice a VS Code takeover.
            commands::sync_excludes_with(&claudegate_core::paths::claudegate_home(), &config::config_path());
            watch::start(app.handle().clone(), claudegate_core::paths::claudegate_home(), known);
            tray::setup(app.handle());
            // Bring the app's own ~/.claudegate/hook.py up to the bundled version (never the
            // extension's; never settings.json). Off the main thread: it runs python3 --version.
            std::thread::spawn(capture::update_hook_at_startup);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            launch::launch_path,
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
            capture::capture_status,
            capture::set_up_capture,
            commands::project_in_vs_code,
            commands::set_project_exclude,
            commands::take_back_exclude,
            config::get_config,
            config::set_editor,
            config::set_read_only,
            config::set_hidden_projects,
            editor::open_in_editor,
            tray::set_pending_count,
            update::check_update,
            update::install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClaudeGate Desktop");
}
