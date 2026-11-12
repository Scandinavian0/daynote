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
mod startup;
mod state;
mod sys;
mod tray;
mod updates;
mod win;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::UpdateState::default())
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
            updates::check_update,
            updates::download_update,
            updates::install_update,
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
        // Tauri runs this when the event loop starts and panics on an error, which a
        // windows-subsystem build would swallow; so failures are shown here instead.
        .setup(move |app| {
            if let Err(e) = setup(app.handle(), autostarted) {
                let text = match e {
                    SetupError::Data(e) => startup::failure_message(&e),
                    SetupError::Other(e) => startup::generic_failure_message(&e.to_string()),
                };
                fail(&text);
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(e) => fail(&startup::generic_failure_message(&e.to_string())),
    };

    app.run(|_app, event| {
        // Hidden windows are not "closed"; only an explicit quit (with an exit code) ends.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}

/// Tell the user why DayNote cannot start, then exit with a failure code.
fn fail(text: &str) -> ! {
    eprintln!("DayNote: {text}");
    sys::error_box("DayNote 无法启动", text);
    std::process::exit(1)
}

enum SetupError {
    /// The data directory or its configuration (the message names what to fix).
    Data(daynote_core::paths::LocationError),
    Other(Box<dyn std::error::Error>),
}

fn setup(app: &AppHandle, autostarted: bool) -> Result<(), SetupError> {
    let opened = startup::open_data().map_err(SetupError::Data)?;
    setup_shell(app, autostarted, opened).map_err(SetupError::Other)
}

fn setup_shell(
    app: &AppHandle,
    autostarted: bool,
    opened: startup::Opened,
) -> Result<(), Box<dyn std::error::Error>> {
    let startup::Opened { dir: data_dir, store, pending_error } = opened;
    let token = match http::load_or_create_token(&data_dir) {
        Ok(t) => Some(t),
        Err(e) => {
            eprintln!("DayNote: {e}");
            None
        }
    };
    let (kick_tx, kick_rx) = channel();
    let (save_tx, save_rx) = channel();
    let mut shell_state = Shell::new(
        store,
        prefs::Prefs::default(),
        token,
        data_dir,
        kick_tx,
        save_tx,
    );
    shell_state.data_dir_error = pending_error;
    app.manage(shell_state);
    let sh = shell(app);
    let settings = sh.call("get_settings", serde_json::json!({}), "app")?;
    if let Some(map) = settings.get("settings").and_then(|v| v.as_object()) {
        *state::lock(&sh.prefs) = prefs::Prefs::from_settings(map);
    }

    // Release builds only; debug builds never write the autostart entry. The entry
    // belongs to whichever copy of DayNote it launches: another installed or portable
    // copy's entry is neither taken over on first run nor rewritten here.
    #[cfg(not(debug_assertions))]
    {
        let exe = std::env::current_exe().ok();
        let entry = || sys::run_entry(tray::RUN_NAME);
        if !sh.prefs().autostart_initialized {
            let enable = exe
                .as_deref()
                .is_none_or(|exe| tray::first_run_enables(entry().as_deref(), exe));
            if enable {
                if let Err(e) = tray::set_autostart(app, true) {
                    eprintln!("DayNote: {e}");
                }
            } else {
                eprintln!("DayNote: autostart already launches another copy; left unchanged");
            }
            state::lock(&sh.prefs).autostart_initialized = true;
            sh.save_setting("autostart_initialized", "1");
        }
        // Keep our own entry in the quoted form (also heals one an older build wrote).
        if let (Some(exe), Some(command)) = (exe.as_deref(), entry()) {
            if tray::needs_rewrite(&command, exe) {
                tray::quote_run_entry();
            }
        }
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
