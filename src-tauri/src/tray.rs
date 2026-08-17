use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::error::Result;
use crate::notify;

pub const TRAY_ID: &str = "workbench";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Workbench", true, None::<&str>)?;
    let test = MenuItem::with_id(app, "test", "Send test notification", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Workbench", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(app, &[&open, &test, &sep, &quit])?;

    #[allow(unused_mut)]
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray.png"
        ))?)
        .tooltip("Workbench")
        .menu(&menu);

    // macOS renders a template image to match the menu bar in either theme, and
    // reserves left-click for us so the window can toggle from the icon.
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true).show_menu_on_left_click(false);
    }

    builder
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            // Deliberately reachable with the window closed — that is the
            // whole point of M0.
            "test" => notify::send(app, "Workbench", "Notifications work with the window closed."),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn toggle_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

/// Puts the "needs you" count beside the menu-bar icon. Zero shows nothing at
/// all — an empty desk should look empty.
pub fn set_badge<R: Runtime>(app: &AppHandle<R>, count: i64) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let label = if count > 0 { Some(count.to_string()) } else { None };
    let _ = tray.set_title(label.as_deref());
    let _ = tray.set_tooltip(Some(if count > 0 {
        format!("Workbench — {count} waiting on you")
    } else {
        "Workbench".to_string()
    }));
}
