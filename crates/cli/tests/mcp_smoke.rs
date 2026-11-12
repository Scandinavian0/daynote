//! Pipes a scripted MCP session into `daynote-cli mcp` and checks every reply line.
mod common;
use common::*;
use serde_json::{json, Value};

fn session(
    dir: &std::path::PathBuf,
    args: &[&str],
    messages: &[Value],
    raw_extra: &[&str],
) -> Vec<Value> {
    let mut input = String::new();
    for m in messages {
        input.push_str(&serde_json::to_string(m).unwrap());
        input.push('\n');
    }
    for r in raw_extra {
        input.push_str(r);
        input.push('\n');
    }
    let mut full = vec!["mcp"];
    full.extend_from_slice(args);
    let o = cli(dir, &full, Some(&input));
    assert!(o.status.success(), "mcp exited with {:?}", o.status);
    stdout(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad line ({e}): {l}")))
        .collect()
}

fn tool_payload(reply: &Value) -> Value {
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(reply["result"]["content"][0]["type"], "text");
    serde_json::from_str(text).unwrap()
}

#[test]
fn scripted_session_gets_exactly_one_line_per_request() {
    let dir = data_dir("mcp-session");
    let replies = session(
        &dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"claude-code","version":"2.0.0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_tasks","arguments":{
                "tasks":[{"title":"准备周会","remind_at":"16:00","subtasks":["整理数据","写提纲"]}]}}}),
            json!({"jsonrpc":"2.0","id":"four","method":"tools/call","params":{"name":"list_tasks","arguments":{}}}),
        ],
        &[],
    );
    assert_eq!(replies.len(), 4, "{replies:#?}");
    let ids: Vec<&Value> = replies.iter().map(|r| &r["id"]).collect();
    assert_eq!(ids, [&json!(1), &json!(2), &json!(3), &json!("four")]);
    for r in &replies {
        assert_eq!(r["jsonrpc"], "2.0");
    }

    let init = &replies[0]["result"];
    assert_eq!(init["protocolVersion"], "2025-06-18");
    assert_eq!(init["capabilities"], json!({"tools": {}}));
    assert_eq!(init["serverInfo"]["name"], "daynote");
    assert!(init["instructions"].as_str().unwrap().contains("本地时间"));

    let tools = replies[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"add_tasks") && names.contains(&"get_summary"));
    assert!(
        !names.contains(&"snooze_task"),
        "internal tools stay hidden"
    );
    assert!(tools
        .iter()
        .all(|t| t["inputSchema"]["type"] == "object" && t["description"].is_string()));

    assert_eq!(replies[2]["result"]["isError"], false);
    let added = tool_payload(&replies[2]);
    let task = &added["tasks"][0];
    assert_eq!(task["title"], "准备周会");
    assert_eq!(task["source"], "claude-code");
    assert_eq!(task["subtasks"].as_array().unwrap().len(), 2);
    assert!(added["today"].is_string() && added["now"].is_string());

    let listed = tool_payload(&replies[3]);
    assert_eq!(listed["tasks"][0]["title"], "准备周会");
    assert_eq!(listed["tasks"][0]["subtasks"][1]["title"], "写提纲");
}

#[test]
fn protocol_edge_cases() {
    let dir = data_dir("mcp-edges");
    let replies = session(
        &dir,
        &["--source", "my-agent"],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01","capabilities":{},"clientInfo":{"name":"Codex CLI","version":"1"}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
            json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}),
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"due_reminders","arguments":{}}}),
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"complete_task","arguments":{"id":12345}}}),
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"add_tasks","arguments":{"tasks":[{"title":"换行\n标题"}]}}}),
        ],
        &["{not json"],
    );
    assert_eq!(replies.len(), 7, "{replies:#?}");
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(replies[1]["result"], json!({}));
    assert_eq!(replies[2]["error"]["code"], -32601);
    assert_eq!(
        replies[3]["error"]["code"], -32602,
        "internal tool is not callable over MCP"
    );
    assert_eq!(replies[4]["result"]["isError"], true);
    let err = tool_payload(&replies[4]);
    assert_eq!(err["error"]["code"], "not_found");
    let added = tool_payload(&replies[5]);
    assert_eq!(added["tasks"][0]["title"], "换行\n标题");
    assert_eq!(
        added["tasks"][0]["source"], "my-agent",
        "--source overrides clientInfo"
    );
    assert_eq!(replies[6]["error"]["code"], -32700);
    assert!(replies[6]["id"].is_null());
}

#[test]
fn client_name_maps_to_source() {
    let dir = data_dir("mcp-source");
    let replies = session(
        &dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"Codex-MCP-Client","version":"1"}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_tasks","arguments":{"tasks":[{"title":"x"}]}}}),
        ],
        &[],
    );
    assert_eq!(replies[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(tool_payload(&replies[1])["tasks"][0]["source"], "codex");
}

#[test]
fn legacy_stdio_exits_at_eof_without_emitting_a_reply() {
    let dir = data_dir("mcp-eof");
    assert!(session(&dir, &[], &[], &[]).is_empty());
    assert!(session(
        &dir,
        &[],
        &[json!({"jsonrpc":"2.0","method":"notifications/initialized"})],
        &[]
    )
    .is_empty());
}

#[test]
fn legacy_stdio_uses_shared_validation_and_protects_reserved_sources() {
    let dir = data_dir("mcp-reserved");
    for source in ["user", "APP"] {
        let replies = session(
            &dir,
            &["--source", source],
            &[
                json!({"id":1,"method":"ping"}),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_tasks","arguments":{"tasks":[{"title":"reserved source"}]}}}),
            ],
            &[],
        );
        assert_eq!(replies[0]["error"]["code"], -32600);
        assert_eq!(tool_payload(&replies[1])["tasks"][0]["source"], "mcp");
    }
    let replies = session(
        &dir,
        &[],
        &[
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"user","version":"1"}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_tasks","arguments":{"tasks":[{"title":"client source"}]}}}),
        ],
        &[],
    );
    assert_eq!(tool_payload(&replies[1])["tasks"][0]["source"], "mcp");
}
