mod common;
use common::*;
use serde_json::json;

fn add(s: &mut daynote_core::Store, date: &str, list: &[&str]) -> Vec<i64> {
    let tasks: Vec<_> = list.iter().map(|t| json!({"title": t})).collect();
    let r = call(s, "add_tasks", json!({"date": date, "tasks": tasks}), &at("2026-11-11 08:00"));
    r["tasks"].as_array().unwrap().iter().map(id).collect()
}

#[test]
fn new_tasks_append_after_the_days_last_sort_order() {
    let mut s = store();
    let a = add(&mut s, "2026-11-11", &["一", "二"]);
    let b = add(&mut s, "2026-11-11", &["三"]);
    let c = at("2026-11-11 08:00");
    let r = call(&mut s, "list_tasks", json!({"date": "2026-11-11"}), &c);
    assert_eq!(titles(&r["tasks"]), ["一", "二", "三"]);
    assert_eq!(r["tasks"][0]["sort_order"], 1);
    assert_eq!(r["tasks"][2]["sort_order"], 3);
    assert_eq!(id(&r["tasks"][2]), b[0]);
    assert_ne!(a[0], b[0]);
}

#[test]
fn reorder_changes_list_order_and_keeps_unlisted_tasks_after() {
    let mut s = store();
    let ids = add(&mut s, "2026-11-11", &["一", "二", "三", "四"]);
    let c = at("2026-11-11 08:00");
    let r = call(&mut s, "reorder_tasks", json!({"date": "2026-11-11", "ids": [ids[2], ids[0], ids[1]]}), &c);
    assert_eq!(titles(&r["tasks"]), ["三", "一", "二", "四"]);
    let r = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(titles(&r["tasks"]), ["三", "一", "二", "四"]);
    let r = add(&mut s, "2026-11-11", &["五"]);
    let list = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(id(&list["tasks"][4]), r[0]);
}

#[test]
fn reorder_rejects_tasks_from_another_day() {
    let mut s = store();
    add(&mut s, "2026-11-11", &["一"]);
    let other = add(&mut s, "2026-11-12", &["别天"]);
    let e = call_err(&mut s, "reorder_tasks", json!({"date": "2026-11-11", "ids": [other[0]]}), &at("2026-11-11 08:00"));
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn list_filters_by_range_and_status_and_overdue_ignores_range() {
    let mut s = store();
    let old = add(&mut s, "2026-11-09", &["前天没做", "前天做了"]);
    add(&mut s, "2026-11-10", &["昨天没做"]);
    let today = add(&mut s, "2026-11-11", &["今天做了", "今天没做"]);
    add(&mut s, "2026-11-12", &["明天"]);
    let c = at("2026-11-11 20:00");
    call(&mut s, "complete_task", json!({"id": old[1]}), &c);
    call(&mut s, "complete_task", json!({"id": today[0]}), &c);

    let r = call(&mut s, "list_tasks", json!({"from": "2026-11-10", "to": "2026-11-12"}), &c);
    assert_eq!(titles(&r["tasks"]), ["昨天没做", "今天做了", "今天没做", "明天"]);
    let r = call(&mut s, "list_tasks", json!({"from": "2026-11-09", "to": "2026-11-11", "status": "done"}), &c);
    assert_eq!(titles(&r["tasks"]), ["前天做了", "今天做了"]);
    let r = call(&mut s, "list_tasks", json!({"status": "todo"}), &c);
    assert_eq!(titles(&r["tasks"]), ["今天没做"]);
    let r = call(&mut s, "list_tasks", json!({"date": "2026-11-12", "status": "overdue"}), &c);
    assert_eq!(titles(&r["tasks"]), ["前天没做", "昨天没做"]);

    let e = call_err(&mut s, "list_tasks", json!({"status": "later"}), &c);
    assert_eq!(e.code, "invalid_argument");
    let e = call_err(&mut s, "list_tasks", json!({"from": "2026-11-12", "to": "2026-11-10"}), &c);
    assert_eq!(e.code, "invalid_argument");
    let e = call_err(&mut s, "list_tasks", json!({"date": "11/12"}), &c);
    assert_eq!(e.code, "invalid_argument");
}

#[test]
fn summary_counts_top_level_tasks_per_day_and_all_overdue() {
    let mut s = store();
    let c = at("2026-11-11 20:00");
    add(&mut s, "2026-11-01", &["很久以前没做"]);
    call(
        &mut s,
        "add_tasks",
        json!({"date":"2026-11-10","tasks":[{"title":"昨天带子任务","subtasks":["子一","子二"]}]}),
        &c,
    );
    let today = add(&mut s, "2026-11-11", &["今一", "今二", "今三"]);
    call(&mut s, "complete_task", json!({"id": today[0]}), &c);

    let r = call(&mut s, "get_summary", json!({"from": "2026-11-10", "to": "2026-11-12"}), &c);
    assert_eq!(r["from"], "2026-11-10");
    assert_eq!(r["to"], "2026-11-12");
    assert_eq!((r["total"].as_i64(), r["done"].as_i64(), r["todo"].as_i64()), (Some(4), Some(1), Some(3)));
    assert_eq!(
        r["days"],
        json!([
            {"date":"2026-11-10","total":1,"done":0,"todo":1},
            {"date":"2026-11-11","total":3,"done":1,"todo":2},
            {"date":"2026-11-12","total":0,"done":0,"todo":0}
        ])
    );
    assert_eq!(titles(&r["overdue"]), ["很久以前没做", "昨天带子任务"]);
    assert_eq!(r["overdue"][1]["subtasks"].as_array().unwrap().len(), 2);

    let r = call(&mut s, "get_summary", json!({}), &c);
    assert_eq!(r["from"], "2026-11-11");
    assert_eq!(r["total"], 3);
}
