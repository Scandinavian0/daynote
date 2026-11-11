mod common;
use common::*;
use serde_json::json;

#[test]
fn added_tasks_are_listed_for_today_with_subtasks_and_clock() {
    let mut s = store();
    let c = by("claude-code", "2026-11-11 09:30");
    let added = call(
        &mut s,
        "add_tasks",
        json!({"tasks":[
            {"title":"写周报","note":"给老板","remind_at":"15:00","subtasks":["收集数据",{"title":"画图","note":"柱状图"}]},
            {"title":"买菜","priority":1}
        ]}),
        &c,
    );
    assert_eq!(added["today"], "2026-11-11");
    assert_eq!(added["now"], "2026-11-11T09:30:00");
    assert_eq!(titles(&added["tasks"]), ["写周报", "买菜"]);

    let listed = call(&mut s, "list_tasks", json!({}), &c);
    assert_eq!(listed["today"], "2026-11-11");
    let tasks = &listed["tasks"];
    assert_eq!(titles(tasks), ["写周报", "买菜"]);
    let first = &tasks[0];
    assert_eq!(first["date"], "2026-11-11");
    assert_eq!(first["note"], "给老板");
    assert_eq!(first["status"], "todo");
    assert_eq!(first["remind_at"], "2026-11-11T15:00:00");
    assert_eq!(first["source"], "claude-code");
    assert_eq!(first["priority"], 0);
    assert!(first["completed_at"].is_null());
    assert!(first["parent_id"].is_null());
    assert_eq!(titles(&first["subtasks"]), ["收集数据", "画图"]);
    assert_eq!(first["subtasks"][1]["note"], "柱状图");
    assert_eq!(first["subtasks"][0]["parent_id"], first["id"]);
    assert_eq!(first["subtasks"][0]["date"], "2026-11-11");
    assert!(first["subtasks"][0].get("subtasks").is_none());
    assert_eq!(tasks[1]["priority"], 1);
    assert_eq!(tasks[1]["subtasks"], json!([]));
}
