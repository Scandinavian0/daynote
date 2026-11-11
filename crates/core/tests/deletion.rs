mod common;
use common::*;
use serde_json::json;

#[test]
fn deleted_task_vanishes_everywhere_and_restore_brings_back_its_subtasks() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let r = call(
        &mut s,
        "add_tasks",
        json!({"tasks":[{"title":"体检","remind_at":"08:00","subtasks":["空腹","带身份证","挂号"]},{"title":"留着"}]}),
        &c,
    );
    let p = &r["tasks"][0];
    let pid = id(p);
    let early_deleted = id(&p["subtasks"][2]);
    call(&mut s, "delete_task", json!({"id": early_deleted}), &at("2026-11-11 09:10"));

    let r = call(&mut s, "delete_task", json!({"id": pid}), &at("2026-11-11 09:20"));
    assert_eq!(r["task"]["deleted_at"], "2026-11-11T09:20:00");

    let list = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(titles(&list["tasks"]), ["留着"]);
    let sum = call(&mut s, "get_summary", json!({}), &c);
    assert_eq!(sum["total"], 1);
    let due = call(&mut s, "due_reminders", json!({}), &c);
    assert_eq!(due["tasks"], json!([]));
    let e = call_err(&mut s, "complete_task", json!({"id": pid}), &c);
    assert_eq!(e.code, "not_found");

    let r = call(&mut s, "restore_task", json!({"id": pid}), &at("2026-11-11 09:30"));
    assert!(r["task"]["deleted_at"].is_null());
    assert_eq!(titles(&r["task"]["subtasks"]), ["空腹", "带身份证"]);
    let list = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(titles(&list["tasks"]), ["体检", "留着"]);
}

#[test]
fn a_single_subtask_can_be_deleted_and_restored() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let r = call(&mut s, "add_tasks", json!({"tasks":[{"title":"父","subtasks":["一","二"]}]}), &c);
    let sub = id(&r["tasks"][0]["subtasks"][0]);
    let r = call(&mut s, "delete_task", json!({"id": sub}), &c);
    assert_eq!(titles(&r["parent"]["subtasks"]), ["二"]);
    let r = call(&mut s, "restore_task", json!({"id": sub}), &c);
    assert_eq!(titles(&r["parent"]["subtasks"]), ["一", "二"]);
}

#[test]
fn restoring_a_task_that_is_not_deleted_or_missing() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let r = call(&mut s, "add_tasks", json!({"tasks":[{"title":"活着"}]}), &c);
    // restoring a live task is harmless
    let r = call(&mut s, "restore_task", json!({"id": id(&r["tasks"][0])}), &c);
    assert_eq!(r["task"]["title"], "活着");
    let e = call_err(&mut s, "restore_task", json!({"id": 404}), &c);
    assert_eq!(e.code, "not_found");
}

#[test]
fn parent_restore_keeps_a_separately_deleted_child_deleted_in_the_same_second() {
    let mut s = store();
    let c = at("2026-11-11 09:00");
    let r = call(
        &mut s,
        "add_tasks",
        json!({"tasks": [{"title": "parent", "subtasks": ["deleted earlier", "kept"]}]}),
        &c,
    );
    let pid = id(&r["tasks"][0]);
    let child = id(&r["tasks"][0]["subtasks"][0]);
    call(&mut s, "delete_task", json!({"id": child}), &c);
    call(&mut s, "delete_task", json!({"id": pid}), &c);
    let restored = call(&mut s, "restore_task", json!({"id": pid}), &c);
    assert_eq!(titles(&restored["task"]["subtasks"]), ["kept"]);
    // A subsequent parent deletion uses a fresh set of cascading children.
    call(&mut s, "restore_task", json!({"id": child}), &c);
    call(&mut s, "delete_task", json!({"id": pid}), &c);
    let restored = call(&mut s, "restore_task", json!({"id": pid}), &c);
    assert_eq!(titles(&restored["task"]["subtasks"]), ["deleted earlier", "kept"]);
}

#[test]
fn migrating_v1_preserves_pending_parent_restoration() {
    let path = std::env::temp_dir().join(format!(
        "daynote-v1-delete-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let c = at("2026-11-11 09:00");
    let mut s = daynote_core::Store::open(&path).unwrap();
    let r = call(&mut s, "add_tasks", json!({"tasks": [{"title": "parent", "subtasks": ["old deletion", "kept"]}]}), &c);
    let pid = id(&r["tasks"][0]);
    let child = id(&r["tasks"][0]["subtasks"][0]);
    call(&mut s, "delete_task", json!({"id": child}), &c);
    call(&mut s, "delete_task", json!({"id": pid}), &at("2026-11-11 09:01"));
    drop(s);
    // Recreate the v1 fixture: task rows existed but no cascade identity table did.
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch("DROP TABLE task_delete_children; PRAGMA user_version=1;").unwrap();
    drop(old);
    let mut migrated = daynote_core::Store::open(&path).unwrap();
    let r = call(&mut migrated, "restore_task", json!({"id": pid}), &c);
    assert_eq!(titles(&r["task"]["subtasks"]), ["kept"]);
    drop(migrated);
    std::fs::remove_file(path).unwrap();
}
