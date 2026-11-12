//! Transport-independent MCP messages and public tool routing.
//!
//! The HTTP adapter supplies a per-request source and shared app backend. The legacy
//! stdio adapter supplies its own source and directory-following store. Neither owns
//! a separate copy of the tool catalog or tool result/error rules.
use crate::api::{self, ApiError};
use serde_json::{json, Value};

pub const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const HTTP_SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];

const INSTRUCTIONS: &str = "DayNote 是用户本机的每日备忘/待办工具：按天记录任务（可带一层子任务、提醒、重要标记）、重复任务和每天一段便签。所有日期时间都是本机本地时间（无时区），省略 date 即表示今天；每个返回都带 today 和 now，请以它们为准，不要自行推算日期。写入前请先用 list_tasks 或 get_summary 查看已有内容，避免重复添加。";

pub fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message.into()}})
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}

pub fn tools_list() -> Value {
    let tools: Vec<Value> = api::tool_specs().into_iter().filter(|tool| tool.public)
        .map(|tool| json!({"name":tool.name, "description":tool.description, "inputSchema":tool.input_schema}))
        .collect();
    json!({"tools":tools})
}

/// Source tags are descriptive, never authority. The GUI's reserved names cannot
/// be claimed by either MCP transport or by REST callers.
pub fn source_tag(claimed: &str, fallback: &str) -> String {
    let claimed = claimed.trim().to_lowercase();
    if claimed.is_empty() || claimed == "user" || claimed == "app" {
        fallback.into()
    } else {
        claimed
    }
}

pub fn source_from_client(name: &str) -> String {
    let name = source_tag(name, "mcp");
    if name.contains("claude") {
        "claude-code".into()
    } else if name.contains("codex") {
        "codex".into()
    } else {
        name
    }
}

fn valid_id(id: &Value) -> bool {
    id.is_string() || id.is_i64() || id.is_u64()
}

/// Route one JSON-RPC message. A notification or client response has no reply and
/// never invokes a tool. Adapters decide whether their negotiated revision permits
/// a batch; this function deliberately accepts only a single message.
pub fn handle(
    message: Value,
    supported_versions: &[&str],
    server_version: &str,
    mut call: impl FnMut(&str, Value) -> Result<Value, ApiError>,
) -> Option<Value> {
    let Value::Object(obj) = message else {
        return Some(error(Value::Null, -32600, "Invalid Request"));
    };
    let id = obj.get("id");
    let reply_id = id.filter(|id| valid_id(id)).cloned().unwrap_or(Value::Null);
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || id.is_some_and(|id| !valid_id(id))
    {
        return Some(error(reply_id, -32600, "Invalid Request"));
    }
    let Some(method) = obj.get("method") else {
        // This server never sends requests, but valid client responses are accepted
        // without replying, as required by the HTTP transport.
        let result = obj.get("result");
        let failure = obj.get("error");
        let valid_error = failure.is_some_and(|value| {
            value.is_object()
                && value
                    .get("code")
                    .is_some_and(|code| code.is_i64() || code.is_u64())
                && value.get("message").is_some_and(Value::is_string)
        });
        if id.is_some()
            && ((result.is_some_and(Value::is_object) && failure.is_none())
                || (result.is_none() && valid_error))
        {
            return None;
        }
        return Some(error(reply_id, -32600, "Invalid Request"));
    };
    let Some(method) = method.as_str() else {
        return Some(error(reply_id, -32600, "Invalid Request"));
    };
    if obj.contains_key("result") || obj.contains_key("error") {
        return Some(error(reply_id, -32600, "Invalid Request"));
    }
    let Some(id) = id.cloned() else { return None };
    let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Some(error(id, -32602, "params must be an object"));
    }
    Some(match method {
        "initialize" => {
            let Some(requested) = params.get("protocolVersion").and_then(Value::as_str) else {
                return Some(error(id, -32602, "initialize requires protocolVersion"));
            };
            if !params.get("capabilities").is_some_and(Value::is_object)
                || !params
                    .pointer("/clientInfo/name")
                    .is_some_and(Value::is_string)
                || !params
                    .pointer("/clientInfo/version")
                    .is_some_and(Value::is_string)
            {
                return Some(error(
                    id,
                    -32602,
                    "initialize requires capabilities and clientInfo.name/version",
                ));
            }
            let version = if supported_versions.contains(&requested) {
                requested
            } else {
                supported_versions[0]
            };
            ok(
                id,
                json!({
                    "protocolVersion":version,
                    "capabilities":{"tools":{}},
                    "serverInfo":{"name":"daynote", "version":server_version},
                    "instructions":INSTRUCTIONS,
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, tools_list()),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, -32602, "tools/call requires name"));
            };
            if !api::tool_specs()
                .iter()
                .any(|tool| tool.public && tool.name == name)
            {
                return Some(error(id, -32602, format!("Unknown tool: {name}")));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !args.is_object() {
                return Some(error(id, -32602, "arguments must be an object"));
            }
            let (payload, is_error) = match call(name, args) {
                Ok(value) => (value, false),
                Err(error) => (error.to_json(), true),
            };
            ok(
                id,
                json!({"content":[{"type":"text", "text":payload.to_string()}], "isError":is_error}),
            )
        }
        _ => error(id, -32601, format!("Method not found: {method}")),
    })
}
