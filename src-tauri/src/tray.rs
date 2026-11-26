//! Tray icon: tooltip `ClaudeGate — N pending`, menu Show / Quit. N counts the workspace only;
//! pending work in known projects outside it is added as ` · +M elsewhere`. The UI pushes both
//! totals it already derives from `list_projects` after every `claudegate://changed`, so the tray never
//! re-reads session files on its own. When no tray is available (no appindicator on Linux, a
//! headless session) it is logged and the app carries on.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

pub const TRAY_ID: &str = "claudegate";
const COUNT_ID: &str = "count";

/// ` · +M elsewhere`, or nothing when no known project outside the workspace has pending work.
fn elsewhere_suffix(elsewhere: usize) -> String {
    if elsewhere == 0 {
        String::new()
    } else {
        format!(" · +{elsewhere} elsewhere")
    }
}

pub fn tooltip(pending: usize, elsewhere: usize) -> String {
    format!("ClaudeGate — {pending} pending{}", elsewhere_suffix(elsewhere))
}

/// Menu line for the count: Linux tray hosts do not show tooltips, so the figure is also here.
pub fn count_label(pending: usize, elsewhere: usize) -> String {
    let n = if pending == 0 { "Nothing pending".into() } else { format!("{pending} pending") };
    n + &elsewhere_suffix(elsewhere)
}

fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let count = MenuItem::with_id(app, COUNT_ID, count_label(0, 0), false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&count, &show, &quit])?;
    app.manage(CountItem(count));
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(0, 0))
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
pub fn set_pending_count<R: Runtime>(app: AppHandle<R>, count: usize, elsewhere: Option<usize>) {
    let elsewhere = elsewhere.unwrap_or(0);
    if let Some(t) = app.tray_by_id(TRAY_ID) {
        let _ = t.set_tooltip(Some(tooltip(count, elsewhere)));
    }
    if let Some(item) = app.try_state::<CountItem<R>>() {
        let _ = item.0.set_text(count_label(count, elsewhere));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_names_the_total() {
        assert_eq!(tooltip(0, 0), "ClaudeGate — 0 pending");
        assert_eq!(tooltip(12, 0), "ClaudeGate — 12 pending");
    }

    #[test]
    fn the_total_is_the_workspace_plus_what_waits_elsewhere() {
        assert_eq!(tooltip(5, 7), "ClaudeGate — 5 pending · +7 elsewhere");
        assert_eq!(count_label(5, 7), "5 pending · +7 elsewhere");
        assert_eq!(count_label(0, 7), "Nothing pending · +7 elsewhere");
    }

    #[test]
    fn count_label_reads_naturally() {
        assert_eq!(count_label(0, 0), "Nothing pending");
        assert_eq!(count_label(3, 0), "3 pending");
    }
}
