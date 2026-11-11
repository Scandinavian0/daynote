//! 2026-11-11 is a Wednesday.
mod common;
use common::*;
use serde_json::{json, Value};

const NOW: &str = "2026-11-11 08:00";

fn series(s: &mut daynote_core::Store, args: Value) -> i64 {
    let r = call(s, "add_recurring", args, &by("codex", NOW));
    id(&r["series"])
}

fn dates(s: &mut daynote_core::Store, from: &str, to: &str, now: &str) -> Vec<String> {
    let r = call(s, "list_tasks", json!({"from": from, "to": to}), &at(now));
    r["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["date"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn daily_series_generates_one_instance_per_day_on_demand() {
    let mut s = store();
    let sid = series(&mut s, json!({"title":"背单词","note":"20个","rule":"daily","remind_time":"07:30"}));
    let r = call(&mut s, "list_tasks", json!({"from":"2026-11-11","to":"2026-11-13"}), &at(NOW));
    assert_eq!(titles(&r["tasks"]), ["背单词", "背单词", "背单词"]);
    let t = &r["tasks"][1];
    assert_eq!(t["date"], "2026-11-12");
    assert_eq!(t["series_id"], sid);
    assert_eq!(t["note"], "20个");
    assert_eq!(t["remind_at"], "2026-11-12T07:30:00");
    assert_eq!(t["source"], "codex");
    // listing again does not duplicate
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-13", NOW).len(), 3);
}

#[test]
fn weekdays_rule_skips_weekends() {
    let mut s = store();
    series(&mut s, json!({"title":"站会","rule":"weekdays"}));
    assert_eq!(
        dates(&mut s, "2026-11-11", "2026-11-17", NOW),
        ["2026-11-11", "2026-11-12", "2026-11-13", "2026-11-16", "2026-11-17"]
    );
}

#[test]
fn weekly_rule_uses_iso_weekdays() {
    let mut s = store();
    series(&mut s, json!({"title":"健身","rule":"weekly:1,3,5"}));
    assert_eq!(
        dates(&mut s, "2026-11-11", "2026-11-17", NOW),
        ["2026-11-11", "2026-11-13", "2026-11-16"]
    );
    let mut s = store();
    series(&mut s, json!({"title":"周日","rule":"weekly:7"}));
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-17", NOW), ["2026-11-15"]);
}

#[test]
fn monthly_rule_falls_back_to_last_day_of_short_months() {
    let mut s = store();
    series(&mut s, json!({"title":"月底对账","rule":"monthly:31"}));
    assert_eq!(
        dates(&mut s, "2026-11-01", "2027-02-28", NOW),
        ["2026-11-30", "2026-12-31", "2027-01-31", "2027-02-28"]
    );
    let mut s = store();
    call(&mut s, "add_recurring", json!({"title":"闰年","rule":"monthly:30"}), &at("2027-12-01 08:00"));
    assert_eq!(
        dates(&mut s, "2028-01-01", "2028-03-31", "2027-12-01 08:00"),
        ["2028-01-30", "2028-02-29", "2028-03-30"]
    );
    let mut s = store();
    series(&mut s, json!({"title":"交房租","rule":"monthly:15"}));
    assert_eq!(
        dates(&mut s, "2026-11-01", "2027-01-31", NOW),
        ["2026-11-15", "2026-12-15", "2027-01-15"]
    );
}

#[test]
fn instances_are_never_backfilled_before_today() {
    let mut s = store();
    series(&mut s, json!({"title":"打卡","rule":"daily","start_date":"2026-11-01"}));
    assert_eq!(dates(&mut s, "2026-11-01", "2026-11-11", NOW), ["2026-11-11"]);
    // the PC was off on the 12th; on the 13th nothing is filled in for the 12th
    assert_eq!(
        dates(&mut s, "2026-11-11", "2026-11-13", "2026-11-13 09:00"),
        ["2026-11-11", "2026-11-13"]
    );
    let sum = call(&mut s, "get_summary", json!({"from":"2026-11-01","to":"2026-11-13"}), &at("2026-11-13 09:00"));
    assert_eq!(sum["total"], 2);
    assert_eq!(titles(&sum["overdue"]), ["打卡"]);
}

#[test]
fn generation_respects_end_date_and_366_day_horizon() {
    let mut s = store();
    series(&mut s, json!({"title":"短期","rule":"daily","end_date":"2026-11-12"}));
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-14", NOW), ["2026-11-11", "2026-11-12"]);
    let mut s = store();
    series(&mut s, json!({"title":"长期","rule":"daily"}));
    assert_eq!(dates(&mut s, "2027-11-11", "2027-11-14", NOW), ["2027-11-11", "2027-11-12"]);
}

#[test]
fn deleted_or_moved_instances_do_not_grow_back() {
    let mut s = store();
    series(&mut s, json!({"title":"喝水","rule":"daily"}));
    let c = at(NOW);
    let r = call(&mut s, "list_tasks", json!({"from":"2026-11-12","to":"2026-11-13"}), &c);
    let (d12, d13) = (id(&r["tasks"][0]), id(&r["tasks"][1]));
    call(&mut s, "delete_task", json!({"id": d12}), &c);
    let r = call(&mut s, "move_tasks", json!({"ids":[d13], "date":"2026-11-20"}), &c);
    assert!(r["tasks"][0]["series_id"].is_null());

    assert!(dates(&mut s, "2026-11-12", "2026-11-13", NOW).is_empty());
    // 11-20 now holds the moved one plus its own generated instance
    let r = call(&mut s, "list_tasks", json!({"date":"2026-11-20"}), &c);
    assert_eq!(r["tasks"].as_array().unwrap().len(), 2);
}

#[test]
fn due_reminders_generates_todays_instance() {
    let mut s = store();
    series(&mut s, json!({"title":"吃维生素","rule":"daily","remind_time":"08:00"}));
    let r = call(&mut s, "due_reminders", json!({}), &at("2026-11-11 08:05"));
    assert_eq!(titles(&r["tasks"]), ["吃维生素"]);
}

#[test]
fn editing_a_series_rebuilds_only_future_open_instances() {
    let mut s = store();
    let sid = series(&mut s, json!({"title":"跑步","rule":"daily","remind_time":"06:00"}));
    let c = at(NOW);
    let r = call(&mut s, "list_tasks", json!({"from":"2026-11-11","to":"2026-11-14"}), &c);
    let future_done = id(&r["tasks"][2]); // 11-13
    call(&mut s, "complete_task", json!({"id": future_done}), &c);

    let r = call(
        &mut s,
        "update_recurring",
        json!({"id": sid, "title":"夜跑","rule":"weekly:4,5,6","remind_time":"20:00"}),
        &c,
    );
    assert_eq!(r["series"]["title"], "夜跑");
    assert_eq!(r["series"]["rule"], "weekly:4,5,6");

    let r = call(&mut s, "list_tasks", json!({"from":"2026-11-11","to":"2026-11-14"}), &c);
    let got: Vec<(String, String, String)> = r["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["date"].as_str().unwrap().into(),
                t["title"].as_str().unwrap().into(),
                t["remind_at"].as_str().unwrap().into(),
            )
        })
        .collect();
    let row = |d: &str, t: &str, r: &str| (d.to_string(), t.to_string(), r.to_string());
    assert_eq!(
        got,
        [
            row("2026-11-11", "跑步", "2026-11-11T06:00:00"),  // today: untouched
            row("2026-11-12", "夜跑", "2026-11-12T20:00:00"),  // Thu, rebuilt
            row("2026-11-13", "跑步", "2026-11-13T06:00:00"),  // done: kept as is
            row("2026-11-14", "夜跑", "2026-11-14T20:00:00"),  // Sat, rebuilt
        ]
    );
}

