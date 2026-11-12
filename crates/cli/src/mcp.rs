//! Legacy MCP over stdio. stdout carries protocol messages only; diagnostics use stderr.
use daynote_core::api::{self, ApiError, Ctx};
use daynote_core::{mcp, now_local, Store};
use serde_json::Value;
use std::io::BufRead;
use std::path::PathBuf;

struct Server {
    source_override: Option<String>,
    source: String,
    /// The open database and the directory it was opened from.
    store: Option<(PathBuf, Store)>,
}

pub fn serve(source_override: Option<String>) -> i32 {
    let mut server = Server {
        source: mcp::source_tag(source_override.as_deref().unwrap_or(""), "mcp"),
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
        let line = match std::str::from_utf8(&buf) {
            Ok(line) => line.trim().trim_start_matches('\u{feff}'),
            Err(_) => {
                send(&mcp::error(Value::Null, -32700, "Request is not UTF-8"));
                continue;
            }
        };
        if line.is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(line) {
            send(&reply);
        }
    }
    0
}

fn send(value: &Value) {
    // Compact JSON escapes control characters and therefore occupies one line.
    crate::write_stdout(&format!("{value}\n"));
}

impl Server {
    fn handle_line(&mut self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                return Some(mcp::error(
                    Value::Null,
                    -32700,
                    format!("Parse error: {error}"),
                ))
            }
        };
        match message {
            // Preserve the legacy stdio batch support for existing clients.
            Value::Array(items) if !items.is_empty() => {
                let replies: Vec<_> = items
                    .into_iter()
                    .filter_map(|message| self.handle(message))
                    .collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            message => self.handle(message),
        }
    }

    fn handle(&mut self, message: Value) -> Option<Value> {
        let client_name = (message.get("method").and_then(Value::as_str) == Some("initialize"))
            .then(|| {
                message
                    .pointer("/params/clientInfo/name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .flatten();
        let source = self.source.clone();
        let reply = mcp::handle(
            message,
            mcp::SUPPORTED_VERSIONS,
            env!("CARGO_PKG_VERSION"),
            |name, args| {
                self.store().and_then(|store| {
                    api::dispatch(
                        store,
                        name,
                        args,
                        &Ctx {
                            source: source.clone(),
                            now: now_local(),
                        },
                    )
                })
            },
        );
        if let Some(version) = reply
            .as_ref()
            .and_then(|reply| reply.pointer("/result/protocolVersion"))
        {
            if self.source_override.is_none() {
                self.source = mcp::source_from_client(client_name.as_deref().unwrap_or(""));
            }
            eprintln!(
                "daynote mcp: initialize version={version} source={}",
                self.source
            );
        }
        reply
    }

    /// Resolve the directory on every call so existing legacy clients follow a
    /// directory switch after the desktop app restarts.
    fn store(&mut self) -> Result<&mut Store, ApiError> {
        let dir = daynote_core::data_dir()
            .map_err(|e| ApiError::internal(format!("读取数据目录失败：{e}")))?;
        if self.store.as_ref().is_none_or(|(open, _)| *open != dir) {
            self.store = None;
            let store = Store::open(&dir.join("daynote.db"))
                .map_err(|e| ApiError::internal(format!("打开数据库失败：{e}")))?;
            eprintln!("daynote mcp: data dir {}", dir.display());
            self.store = Some((dir, store));
        }
        Ok(&mut self.store.as_mut().expect("store just opened").1)
    }
}
