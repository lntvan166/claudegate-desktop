pub mod capture;
pub mod commands;
pub mod config;
pub mod editor;
pub mod gitlog;
pub mod launch;
pub mod tray;
pub mod update;
pub mod upkeep;
pub mod watch;
pub mod window_state;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        // First: a second launch must hand over its argv and exit before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| launch::second_instance(app, &args, &cwd)))
        // The window's size, position and maximised state, kept across restarts (see window_state).
        .plugin(tauri_plugin_window_state::Builder::new().with_state_flags(window_state::flags()).build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(launch::LaunchState::default())
        .manage(watch::KnownMap::default())
        .manage(commands::WriteLock::default())
        .manage(upkeep::UpkeepState::default())
        .manage(gitlog::GitLogState::default())
        .setup(|app| {
            launch::remember(app.handle(), &std::env::args().collect::<Vec<_>>());
            let known = app.state::<watch::KnownMap>().inner().clone();
            // Re-add any of the app's projects the extension dropped while we were not running.
            commands::ensure_roots_with(&claudegate_core::paths::claudegate_home(), &config::config_path());
            // Hand the hook the exclude of the app's own projects; notice a VS Code takeover.
            commands::sync_excludes_with(&claudegate_core::paths::claudegate_home(), &config::config_path());
            watch::start(app.handle().clone(), claudegate_core::paths::claudegate_home(), known);
            tray::setup(app.handle());
            window_state::save_as_it_changes(app.handle());
            // Bring the app's own ~/.claudegate/hook.py up to the bundled version (never the
            // extension's; never settings.json). Off the main thread: it runs python3 --version.
            std::thread::spawn(capture::update_hook_at_startup);
            Ok(())
        })
        .on_window_event(|window, event| {
            // A closed window's git log query and its children go with it.
            if let tauri::WindowEvent::Destroyed = event {
                window.state::<gitlog::GitLogState>().0.forget_window(window.label());
            }
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
            commands::remove_from_workspace,
            commands::pick_folder,
            capture::capture_status,
            capture::set_up_capture,
            capture::update_capture_hook,
            commands::project_in_vs_code,
            commands::set_project_exclude,
            commands::take_back_exclude,
            config::get_config,
            config::set_editor,
            config::set_read_only,
            config::add_to_workspace_cmd,
            editor::open_in_editor,
            gitlog::gitlog_repos,
            config::gitlog_config,
            config::gitlog_tick,
            config::gitlog_untick,
            config::gitlog_show_all,
            config::gitlog_show_only,
            config::gitlog_profile_apply,
            config::gitlog_profile_save,
            config::gitlog_profile_rename,
            config::gitlog_profile_delete,
            config::gitlog_scan_depth,
            config::gitlog_exclude,
            config::gitlog_include,
            config::gitlog_last_filter,
            config::gitlog_pane,
            gitlog::gitlog_page,
            gitlog::gitlog_changes,
            gitlog::gitlog_file,
            gitlog::gitlog_worktree_file,
            gitlog::gitlog_message,
            gitlog::gitlog_open_remote,
            tray::set_pending_count,
            update::check_update,
            update::install_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClaudeGate Desktop");
}
