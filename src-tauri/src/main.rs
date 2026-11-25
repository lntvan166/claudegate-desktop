// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // GNOME on Wayland lets no app place its own window; through XWayland the remembered position
    // can be restored. Set before GTK starts. See window_state.rs.
    #[cfg(target_os = "linux")]
    {
        let var = |k: &str| std::env::var(k).ok();
        if claudegate_desktop_lib::window_state::prefer_x11(
            var("XDG_SESSION_TYPE").as_deref(),
            var("GDK_BACKEND").as_deref(),
            var("CLAUDEGATE_WAYLAND").as_deref(),
        ) {
            std::env::set_var("GDK_BACKEND", "x11");
            // XWayland can refuse us (a stale XAUTHORITY after the session restarted, no Xwayland
            // at all); GTK would then fail to start and the app would panic. Probe it here and
            // fall back to native Wayland, which only loses the remembered position.
            if gtk::init().is_err() {
                std::env::set_var("GDK_BACKEND", "wayland");
            }
        }
    }
    claudegate_desktop_lib::run()
}
