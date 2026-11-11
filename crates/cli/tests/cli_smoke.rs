mod common;
use common::*;
use serde_json::Value;

fn run_json(dir: &std::path::PathBuf, args: &[&str]) -> Value {
    let o = cli(dir, args, None);
    let text = stdout(&o);
    assert!(o.status.success(), "{args:?} failed: {text} {}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{args:?} not JSON ({e}): {text}"))
}

#[test]
fn add_list_done_summary_round_trip_with_chinese() {
    let dir = data_dir("roundtrip");
    let added = run_json(&dir, &["add", "给妈妈打电话", "--remind", "20:00", "--note", "问问体检结果", "--important"]);
    let task = &added["tasks"][0];
    let today = added["today"].as_str().unwrap().to_string();
    assert_eq!(task["title"], "给妈妈打电话");
    assert_eq!(task["note"], "问问体检结果");
    assert_eq!(task["priority"], 1);
    assert_eq!(task["source"], "cli");
    assert_eq!(task["remind_at"], format!("{today}T20:00:00"));
    let id = task["id"].as_i64().unwrap().to_string();

    let listed = run_json(&dir, &["list"]);
    assert_eq!(listed["tasks"][0]["title"], "给妈妈打电话");

    let done = run_json(&dir, &["--source", "codex", "done", &id]);
    assert_eq!(done["task"]["status"], "done");
    assert_eq!(done["task"]["completed_by"], "codex");

    let summary = run_json(&dir, &["summary"]);
    assert_eq!(summary["from"], today.as_str());
    assert_eq!(summary["total"], 1);
    assert_eq!(summary["done"], 1);
    assert!(dir.join("daynote.db").exists());
}

#[test]
fn call_accepts_inline_file_and_stdin_json() {
    let dir = data_dir("call");
    run_json(&dir, &["call", "set_day_note", r#"{"content":"内联"}"#]);
    let file = dir.join("args.json");
    std::fs::write(&file, "\u{feff}{\"content\":\"文件\"}").unwrap();
    let arg = format!("@{}", file.display());
    let r = run_json(&dir, &["call", "set_day_note", &arg]);
    assert_eq!(r["content"], "文件");
    let o = cli(&dir, &["call", "set_day_note", "-"], Some(r#"{"content":"标准输入"}"#));
    let r: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(r["content"], "标准输入");
    // internal tools are reachable from the CLI
    let r = run_json(&dir, &["call", "get_settings"]);
    assert!(r["settings"].is_object());
}

#[test]
fn errors_are_json_on_stdout_with_nonzero_exit() {
    let dir = data_dir("errors");
    for args in [
        &["done", "999"][..],
        &["call", "nope"][..],
        &["call", "list_tasks", "{bad"][..],
        &["frobnicate"][..],
        &["list", "--status"][..],
    ] {
        let o = cli(&dir, args, None);
        assert!(!o.status.success(), "{args:?} should fail");
        let v: Value = serde_json::from_str(&stdout(&o)).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert!(v["error"]["code"].is_string(), "{args:?}: {v}");
        assert!(v["error"]["message"].is_string());
    }
    let o = cli(&dir, &["done", "999"], None);
    let v: Value = serde_json::from_str(&stdout(&o)).unwrap();
    assert_eq!(v["error"]["code"], "not_found");
}

#[test]
fn tools_lists_only_public_tools() {
    let dir = data_dir("tools");
    let v = run_json(&dir, &["tools"]);
    let names: Vec<&str> = v["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"list_tasks"));
    assert!(!names.contains(&"due_reminders"));
    assert!(v["tools"][0]["inputSchema"].is_object());
}
