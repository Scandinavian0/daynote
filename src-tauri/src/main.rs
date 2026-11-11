#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! DayNote desktop shell: windows, tray, reminders, global shortcut, autostart and the
//! local HTTP API, all over `daynote_core::api::dispatch`.

mod commands;
mod data_location;
mod geometry;
mod http;
mod prefs;
mod reminders;
mod scheduler;
mod state;
mod sys;
mod tray;
mod win;

use daynote_core::Store;
use state::{shell, Shell};
use std::sync::mpsc::channel;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_global_shortcut::ShortcutState;

fn main() {
    let autostarted = std::env::args().any(|a| a == "--autostart");
    let app = tauri::Builder::default()
        // Must be first: a second launch only brings the running instance forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            win::show_main(app)
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("DayNote")
                .arg("--autostart")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let handle = app.clone();
                        let _ = app.run_on_main_thread(move || win::quickadd_toggle(&handle));
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::api,
            commands::app_state,
            commands::set_expanded,
            commands::set_pinned,
            commands::hide_main,
            commands::set_autostart,
            commands::set_hotkey,
            commands::set_pref,
            commands::regenerate_api_token,
            data_location::set_data_directory,
            commands::quit_app,
            commands::reminder_cards,
            commands::reminder_action,
            commands::reminder_resize,
            commands::quickadd_hide,
        ])
        .setup(move |app| {
            setup(app.handle(), autostarted)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building DayNote");

    app.run(|_app, event| {
        // Hidden windows are not "closed"; only an explicit quit (with an exit code) ends.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}

fn setup(app: &AppHandle, autostarted: bool) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = daynote_core::data_dir()?;
    let store = Store::open(&data_dir.join("daynote.db"))?;
    let token = match http::load_or_create_token(&data_dir) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("DayNote: {e}");
            None
        }
    };
    let (kick_tx, kick_rx) = channel();
    let (save_tx, save_rx) = channel();
    app.manage(Shell::new(
        store,
        prefs::Prefs::default(),
        token,
        data_dir,
        kick_tx,
        save_tx,
    ));
    let sh = shell(app);
    let settings = sh.call("get_settings", serde_json::json!({}), "app")?;
    if let Some(map) = settings.get("settings").and_then(|v| v.as_object()) {
        *state::lock(&sh.prefs) = prefs::Prefs::from_settings(map);
    }

    // First run of a release build turns autostart on once; debug builds never do.
    #[cfg(not(debug_assertions))]
    if !sh.prefs().autostart_initialized {
        if let Err(e) = tray::set_autostart(app, true) {
            eprintln!("DayNote: {e}");
        }
        state::lock(&sh.prefs).autostart_initialized = true;
        sh.save_setting("autostart_initialized", "1");
    }
    // Keep an existing entry in the quoted form (also heals one written by an older build).
    #[cfg(not(debug_assertions))]
    if tray::autostart_enabled(app) {
        tray::quote_run_entry();
    }

    win::build_main(app, !autostarted)?;
    win::build_reminder(app)?;
    win::build_quickadd(app)?;
    tray::build(app)?;

    let hotkey = sh.prefs().hotkey;
    if let Err(e) = commands::register_hotkey(app, &hotkey) {
        eprintln!("DayNote: {e}");
        // The untouched default is taken on this machine: use the first free fallback
        // and remember it, so quick-add works without a trip to the settings page.
        if hotkey == prefs::DEFAULT_HOTKEY {
            if let Some(free) = prefs::FALLBACK_HOTKEYS
                .iter()
                .find(|k| commands::register_hotkey(app, k).is_ok())
            {
                state::lock(&sh.prefs).hotkey = free.to_string();
                sh.save_setting("hotkey", free);
            }
        }
    }
    commands::restart_http(app);
    scheduler::spawn(app.clone(), kick_rx, autostarted);
    scheduler::spawn_watcher(app.clone());
    win::spawn_saver(app.clone(), save_rx);
    Ok(())
}
