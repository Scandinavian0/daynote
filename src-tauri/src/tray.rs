//! Tray icon: left click toggles the main window, right click opens the menu.
use crate::state::{lock, shell};
use crate::win;
use std::path::Path;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

pub const TRAY_ID: &str = "main";

/// The autostart check item, kept so it can reflect the real state.
pub struct TrayItems {
    pub autostart: CheckMenuItem<Wry>,
}

/// Registry value name of the autostart entry (the plugin's `app_name`).
pub const RUN_NAME: &str = "DayNote";

/// Autostart is on for this copy: the entry exists, Task Manager has not disabled it,
/// and it launches this very program (not another installed or portable copy).
pub fn autostart_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false) && entry_is_ours()
}

/// Turning autostart on always points it at this program (an explicit user choice).
/// Turning it off removes only an entry that launches this program.
pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if !enabled && !entry_is_ours() {
        sync_autostart(app);
        return Ok(());
    }
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
        if !crate::sys::set_run_entry(RUN_NAME, &canonical_entry(&exe)) {
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

/// A Windows path in comparable form: no `\\?\` prefix, backslashes, lower case.
fn normalize(path: &str) -> String {
    let path = path.trim();
    let path = match path.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path).to_string(),
    };
    path.replace('/', "\\").to_lowercase()
}

/// Whether the autostart command line `command` launches the program at `exe`. Accepts
/// the quoted form this app writes and the unquoted `<exe> --autostart` the plugin
/// writes (whose path may contain spaces).
pub fn entry_points_to(command: &str, exe: &Path) -> bool {
    let exe = normalize(&exe.to_string_lossy());
    let command = command.trim();
    if exe.is_empty() || command.is_empty() {
        return false;
    }
    if let Some(rest) = command.strip_prefix('"') {
        return rest.split_once('"').is_some_and(|(path, _)| normalize(path) == exe);
    }
    normalize(command)
        .strip_prefix(&exe)
        .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
}

/// The command line this program keeps in its autostart entry.
fn canonical_entry(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

/// Whether an existing entry is this program's but not in the canonical quoted form.
/// An entry that launches another copy is never rewritten.
#[cfg_attr(debug_assertions, allow(dead_code))] // used by release startup only
pub fn needs_rewrite(command: &str, exe: &Path) -> bool {
    entry_points_to(command, exe) && command != canonical_entry(exe)
}

/// First run of a release build: turn autostart on unless it belongs to another copy.
#[cfg_attr(debug_assertions, allow(dead_code))] // used by release startup only
pub fn first_run_enables(entry: Option<&str>, exe: &Path) -> bool {
    match entry.map(str::trim) {
        None | Some("") => true,
        Some(command) => entry_points_to(command, exe),
    }
}

/// Whether the autostart entry exists and launches this very program.
fn entry_is_ours() -> bool {
    match (std::env::current_exe(), crate::sys::run_entry(RUN_NAME)) {
        (Ok(exe), Some(command)) => entry_points_to(&command, &exe),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Program Files\DayNote\DayNote.exe";

    #[test]
    fn an_entry_belongs_to_the_program_it_launches() {
        let exe = Path::new(EXE);
        assert!(entry_points_to(r#""C:\Program Files\DayNote\DayNote.exe" --autostart"#, exe), "quoted");
        assert!(entry_points_to(r"C:\Program Files\DayNote\DayNote.exe --autostart", exe), "unquoted, with spaces");
        assert!(entry_points_to(r"C:\Program Files\DayNote\DayNote.exe", exe), "no arguments");
        assert!(entry_points_to(r#""c:\program files\daynote\DAYNOTE.EXE" --autostart"#, exe), "case");
        assert!(entry_points_to(r#""\\?\C:\Program Files\DayNote\DayNote.exe" --autostart"#, exe), "verbatim prefix");
        assert!(entry_points_to(r#""C:\Program Files\DayNote\DayNote.exe" --autostart"#, Path::new(r"\\?\C:\Program Files\DayNote\DayNote.exe")), "verbatim exe");
        assert!(entry_points_to(r"\\?\UNC\server\share\DayNote.exe --autostart", Path::new(r"\\server\share\DayNote.exe")), "UNC");
    }

    #[test]
    fn an_entry_for_another_copy_is_not_ours() {
        let exe = Path::new(EXE);
        assert!(!entry_points_to(r#""D:\Portable\DayNote\DayNote.exe" --autostart"#, exe));
        assert!(!entry_points_to(r"D:\Portable\DayNote\DayNote.exe --autostart", exe));
        assert!(!entry_points_to(r"C:\Program Files\DayNote\DayNote.exe.old --autostart", exe), "longer name");
        assert!(!entry_points_to(r#""C:\Program Files\DayNote\DayNote.exe --autostart"#, exe), "unterminated quote");
        assert!(!entry_points_to("", exe), "empty");
        assert!(!entry_points_to("   ", exe), "blank");
    }

    #[test]
    fn only_our_own_unquoted_entry_is_rewritten() {
        let exe = Path::new(EXE);
        assert!(needs_rewrite(r"C:\Program Files\DayNote\DayNote.exe --autostart", exe));
        assert!(needs_rewrite(r#""c:\program files\daynote\daynote.exe" --autostart"#, exe), "other spelling of our path");
        assert!(!needs_rewrite(r#""C:\Program Files\DayNote\DayNote.exe" --autostart"#, exe), "already canonical");
        assert!(!needs_rewrite(r"D:\Portable\DayNote\DayNote.exe --autostart", exe), "another copy");
        assert!(!needs_rewrite("", exe));
    }

    #[test]
    fn first_run_leaves_another_copys_autostart_alone() {
        let exe = Path::new(EXE);
        assert!(first_run_enables(None, exe), "no entry yet");
        assert!(first_run_enables(Some(""), exe), "an empty value launches nothing");
        assert!(first_run_enables(Some(r"C:\Program Files\DayNote\DayNote.exe --autostart"), exe));
        assert!(!first_run_enables(Some(r#""D:\Portable\DayNote\DayNote.exe" --autostart"#), exe));
    }

    #[test]
    fn tooltip_text() {
        assert_eq!(super::tip_text(0), "DayNote · 今天都做完了");
        assert_eq!(super::tip_text(3), "DayNote · 今天还有 3 件");
    }
}
