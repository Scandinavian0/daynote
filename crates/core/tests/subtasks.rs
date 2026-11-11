mod common;
use common::*;
use serde_json::{json, Value};

fn setup(s: &mut daynote_core::Store) -> (i64, Vec<i64>) {
    let c = at("2026-11-11 09:00");
    let r = call(
        s,
        "add_tasks",
        json!({"tasks":[{"title":"搬家","subtasks":["打包","叫车","退押金"]}]}),
        &c,
    );
    let p = &r["tasks"][0];
    let subs = p["subtasks"].as_array().unwrap().iter().map(id).collect();
    (id(p), subs)
}

fn statuses(v: &Value) -> Vec<String> {
    v["subtasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["status"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn completing_parent_completes_open_subtasks_and_records_who() {
    let mut s = store();
    let (p, subs) = setup(&mut s);
    call(&mut s, "complete_task", json!({"id": subs[0]}), &by("user", "2026-11-11 10:00"));
    let r = call(&mut s, "complete_task", json!({"id": p}), &by("codex", "2026-11-11 11:00"));
    let t = &r["task"];
    assert_eq!(t["status"], "done");
    assert_eq!(t["completed_by"], "codex");
    assert_eq!(t["completed_at"], "2026-11-11T11:00:00");
    assert_eq!(statuses(t), ["done", "done", "done"]);
    // the already-done subtask keeps its original completion record
    assert_eq!(t["subtasks"][0]["completed_by"], "user");
    assert_eq!(t["subtasks"][1]["completed_by"], "codex");
    assert!(r.get("parent").is_none());
}

#[test]
fn completing_last_open_subtask_completes_parent() {
    let mut s = store();
    let (p, subs) = setup(&mut s);
    let c = at("2026-11-11 12:00");
    call(&mut s, "complete_task", json!({"id": subs[0]}), &c);
    let r = call(&mut s, "complete_task", json!({"id": subs[1]}), &c);
    assert_eq!(r["parent"]["status"], "todo");
    let r = call(&mut s, "complete_task", json!({"id": subs[2], "done": true}), &c);
    assert_eq!(r["task"]["status"], "done");
    assert_eq!(r["parent"]["id"], p);
    assert_eq!(r["parent"]["status"], "done");
    assert_eq!(r["parent"]["completed_at"], "2026-11-11T12:00:00");
}

#[test]
fn reopening_a_subtask_reopens_done_parent() {
    let mut s = store();
    let (p, subs) = setup(&mut s);
    let c = at("2026-11-11 12:00");
    call(&mut s, "complete_task", json!({"id": p}), &c);
    let r = call(&mut s, "complete_task", json!({"id": subs[1], "done": false}), &c);
    assert_eq!(r["task"]["status"], "todo");
    assert!(r["task"]["completed_at"].is_null());
    assert_eq!(r["parent"]["status"], "todo");
    assert!(r["parent"]["completed_at"].is_null());
    assert_eq!(statuses(&r["parent"]), ["done", "todo", "done"]);
}

#[test]
fn reopening_parent_leaves_subtasks_done() {
    let mut s = store();
    let (p, _) = setup(&mut s);
    let c = at("2026-11-11 12:00");
    call(&mut s, "complete_task", json!({"id": p}), &c);
    let r = call(&mut s, "complete_task", json!({"id": p, "done": false}), &c);
    assert_eq!(r["task"]["status"], "todo");
    assert_eq!(statuses(&r["task"]), ["done", "done", "done"]);
}

#[test]
fn add_subtasks_appends_under_parent_and_returns_parent() {
    let mut s = store();
    let (p, _) = setup(&mut s);
    let c = at("2026-11-11 12:00");
    let r = call(&mut s, "add_subtasks", json!({"parent_id": p, "subtasks": ["交钥匙"]}), &c);
    let tasks = r["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(id(&tasks[0]), p);
    assert_eq!(titles(&tasks[0]["subtasks"]), ["打包", "叫车", "退押金", "交钥匙"]);
}

#[test]
fn subtasks_cannot_nest() {
    let mut s = store();
    let (_, subs) = setup(&mut s);
    let e = call_err(
        &mut s,
        "add_subtasks",
        json!({"parent_id": subs[0], "subtasks": ["孙任务"]}),
        &at("2026-11-11 12:00"),
    );
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn completing_unknown_task_is_not_found() {
    let mut s = store();
    let e = call_err(&mut s, "complete_task", json!({"id": 999}), &at("2026-11-11 12:00"));
    assert_eq!(e.code, "not_found");
}
