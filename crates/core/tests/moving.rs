mod common;
use common::*;
use serde_json::json;

/// A task with a 15:00 reminder that has fired once and been snoozed.
fn reminded_task(s: &mut daynote_core::Store) -> (i64, i64) {
    let c = at("2026-11-11 09:00");
    let r = call(
        s,
        "add_tasks",
        json!({"tasks":[{"title":"交水费","remind_at":"2026-11-11 15:00","subtasks":["查账单"]}]}),
        &c,
    );
    let t = &r["tasks"][0];
    let tid = id(t);
    let sub = id(&t["subtasks"][0]);
    call(s, "mark_reminded", json!({"id": tid}), &at("2026-11-11 15:00"));
    let r = call(s, "snooze_task", json!({"id": tid, "minutes": 10}), &at("2026-11-11 15:00"));
    assert_eq!(r["task"]["reminded_at"], "2026-11-11T15:00:00");
    assert_eq!(r["task"]["snooze_until"], "2026-11-11T15:10:00");
    (tid, sub)
}

#[test]
fn moving_a_task_carries_subtasks_and_shifts_reminder_to_same_time() {
    let mut s = store();
    let c = at("2026-11-11 16:00");
    call(&mut s, "add_tasks", json!({"date":"2026-11-12","tasks":[{"title":"已有"}]}), &c);
    let (tid, sub) = reminded_task(&mut s);

    let r = call(&mut s, "update_task", json!({"id": tid, "date": "2026-11-12"}), &c);
    let t = &r["task"];
    assert_eq!(t["date"], "2026-11-12");
    assert_eq!(t["remind_at"], "2026-11-12T15:00:00");
    assert!(t["reminded_at"].is_null());
    assert!(t["snooze_until"].is_null());
    assert_eq!(t["subtasks"][0]["id"], sub);
    assert_eq!(t["subtasks"][0]["date"], "2026-11-12");

    let tomorrow = call(&mut s, "list_tasks", json!({"date":"2026-11-12"}), &c);
    assert_eq!(titles(&tomorrow["tasks"]), ["已有", "交水费"]);
    let today = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(today["tasks"], json!([]));
}

#[test]
fn move_tasks_moves_a_batch() {
    let mut s = store();
    let c = at("2026-11-11 16:00");
    let r = call(
        &mut s,
        "add_tasks",
        json!({"date":"2026-11-09","tasks":[{"title":"甲"},{"title":"乙","remind_at":"08:30"},{"title":"丙"}]}),
        &c,
    );
    let ids: Vec<i64> = r["tasks"].as_array().unwrap().iter().map(id).collect();
    let r = call(&mut s, "move_tasks", json!({"ids": [ids[1], ids[0]], "date": "2026-11-11"}), &c);
    assert_eq!(titles(&r["tasks"]), ["乙", "甲"]);
    assert_eq!(r["tasks"][0]["remind_at"], "2026-11-11T08:30:00");
    let today = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(titles(&today["tasks"]), ["乙", "甲"]);
}

#[test]
fn changing_or_clearing_reminder_resets_reminder_state() {
    let mut s = store();
    let c = at("2026-11-11 16:00");
    let (tid, _) = reminded_task(&mut s);
    let r = call(&mut s, "update_task", json!({"id": tid, "title": "交电费"}), &c);
    assert_eq!(r["task"]["reminded_at"], "2026-11-11T15:00:00", "title edit keeps state");

    let r = call(&mut s, "update_task", json!({"id": tid, "remind_at": "17:30"}), &c);
    assert_eq!(r["task"]["remind_at"], "2026-11-11T17:30:00");
    assert!(r["task"]["reminded_at"].is_null());
    assert!(r["task"]["snooze_until"].is_null());

    call(&mut s, "mark_reminded", json!({"id": tid}), &at("2026-11-11 17:30"));
    let r = call(&mut s, "update_task", json!({"id": tid, "remind_at": null}), &c);
    assert!(r["task"]["remind_at"].is_null());
    assert!(r["task"]["reminded_at"].is_null());
}

#[test]
fn subtask_date_cannot_be_changed_on_its_own() {
    let mut s = store();
    let (_, sub) = reminded_task(&mut s);
    let e = call_err(&mut s, "update_task", json!({"id": sub, "date": "2026-11-20"}), &at("2026-11-11 16:00"));
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn update_task_edits_fields() {
    let mut s = store();
    let c = at("2026-11-11 16:00");
    let r = call(&mut s, "add_tasks", json!({"tasks":[{"title":"旧","note":"n"}]}), &c);
    let tid = id(&r["tasks"][0]);
    let r = call(&mut s, "update_task", json!({"id": tid, "title":"新","note":null,"priority":1}), &c);
    assert_eq!(r["task"]["title"], "新");
    assert!(r["task"]["note"].is_null());
    assert_eq!(r["task"]["priority"], 1);
    let e = call_err(&mut s, "update_task", json!({"id": tid, "title":"  "}), &c);
    assert_eq!(e.code, "invalid_argument");
}