#[test]
fn deactivating_and_deleting_a_series_clear_future_instances() {
    let mut s = store();
    let sid = series(&mut s, json!({"title":"冥想","rule":"daily"}));
    let c = at(NOW);
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-13", NOW).len(), 3);

    let r = call(&mut s, "update_recurring", json!({"id": sid, "active": false}), &c);
    assert_eq!(r["series"]["active"], false);
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-13", NOW), ["2026-11-11"]);

    call(&mut s, "update_recurring", json!({"id": sid, "active": true}), &c);
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-13", NOW).len(), 3);

    let r = call(&mut s, "delete_recurring", json!({"id": sid}), &c);
    assert_eq!(r["series"]["id"], sid);
    assert_eq!(dates(&mut s, "2026-11-11", "2026-11-13", NOW), ["2026-11-11"]);
    let r = call(&mut s, "list_recurring", json!({}), &c);
    assert_eq!(r["series"], json!([]));
    let e = call_err(&mut s, "delete_recurring", json!({"id": sid}), &c);
    assert_eq!(e.code, "not_found");
}

#[test]
fn list_recurring_shows_series_fields() {
    let mut s = store();
    series(&mut s, json!({"title":"例会","rule":"weekly:1","remind_time":"9:30","start_date":"2026-11-16","end_date":"2026-12-31"}));
    let r = call(&mut s, "list_recurring", json!({}), &at(NOW));
    let x = &r["series"][0];
    assert_eq!(x["title"], "例会");
    assert_eq!(x["rule"], "weekly:1");
    assert_eq!(x["remind_time"], "09:30");
    assert_eq!(x["start_date"], "2026-11-16");
    assert_eq!(x["end_date"], "2026-12-31");
    assert_eq!(x["active"], true);
    assert_eq!(x["source"], "codex");
    assert!(x["note"].is_null());
    assert!(x["created_at"].is_string());
}

#[test]
fn invalid_rules_are_rejected() {
    let mut s = store();
    for rule in ["hourly", "weekly:", "weekly:0", "weekly:8", "monthly:0", "monthly:32", "monthly:x"] {
        let e = call_err(&mut s, "add_recurring", json!({"title":"x","rule":rule}), &at(NOW));
        assert_eq!(e.code, "invalid_argument", "{rule}");
    }
}
