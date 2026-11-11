//! Tray icon: left click toggles the main window, right click opens the menu.
use crate::state::{lock, shell};
use crate::win;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

pub const TRAY_ID: &str = "main";

/// The autostart check item, kept so it can reflect the real state.
pub struct TrayItems {
    pub autostart: CheckMenuItem<Wry>,
}

pub fn autostart_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let al = app.autolaunch();
    let r = if enabled { al.enable() } else { al.disable() };
    let out = r.map_err(|e| format!("设置开机自启失败：{e}"));
    if enabled && out.is_ok() {
        quote_run_entry();
    }
    sync_autostart(app);
    out
}

/// Rewrite the autostart entry as `"<exe>" --autostart` (the plugin leaves the path
/// unquoted, which is fragile for a path with spaces).
pub fn quote_run_entry() {
    if let Ok(exe) = std::env::current_exe() {
        let command = format!("\"{}\" --autostart", exe.display());
        if !crate::sys::set_run_entry("DayNote", &command) {
            eprintln!("DayNote: could not rewrite the autostart entry");
        }
    }
}

pub fn sync_autostart(app: &AppHandle) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.autostart.set_checked(autostart_enabled(app));
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开 DayNote", true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quickadd", "速记", true, None::<&str>)?;
    let auto = CheckMenuItem::with_id(
        app,
        "autostart",
        "开机自启",
        true,
        autostart_enabled(app),
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quick, &auto, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("DayNote")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => win::show_main(app),
            "quickadd" => win::quickadd_toggle(app),
            "autostart" => {
                let want = !autostart_enabled(app);
                if let Err(e) = set_autostart(app, want) {
                    eprintln!("DayNote: {e}");
                }
                crate::state::app_state_changed(app);
            }
            "quit" => crate::commands::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                win::toggle_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(TrayItems { autostart: auto });
    Ok(())
}

pub fn tip_text(open_today: usize) -> String {
    if open_today == 0 {
        "DayNote · 今天都做完了".into()
    } else {
        format!("DayNote · 今天还有 {open_today} 件")
    }
}

/// Update the tooltip (from any thread; the tray call itself runs on the main thread).
pub fn set_tip(app: &AppHandle, open_today: usize) {
    let text = tip_text(open_today);
    {
        let sh = shell(app);
        let mut cur = lock(&sh.tray_tip);
        if *cur == text {
            return;
        }
        *cur = text.clone();
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(text));
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn tooltip_text() {
        assert_eq!(super::tip_text(0), "DayNote · 今天都做完了");
        assert_eq!(super::tip_text(3), "DayNote · 今天还有 3 件");
    }
}
