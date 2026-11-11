//! `#[tauri::command]` handlers called by the UI (names and arguments must match
//! ui/src/lib/backend.ts and the Svelte pages).
use crate::http::{self, Backend};
use crate::prefs::fmt_hhmm;
use crate::reminders::{self, Card, Kind};
use crate::state::{after_write, app_state_changed, lock, shell};
use crate::{tray, win};
use chrono::Duration;
use daynote_core::api::ApiError;
use daynote_core::now_local;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

const USER: &str = "user";

#[derive(Debug, Serialize)]
pub struct CmdError {
    pub code: String,
    pub message: String,
}

impl From<ApiError> for CmdError {
    fn from(e: ApiError) -> Self {
        CmdError {
            code: e.code.to_string(),
            message: e.message,
        }
    }
}

#[tauri::command]
pub async fn api(app: AppHandle, tool: String, args: Option<Value>) -> Result<Value, CmdError> {
    let v = shell(&app).call(&tool, args.unwrap_or(Value::Null), USER)?;
    if http::is_write(&tool) {
        after_write(&app);
    }
    Ok(v)
}

/// Shell-side state (`AppState` in ui/src/lib/types.ts).
#[derive(Debug, Serialize)]
pub struct AppState {
    expanded: bool,
    pinned: bool,
    autostart: bool,
    hotkey: String,
    morning_enabled: bool,
    morning_time: String,
    evening_enabled: bool,
    evening_time: String,
    sound: bool,
    http_enabled: bool,
    http_port: u16,
    http_active_port: u16,
    http_error: Option<String>,
    token_path: String,
    data_dir: String,
    next_data_dir: String,
    data_dir_overridden: bool,
    cli_path: String,
    version: String,
    glass: bool,
}

#[tauri::command]
pub async fn app_state(app: AppHandle) -> AppState {
    let sh = shell(&app);
    let (p, http_active_port, http_error) = {
        let _http = lock(&sh.http);
        (
            sh.prefs(),
            sh.http_port.load(Ordering::Relaxed),
            lock(&sh.http_error).clone(),
        )
    };
    let cli = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("daynote-cli.exe")))
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let hotkey = lock(&sh.hotkey).clone();
    AppState {
        expanded: p.expanded,
        pinned: p.pinned,
        autostart: tray::autostart_enabled(&app),
        hotkey,
        morning_enabled: p.morning_enabled,
        morning_time: fmt_hhmm(p.morning_time),
        evening_enabled: p.evening_enabled,
        evening_time: fmt_hhmm(p.evening_time),
        sound: p.sound,
        http_enabled: p.http_enabled,
        http_port: p.http_port,
        http_active_port,
        http_error,
        token_path: http::token_path(&sh.data_dir).display().to_string(),
        data_dir: sh.data_dir.display().to_string(),
        next_data_dir: daynote_core::data_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        data_dir_overridden: std::env::var_os("DAYNOTE_DATA_DIR").is_some_and(|v| !v.is_empty()),
        cli_path: cli,
        version: app.package_info().version.to_string(),
        glass: sh.glass.load(Ordering::Relaxed),
    }
}

#[tauri::command]
pub fn set_expanded(app: AppHandle, expanded: bool) {
    win::set_expanded(&app, expanded);
}

#[tauri::command]
pub fn set_pinned(app: AppHandle, pinned: bool) {
    win::set_pinned(&app, pinned);
}

#[tauri::command]
pub fn hide_main(app: AppHandle) {
    win::hide_main(&app);
}

#[tauri::command]
pub async fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let r = tray::set_autostart(&app, enabled);
    app_state_changed(&app);
    r
}

/// Register `accel` as the quick-add shortcut, replacing the current one. On failure the
/// previous shortcut stays registered.
pub fn register_hotkey(app: &AppHandle, accel: &str) -> Result<(), String> {
    let accel = accel.trim();
    let new: Shortcut = accel
        .parse()
        .map_err(|_| format!("快捷键格式不对：{accel}"))?;
    let gs = app.global_shortcut();
    let sh = shell(app);
    let old_text = lock(&sh.hotkey).clone();
    let old: Option<Shortcut> = old_text.parse().ok().filter(|_| !old_text.is_empty());
    if old.is_some_and(|o| o.id() == new.id()) {
        *lock(&sh.hotkey) = accel.to_string();
        return Ok(());
    }
    if let Some(o) = old {
        let _ = gs.unregister(o);
    }
    match gs.register(new) {
        Ok(()) => {
            *lock(&sh.hotkey) = accel.to_string();
            Ok(())
        }
        Err(e) => {
            eprintln!("DayNote: registering shortcut {accel} failed: {e}");
            if let Some(o) = old {
                if gs.register(o).is_err() {
                    lock(&sh.hotkey).clear();
                }
            }
            Err(format!("快捷键 {accel} 注册失败，可能已被其他程序占用"))
        }
    }
}

#[tauri::command]
pub async fn set_hotkey(app: AppHandle, accelerator: String) -> Result<(), String> {
    let r = register_hotkey(&app, &accelerator);
    if r.is_ok() {
        let sh = shell(&app);
        let accel = accelerator.trim().to_string();
        lock(&sh.prefs).hotkey = accel.clone();
        sh.save_setting("hotkey", &accel);
    }
    app_state_changed(&app);
    r
}

