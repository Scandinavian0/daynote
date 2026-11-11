mod common;
use common::*;
use serde_json::json;

fn due(s: &mut daynote_core::Store, now: &str) -> Vec<String> {
    let r = call(s, "due_reminders", json!({}), &at(now));
    titles(&r["tasks"])
}

#[test]
fn a_reminder_fires_once_at_its_time() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let r = call(
        &mut s,
        "add_tasks",
        json!({"tasks":[{"title":"开会","remind_at":"10:00"},{"title":"无提醒"}]}),
        &c,
    );
    let tid = id(&r["tasks"][0]);
    assert!(due(&mut s, "2026-11-11 09:59").is_empty());
    assert_eq!(due(&mut s, "2026-11-11 10:00"), ["开会"]);
    call(&mut s, "mark_reminded", json!({"id": tid}), &at("2026-11-11 10:00"));
    assert!(due(&mut s, "2026-11-11 10:01").is_empty());
    assert!(due(&mut s, "2026-11-11 23:00").is_empty());
}

#[test]
fn a_snoozed_reminder_comes_back_once_at_snooze_time() {
    let mut s = store();
    let r = call(
        &mut s,
        "add_tasks",
        json!({"tasks":[{"title":"吃药","remind_at":"2026-11-11T10:00"}]}),
        &at("2026-11-11 09:00"),
    );
    let tid = id(&r["tasks"][0]);
    call(&mut s, "mark_reminded", json!({"id": tid}), &at("2026-11-11 10:00"));
    call(&mut s, "snooze_task", json!({"id": tid, "until": "10:15"}), &at("2026-11-11 10:00"));
    assert!(due(&mut s, "2026-11-11 10:14").is_empty());
    assert_eq!(due(&mut s, "2026-11-11 10:15"), ["吃药"]);
    call(&mut s, "mark_reminded", json!({"id": tid}), &at("2026-11-11 10:16"));
    assert!(due(&mut s, "2026-11-11 10:30").is_empty());
}

#[test]
fn a_reminder_missed_while_the_pc_was_off_is_still_returned() {
    let mut s = store();
    call(
        &mut s,
        "add_tasks",
        json!({"date":"2026-11-10","tasks":[{"title":"昨天的提醒","remind_at":"21:00"}]}),
        &at("2026-11-10 08:00"),
    );
    assert_eq!(due(&mut s, "2026-11-11 08:00"), ["昨天的提醒"]);
}

#[test]
fn completed_tasks_do_not_remind() {
    let mut s = store();
    let r = call(
        &mut s,
        "add_tasks",
        json!({"tasks":[{"title":"已做","remind_at":"10:00"}]}),
        &at("2026-11-11 09:00"),
    );
    call(&mut s, "complete_task", json!({"id": id(&r["tasks"][0])}), &at("2026-11-11 09:30"));
    assert!(due(&mut s, "2026-11-11 10:00").is_empty());
}

#[test]
fn snooze_needs_a_reminder() {
    let mut s = store();
    let r = call(&mut s, "add_tasks", json!({"tasks":[{"title":"x"}]}), &at("2026-11-11 09:00"));
    let e = call_err(&mut s, "snooze_task", json!({"id": id(&r["tasks"][0]), "minutes": 5}), &at("2026-11-11 09:00"));
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn snooze_rejects_unrepresentable_minutes_without_changing_the_task() {
    let mut s = store();
    let ctx = at("2026-11-11 09:00");
    let r = call(
        &mut s,
        "add_tasks",
        json!({"tasks": [{"title": "reminder", "remind_at": "10:00"}]}),
        &ctx,
    );
    let tid = id(&r["tasks"][0]);
    for minutes in [i64::MAX, 1_000_000_000_000i64] {
        let e = call_err(
            &mut s,
            "snooze_task",
            json!({"id": tid, "minutes": minutes}),
            &ctx,
        );
        assert_eq!(e.code, "invalid_argument");
        let after = call(&mut s, "list_tasks", json!({}), &ctx);
        assert_eq!(after["tasks"][0], r["tasks"][0]);
    }
}
