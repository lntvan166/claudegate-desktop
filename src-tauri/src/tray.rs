//! Tray icon: tooltip `ClaudeGate — N pending`, menu Show / Quit. The UI pushes the total it
//! already derives from `list_projects` after every `claudegate://changed`, so the tray never
//! re-reads session files on its own. When no tray is available (no appindicator on Linux, a
//! headless session) it is logged and the app carries on.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

pub const TRAY_ID: &str = "claudegate";
const COUNT_ID: &str = "count";

pub fn tooltip(pending: usize) -> String {
    format!("ClaudeGate — {pending} pending")
}

/// Menu line for the count: Linux tray hosts do not show tooltips, so the figure is also here.
pub fn count_label(pending: usize) -> String {
    if pending == 0 {
        "Nothing pending".into()
    } else {
        format!("{pending} pending")
    }
}

fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let count = MenuItem::with_id(app, COUNT_ID, count_label(0), false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&count, &show, &quit])?;
    app.manage(CountItem(count));
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(0))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

struct CountItem<R: Runtime>(MenuItem<R>);

/// Creates the tray. Never fails the app: errors and panics from the platform layer are logged.
pub fn setup<R: Runtime>(app: &AppHandle<R>) {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(app)));
    match r {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("claudegate: tray unavailable: {e}"),
        Err(_) => eprintln!("claudegate: tray unavailable (platform library missing)"),
    }
}

#[tauri::command]
pub fn set_pending_count<R: Runtime>(app: AppHandle<R>, count: usize) {
    if let Some(t) = app.tray_by_id(TRAY_ID) {
        let _ = t.set_tooltip(Some(tooltip(count)));
    }
    if let Some(item) = app.try_state::<CountItem<R>>() {
        let _ = item.0.set_text(count_label(count));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_names_the_total() {
        assert_eq!(tooltip(0), "ClaudeGate — 0 pending");
        assert_eq!(tooltip(12), "ClaudeGate — 12 pending");
    }

    #[test]
    fn count_label_reads_naturally() {
        assert_eq!(count_label(0), "Nothing pending");
        assert_eq!(count_label(3), "3 pending");
    }
}
