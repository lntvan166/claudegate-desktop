//! Remembering the window's size and place across restarts.
//!
//! `tauri-plugin-window-state` writes its file only when the app quits cleanly, so a window that
//! was resized and then lost to a logout, a crash or a `kill` came back at the default size every
//! time. The state is saved here as it changes instead, after the window has been still for a
//! moment.
//!
//! GNOME on Wayland lets no app place its own window, so on a Wayland session the app runs
//! through XWayland (`prefer_x11`) where the remembered position can be restored. The plugin
//! skips a saved position that is on no current monitor, so an unplugged screen can't strand the
//! window. `CLAUDEGATE_WAYLAND=1` keeps the native Wayland backend (size only).

use std::sync::mpsc;
use std::time::Duration;

use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

/// What is remembered: size, position, and whether the window is maximised.
pub fn flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

/// How long the window must be still before its state is written.
const SETTLE: Duration = Duration::from_millis(600);

/// Whether to run through XWayland: on a Wayland session, unless a GDK backend was chosen
/// explicitly or `CLAUDEGATE_WAYLAND` asks for native Wayland.
pub fn prefer_x11(session: Option<&str>, gdk_backend: Option<&str>, opt_out: Option<&str>) -> bool {
    let set = |v: Option<&str>| v.is_some_and(|s| !s.is_empty() && s != "0");
    session == Some("wayland") && !gdk_backend.is_some_and(|b| !b.is_empty()) && !set(opt_out)
}

/// Save the main window's state once it has stopped moving or resizing for `SETTLE`.
pub fn save_as_it_changes(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    let (tx, rx) = mpsc::channel::<()>();
    let handle = app.clone();
    std::thread::spawn(move || {
        // Wait for a change, then for the window to be still, then save once.
        while rx.recv().is_ok() {
            loop {
                match rx.recv_timeout(SETTLE) {
                    Ok(()) => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let _ = handle.save_window_state(flags());
                        break;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    });
    window.on_window_event(move |e| {
        if matches!(e, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
            let _ = tx.send(());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::prefer_x11;

    #[test]
    fn x11_only_on_wayland_without_an_explicit_choice() {
        assert!(prefer_x11(Some("wayland"), None, None));
        assert!(prefer_x11(Some("wayland"), Some(""), Some("0")));
        assert!(!prefer_x11(Some("x11"), None, None));
        assert!(!prefer_x11(None, None, None));
        assert!(!prefer_x11(Some("wayland"), Some("wayland"), None));
        assert!(!prefer_x11(Some("wayland"), None, Some("1")));
    }
}
