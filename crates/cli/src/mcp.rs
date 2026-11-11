//! MCP over stdio: newline-delimited JSON-RPC 2.0, hand-written on serde_json.
//! stdout carries protocol messages only; diagnostics go to stderr.
use daynote_core::api::{self, ApiError, Ctx};
use daynote_core::{now_local, Store};
use serde_json::{json, Value};
use std::io::BufRead;

/// Handshake-based revisions whose `initialize` / `tools/*` shapes we implement.
/// (2026-07-28 dropped `initialize` for per-request `_meta`, so it is not listed.)
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const LATEST: &str = "2025-11-25";

const INSTRUCTIONS: &str = "DayNote 是用户本机的每日备忘/待办工具：按天记录任务（可带一层子任务、提醒、重要标记）、重复任务和每天一段便签。所有日期时间都是本机本地时间（无时区），省略 date 即表示今天；每个返回都带 today 和 now，请以它们为准，不要自行推算日期。写入前请先用 list_tasks 或 get_summary 查看已有内容，避免重复添加。";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

struct Server {
    source_override: Option<String>,
    source: String,
    store: Option<Store>,
}

pub fn serve(source_override: Option<String>) -> i32 {
    let mut server = Server {
        source: source_override.clone().unwrap_or_else(|| "mcp".into()),
        source_override,
        store: None,
    };
    match daynote_core::data_dir() {
        Ok(dir) => eprintln!("daynote mcp: ready, data dir {}", dir.display()),
        Err(e) => {
            eprintln!("daynote mcp: invalid data directory configuration: {e}");
            return 1;
        }
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match lines.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("daynote mcp: stdin error: {e}");
                return 1;
            }
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(line) {
            send(&reply);
        }
    }
    0
}

fn send(v: &Value) {
    // serde_json escapes control characters, so the compact form is a single line.
    let text = serde_json::to_string(v).unwrap_or_else(|_| "{}".into());
    crate::write_stdout(&format!("{text}\n"));
}

fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// Map an MCP client name to a `source` tag.
fn source_from_client(name: &str) -> String {
    let n = name.trim().to_lowercase();
    if n.contains("claude") {
        "claude-code".into()
    } else if n.contains("codex") {
        "codex".into()
    } else if n.is_empty() {
        "mcp".into()
    } else {
        n
    }
}

impl Server {
    fn handle_line(&mut self, line: &str) -> Option<Value> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(Value::Null, PARSE_ERROR, format!("Parse error: {e}"))),
        };
        match msg {
            // JSON-RPC batch (allowed by 2025-03-26).
            Value::Array(items) => {
                if items.is_empty() {
                    return Some(error(Value::Null, INVALID_REQUEST, "Empty batch"));
                }
                let replies: Vec<Value> = items.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            m => self.handle(m),
        }
    }

    fn handle(&mut self, msg: Value) -> Option<Value> {
        let Value::Object(obj) = msg else {
            return Some(error(Value::Null, INVALID_REQUEST, "Invalid Request"));
        };
        let method = obj.get("method").and_then(Value::as_str);
        let id = obj.get("id").cloned();
        let (Some(method), Some(id)) = (method, id) else {
            // Notifications (no id) and responses (no method) get no reply.
            return None;
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        Some(match method {
            "initialize" => ok(id, self.initialize(&params)),
            "ping" => ok(id, json!({})),
            "tools/list" => ok(id, tools_list()),
            "tools/call" => self.tools_call(id, &params),
            _ => error(id, METHOD_NOT_FOUND, format!("Method not found: {method}")),
        })
    }

    fn initialize(&mut self, params: &Value) -> Value {
        let requested = params.get("protocolVersion").and_then(Value::as_str);
        let version = match requested {
            Some(v) if SUPPORTED_VERSIONS.contains(&v) => v,
            _ => LATEST,
        };
        if self.source_override.is_none() {
            let name = params
                .pointer("/clientInfo/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            self.source = source_from_client(name);
        }
        eprintln!(
            "daynote mcp: initialize version={version} source={}",
            self.source
        );
        json!({
            "protocolVersion": version,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "daynote", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        })
    }

    fn tools_call(&mut self, id: Value, params: &Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error(id, INVALID_PARAMS, "tools/call 需要 name");
        };
        if !api::tool_specs().iter().any(|t| t.public && t.name == name) {
            return error(id, INVALID_PARAMS, format!("Unknown tool: {name}"));
        }
        let args = params.get("arguments").cloned().unwrap_or(Value::Null);
        let ctx = Ctx {
            source: self.source.clone(),
            now: now_local(),
        };
        let outcome = self
            .store()
            .and_then(|store| api::dispatch(store, name, args, &ctx));
        let (payload, is_error) = match outcome {
            Ok(v) => (v, false),
            Err(e) => (e.to_json(), true),
        };
        let text = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
        ok(
            id,
            json!({"content": [{"type": "text", "text": text}], "isError": is_error}),
        )
    }

    /// Open the database on first use and keep it open for the process lifetime.
    fn store(&mut self) -> Result<&mut Store, ApiError> {
        if self.store.is_none() {
            let s = Store::open_default()
                .map_err(|e| ApiError::internal(format!("打开数据库失败：{e}")))?;
            self.store = Some(s);
        }
        Ok(self.store.as_mut().expect("store just opened"))
    }
}

fn tools_list() -> Value {
    let tools: Vec<Value> = api::tool_specs()
        .into_iter()
        .filter(|t| t.public)
        .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.input_schema}))
        .collect();
    json!({ "tools": tools })
}