#[tauri::command]
pub async fn set_pref(app: AppHandle, key: String, value: String) -> Result<(), String> {
    let sh = shell(&app);
    sh.set_pref(&key, &value, |port| start_http(&app, port))?;
    if key.starts_with("morning_") || key.starts_with("evening_") {
        sh.kick();
    }
    app_state_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn regenerate_api_token(app: AppHandle) -> Result<(), String> {
    shell(&app).rotate_token()?;
    app_state_changed(&app);
    Ok(())
}

pub fn quit(app: &AppHandle) {
    win::save_main(app);
    app.exit(0);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    quit(&app);
}

#[tauri::command]
pub fn reminder_cards(app: AppHandle) -> Vec<Card> {
    lock(&shell(&app).queue).clone()
}

fn top_level_id(t: &Value) -> Option<i64> {
    t.get("parent_id")
        .and_then(Value::as_i64)
        .or_else(|| reminders::task_id(t))
}

#[tauri::command]
pub async fn reminder_action(
    app: AppHandle,
    card_id: String,
    action: String,
    minutes: Option<i64>,
) -> Result<(), CmdError> {
    let sh = shell(&app);
    let card = {
        let mut q = lock(&sh.queue);
        q.iter().position(|c| c.id == card_id).map(|i| q.remove(i))
    };
    let _ = app.emit("reminders-changed", ());
    win::reminder_sync_later(&app);
    let Some(card) = card else { return Ok(()) };

    match run_card_action(&app, &card, &action, minutes) {
        Ok(wrote) => {
            if wrote {
                after_write(&app);
            }
            Ok(())
        }
        Err(e) => {
            // Nothing was done, so the reminder must not vanish: put the card back.
            {
                let mut q = lock(&sh.queue);
                if !q.iter().any(|c| c.id == card.id) {
                    q.insert(0, card);
                }
            }
            let _ = app.emit("reminders-changed", ());
            win::reminder_sync_later(&app);
            Err(e)
        }
    }
}

/// Carry out one card button. Returns whether data was written.
fn run_card_action(
    app: &AppHandle,
    card: &Card,
    action: &str,
    minutes: Option<i64>,
) -> Result<bool, CmdError> {
    let sh = shell(app);
    let today = now_local().date();
    let day = |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string();
    let tomorrow = day(today + Duration::days(1));
    let task = card.tasks.first();
    let mut wrote = false;
    match (card.kind, action) {
        (_, "dismiss") => {}
        (Kind::Task, "done") => {
            let id = task.and_then(reminders::task_id).unwrap_or_default();
            sh.call("complete_task", json!({ "id": id }), USER)?;
            wrote = true;
        }
        (Kind::Task, "snooze") => {
            let id = task.and_then(reminders::task_id).unwrap_or_default();
            let m = minutes.filter(|m| *m > 0).unwrap_or(10);
            sh.call("snooze_task", json!({ "id": id, "minutes": m }), USER)?;
            wrote = true;
        }
        (Kind::Task, "tomorrow") => {
            let id = task.and_then(top_level_id).unwrap_or_default();
            sh.call("move_tasks", json!({ "ids": [id], "date": tomorrow }), USER)?;
            wrote = true;
        }
        (Kind::Task, "open") => {
            let date = task
                .and_then(reminders::task_date)
                .map(day)
                .unwrap_or_else(|| day(today));
            win::open_day(app, date);
        }
        (Kind::Evening, "tomorrow") => {
            let open = sh.call(
                "list_tasks",
                json!({"date": day(today), "status": "todo"}),
                USER,
            )?;
            let ids: Vec<i64> = open["tasks"]
                .as_array()
                .map(|a| a.iter().filter_map(reminders::task_id).collect())
                .unwrap_or_default();
            if !ids.is_empty() {
                sh.call("move_tasks", json!({ "ids": ids, "date": tomorrow }), USER)?;
                wrote = true;
            }
        }
        (_, "open") => win::open_day(app, day(today)),
        _ => {
            return Err(CmdError {
                code: "invalid_argument".into(),
                message: format!("这张卡片不支持操作 {action}"),
            })
        }
    }
    Ok(wrote)
}

#[tauri::command]
pub fn reminder_resize(app: AppHandle, height: f64) {
    win::reminder_resize(&app, height);
}

#[tauri::command]
pub fn quickadd_hide(app: AppHandle) {
    win::quickadd_hide(&app);
}

// HTTP ----------------------------------------------------------------------

struct AppBackend(AppHandle);

impl Backend for AppBackend {
    fn call(&self, tool: &str, args: Value, source: &str) -> Result<Value, ApiError> {
        shell(&self.0).call(tool, args, source)
    }
}

fn start_http(app: &AppHandle, port: u16) -> Result<http::Server, String> {
    let sh = shell(app);
    let token = lock(&sh.token)
        .clone()
        .ok_or_else(|| "API 令牌不可用，请检查数据目录后重启应用".to_string())?;
    let on_write = {
        let app = app.clone();
        Box::new(move || after_write(&app))
    };
    http::Server::start(
        port,
        token,
        app.package_info().version.to_string(),
        Box::new(AppBackend(app.clone())),
        on_write,
    )
    .map_err(|e| format!("无法监听 127.0.0.1:{port}，端口可能已被占用：{e}"))
}

/// (Re)start the HTTP listener according to the current prefs.
pub fn restart_http(app: &AppHandle) {
    let sh = shell(app);
    let mut slot = lock(&sh.http);
    if let Some(old) = slot.take() {
        old.stop();
    }
    sh.http_port.store(0, Ordering::Relaxed);
    let p = sh.prefs();
    *lock(&sh.http_error) = None;
    if !p.http_enabled {
        return;
    }
    match start_http(app, p.http_port) {
        Ok(server) => {
            sh.http_port.store(server.port, Ordering::Relaxed);
            *slot = Some(server);
        }
        Err(e) => {
            eprintln!("DayNote: {e}");
            *lock(&sh.http_error) = Some(e);
        }
    }
}
