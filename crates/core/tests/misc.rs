mod common;
use common::*;
use daynote_core::{api, Store};
use serde_json::json;

#[test]
fn day_note_defaults_to_today_and_is_empty_until_set() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let r = call(&mut s, "get_day_note", json!({}), &c);
    assert_eq!(r["date"], "2026-11-11");
    assert_eq!(r["content"], "");
    assert!(r["updated_at"].is_null());
    let r = call(&mut s, "set_day_note", json!({"content": "今天心情不错"}), &c);
    assert_eq!(r["content"], "今天心情不错");
    assert_eq!(r["updated_at"], "2026-11-11T09:00:00");
    call(&mut s, "set_day_note", json!({"date": "2026-11-12", "content": "明天"}), &c);
    let r = call(&mut s, "get_day_note", json!({"date": "2026-11-11"}), &at("2026-11-12 09:00"));
    assert_eq!(r["content"], "今天心情不错");
    let e = call_err(&mut s, "set_day_note", json!({}), &c);
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn settings_are_stored_as_strings() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    assert_eq!(call(&mut s, "get_settings", json!({}), &c)["settings"], json!({}));
    call(&mut s, "set_setting", json!({"key": "hotkey", "value": "Ctrl+Alt+D"}), &c);
    let r = call(&mut s, "set_setting", json!({"key": "http_port", "value": 8765}), &c);
    assert_eq!(r["settings"], json!({"hotkey": "Ctrl+Alt+D", "http_port": "8765"}));
    let e = call_err(&mut s, "set_setting", json!({"value": "x"}), &c);
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn every_success_carries_today_and_now_and_unknown_tools_are_not_found() {
    let mut s = store();
    let c = at("2026-11-11 23:59");
    for tool in ["list_tasks", "get_summary", "get_day_note", "list_recurring", "due_reminders", "get_settings"] {
        let r = call(&mut s, tool, json!({}), &c);
        assert_eq!(r["today"], "2026-11-11", "{tool}");
        assert_eq!(r["now"], "2026-11-11T23:59:00", "{tool}");
    }
    assert_eq!(call_err(&mut s, "nope", json!({}), &c).code, "not_found");
    let e = api::dispatch(&mut s, "list_tasks", json!([1]), &c).unwrap_err();
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn failed_calls_leave_no_partial_writes() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let e = call_err(&mut s, "add_tasks", json!({"tasks":[{"title":"好的"},{"title":""}]}), &c);
    assert_eq!(e.code, "invalid_argument");
    assert_eq!(call(&mut s, "list_tasks", json!({}), &c)["tasks"], json!([]));
}

#[test]
fn tool_specs_cover_public_and_internal_tools() {
    let specs = api::tool_specs();
    let public: Vec<_> = specs.iter().filter(|t| t.public).map(|t| t.name).collect();
    for name in [
        "list_tasks", "add_tasks", "add_subtasks", "update_task", "complete_task", "move_tasks",
        "reorder_tasks", "delete_task", "restore_task", "get_summary", "get_day_note",
        "set_day_note", "add_recurring", "list_recurring", "update_recurring", "delete_recurring",
    ] {
        assert!(public.contains(&name), "{name} should be public");
    }
    for name in ["due_reminders", "mark_reminded", "snooze_task", "get_settings", "set_setting"] {
        let spec = specs.iter().find(|t| t.name == name).expect(name);
        assert!(!spec.public, "{name} should be internal");
    }
    let mut s = store();
    let c = at("2026-11-11 09:00");
    for spec in &specs {
        assert_eq!(spec.input_schema["type"], "object", "{}", spec.name);
        assert!(!spec.description.is_empty());
        // every spec'd tool is actually dispatchable (argument errors are fine)
        if let Err(e) = api::dispatch(&mut s, spec.name, json!({}), &c) {
            assert_eq!(e.code, "invalid_argument", "{}: {}", spec.name, e.message);
        }
    }
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("daynote-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("nested").join("daynote.db")
}

#[test]
fn two_connections_share_a_file_and_see_each_others_writes() {
    let path = temp_db("share");
    let mut gui = Store::open(&path).unwrap();
    let mut cli = Store::open(&path).unwrap();
    let c = at("2026-11-11 09:00");
    let v0 = gui.data_version().unwrap();
    call(&mut cli, "add_tasks", json!({"tasks":[{"title":"命令行加的"}]}), &by("cli", "2026-11-11 09:00"));
    assert_ne!(gui.data_version().unwrap(), v0, "GUI must notice external change");
    let r = call(&mut gui, "list_tasks", json!({}), &c);
    assert_eq!(titles(&r["tasks"]), ["命令行加的"]);
    drop((gui, cli));
    // reopening an existing database keeps the data (migration is idempotent)
    let mut again = Store::open(&path).unwrap();
    assert_eq!(titles(&call(&mut again, "list_tasks", json!({}), &c)["tasks"]), ["命令行加的"]);
}

#[test]
fn concurrent_writers_do_not_fail_or_lose_sort_order() {
    let path = temp_db("concurrent");
    Store::open(&path).unwrap();
    let handles: Vec<_> = (0..4)
        .map(|w| {
            let path = path.clone();
            std::thread::spawn(move || {
                let mut s = Store::open(&path).unwrap();
                for i in 0..25 {
                    call(
                        &mut s,
                        "add_tasks",
                        json!({"tasks":[{"title": format!("w{w}-{i}")}]}),
                        &at("2026-11-11 09:00"),
                    );
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    let r = call(&mut s, "list_tasks", json!({}), &at("2026-11-11 09:00"));
    let orders: Vec<i64> = r["tasks"].as_array().unwrap().iter().map(|t| t["sort_order"].as_i64().unwrap()).collect();
    assert_eq!(orders, (1..=100).collect::<Vec<_>>());
}
