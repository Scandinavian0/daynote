//! Background threads: the reminder scheduler and the external-change watcher.
use crate::reminders::{self, Snapshot};
use crate::state::{lock, shell};
use crate::{sys, tray, win};
use chrono::NaiveDate;
use daynote_core::api::ApiError;
use daynote_core::now_local;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const ROUND_EVERY: Duration = Duration::from_secs(15);
const SOURCE: &str = "app";

/// Rounds run on this one thread only, every 15 s and whenever someone kicks it.
pub fn spawn(app: AppHandle, kicks: Receiver<()>, autostarted: bool) {
    let _ = std::thread::Builder::new()
        .name("daynote-scheduler".into())
        .spawn(move || {
            let mut first = true;
            loop {
                if let Err(e) = round(&app, first, first && autostarted) {
                    eprintln!("DayNote: reminder round failed: {e}");
                } else {
                    first = false;
                }
                match kicks.recv_timeout(ROUND_EVERY) {
                    Ok(()) => while kicks.try_recv().is_ok() {},
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
}

/// Poll `data_version` every second; another process (CLI, MCP) wrote → broadcast.
pub fn spawn_watcher(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("daynote-watcher".into())
        .spawn(move || {
            let mut last = shell(&app).data_version();
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let v = shell(&app).data_version();
                if v.is_some() && v != last {
                    last = v;
                    crate::state::after_write(&app);
                }
            }
        });
}

fn tasks(v: Value) -> Vec<Value> {
    match v {
        Value::Object(mut m) => match m.remove("tasks") {
            Some(Value::Array(a)) => a,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn round(app: &AppHandle, first: bool, at_login: bool) -> Result<(), ApiError> {
    let sh = shell(app);
    let now = now_local();
    let today = now.date();
    let day = |d: NaiveDate| d.format("%Y-%m-%d").to_string();

    let due = tasks(sh.call("due_reminders", json!({}), SOURCE)?);
    let open = tasks(sh.call(
        "list_tasks",
        json!({"date": day(today), "status": "todo"}),
        SOURCE,
    )?);
    let overdue = tasks(sh.call("list_tasks", json!({"status": "overdue"}), SOURCE)?).len();
    let prefs = sh.prefs();
    let snapshot = lock(&sh.queue).clone();

    // Latest state of every task the queue refers to.
    let mut dates: Vec<NaiveDate> = reminders::referenced_tasks(&snapshot)
        .into_iter()
        .filter_map(|(_, d)| d)
        .collect();
    dates.sort();
    dates.dedup();
    let mut lookup: HashMap<i64, Value> = HashMap::new();
    for d in dates {
        for t in tasks(sh.call("list_tasks", json!({ "date": day(d) }), SOURCE)?) {
            if let Some(Value::Array(subs)) = t.get("subtasks") {
                for s in subs {
                    if let Some(id) = reminders::task_id(s) {
                        lookup.insert(id, s.clone());
                    }
                }
            }
            if let Some(id) = reminders::task_id(&t) {
                lookup.insert(id, t);
            }
        }
    }

    let refreshed = reminders::refresh(&snapshot, &lookup, &open, overdue, now);
    let queued: Vec<String> = refreshed.iter().map(|c| c.id.clone()).collect();
    let plan = reminders::plan(&Snapshot {
        now,
        morning: prefs.morning_enabled.then_some(prefs.morning_time),
        evening: prefs.evening_enabled.then_some(prefs.evening_time),
        due: &due,
        today_open: &open,
        overdue,
        last_morning: prefs.last_morning,
        last_evening: prefs.last_evening,
        first_round: first,
        at_login,
        queued: &queued,
    });

    for id in &plan.mark_reminded {
        if let Err(e) = sh.call("mark_reminded", json!({ "id": id }), SOURCE) {
            eprintln!("DayNote: mark_reminded {id} failed: {e}");
        }
    }
    if let Some(d) = plan.last_morning {
        lock(&sh.prefs).last_morning = Some(d);
        sh.save_setting("last_morning", &day(d));
    }
    if let Some(d) = plan.last_evening {
        lock(&sh.prefs).last_evening = Some(d);
        sh.save_setting("last_evening", &day(d));
    }

    let added = !plan.cards.is_empty();
    let changed = {
        let mut q = lock(&sh.queue);
        let before = q.clone();
        reminders::merge(&mut q, &refreshed, plan.cards);
        *q != before
    };
    if changed {
        let _ = app.emit("reminders-changed", ());
        win::reminder_sync_later(app);
    }
    if added && prefs.sound {
        sys::play_notification();
    }
    if !plan.mark_reminded.is_empty() {
        let _ = app.emit("data-changed", ());
    }
    tray::set_tip(app, open.len());
    Ok(())
}
