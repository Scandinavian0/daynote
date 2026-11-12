//! Local HTTP API on 127.0.0.1. [`handle`] turns a request into a status code and a
//! JSON body without touching sockets, so the security checks and routing are unit tested;
//! [`Server`] provides bounded HTTP/1 transport around it.
use daynote_core::api::{tool_specs, ApiError};
use daynote_core::mcp;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Tools whose success never changes data (no `data-changed` broadcast).
pub const READ_ONLY_TOOLS: &[&str] = &[
    "list_tasks",
    "get_summary",
    "get_day_note",
    "list_recurring",
    "get_settings",
    "due_reminders",
];

pub fn is_write(tool: &str) -> bool {
    !READ_ONLY_TOOLS.contains(&tool)
}

/// Runs a tool; implemented by the app over the shared store, and by a fake in tests.
pub trait Backend {
    fn call(&self, tool: &str, args: Value, source: &str) -> Result<Value, ApiError>;
}

pub struct Req<'a> {
    pub method: &'a str,
    /// Path plus optional query, as sent on the request line.
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

#[derive(Debug, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
    /// A write tool succeeded: the caller should broadcast `data-changed`.
    pub wrote: bool,
}

pub struct Guard<'a> {
    pub port: u16,
    pub token: &'a str,
    pub version: &'a str,
}

pub const MAX_BODY: usize = 1 << 20;

fn error(status: u16, code: &str, message: impl Into<String>) -> Reply {
    Reply {
        status,
        body: json!({"error": {"code": code, "message": message.into()}}),
        wrote: false,
    }
}

fn ok(body: Value) -> Reply {
    Reply {
        status: 200,
        body,
        wrote: false,
    }
}

/// HTTP status for a daynote-core error code.
pub fn status_of(code: &str) -> u16 {
    match code {
        "invalid_argument" => 400,
        "not_found" => 404,
        _ => 500,
    }
}

fn api_error(e: &ApiError) -> Reply {
    error(status_of(e.code), e.code, e.message.clone())
}

/// Constant-time comparison (length is not secret: tokens are fixed-size).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header<'a>(req: &'a Req, name: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn host_ok(host: &str, port: u16) -> bool {
    let host = host.trim().to_ascii_lowercase();
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

fn bearer_ok(req: &Req, token: &str) -> bool {
    let Some(v) = header(req, "authorization") else {
        return false;
    };
    let v = v.trim();
    let Some(scheme) = v.get(..7) else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer ") {
        return false;
    }
    !token.is_empty() && ct_eq(v[7..].trim().as_bytes(), token.as_bytes())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push((hi * 16 + lo) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_args(query: &str, allowed: &[&str]) -> Map<String, Value> {
    let mut m = Map::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (percent_decode(k), percent_decode(v));
        let v = v.trim();
        if allowed.contains(&k.as_str()) && !v.is_empty() {
            m.insert(k, Value::String(v.to_string()));
        }
    }
    m
}

/// Who to record as the writer. `user` and `app` are reserved for the person at the
/// panel and for the shell itself, so an HTTP caller cannot pass itself off as either.
fn source_of(req: &Req) -> String {
    mcp::source_tag(header(req, "x-daynote-source").unwrap_or(""), "http")
}

fn path_of(url: &str) -> &str {
    url.split_once('?').map_or(url, |(path, _)| path)
}

/// The three checks every request must pass, decided from the request line and headers
/// alone so the server can refuse before it reads a body. `None` means "let it through".
pub fn gate(req: &Req, g: &Guard) -> Option<Reply> {
    // 1. Browsers always send Origin on cross-site requests: refuse them all.
    if header(req, "origin").is_some() {
        return Some(error(403, "forbidden", "不接受来自网页的请求"));
    }
    // 2. DNS-rebinding guard.
    if !header(req, "host").is_some_and(|h| host_ok(h, g.port)) {
        return Some(error(403, "forbidden", "Host 头不正确"));
    }
    // 3. Everything except the health probe needs the token.
    if path_of(req.url) != "/v1/health" && !bearer_ok(req, g.token) {
        return Some(error(
            401,
            "unauthorized",
            "缺少或错误的令牌（Authorization: Bearer <令牌>）",
        ));
    }
    None
}

fn run(backend: &dyn Backend, tool: &str, args: Value, source: &str) -> Reply {
    match backend.call(tool, args, source) {
        Ok(v) => Reply {
            status: 200,
            body: v,
            wrote: is_write(tool),
        },
        Err(e) => api_error(&e),
    }
}

/// Decide the response for one request.
pub fn handle(req: &Req, g: &Guard, backend: &dyn Backend) -> Reply {
    if let Some(refused) = gate(req, g) {
        return refused;
    }
    let (path, query) = req.url.split_once('?').unwrap_or((req.url, ""));
    let method = req.method.to_ascii_uppercase();
    let only = |m: &str| method == m;

    if path == "/mcp" {
        return handle_mcp(req, g, backend);
    }

    if path == "/v1/health" {
        if !only("GET") {
            return error(405, "method_not_allowed", "只支持 GET");
        }
        return ok(json!({"ok": true, "version": g.version}));
    }

    match path {
        "/v1/tools" => {
            if !only("GET") {
                return error(405, "method_not_allowed", "只支持 GET");
            }
            ok(mcp::tools_list())
        }
        "/v1/tasks" | "/v1/summary" => {
            if !only("GET") {
                return error(405, "method_not_allowed", "只支持 GET");
            }
            let (tool, allowed): (&str, &[&str]) = if path == "/v1/tasks" {
                ("list_tasks", &["date", "from", "to", "status"])
            } else {
                ("get_summary", &["date", "from", "to"])
            };
            run(
                backend,
                tool,
                Value::Object(query_args(query, allowed)),
                &source_of(req),
            )
        }
        _ => {
            let Some(tool) = path.strip_prefix("/v1/call/") else {
                return error(404, "not_found", format!("没有这个接口：{path}"));
            };
            if !only("POST") {
                return error(405, "method_not_allowed", "只支持 POST");
            }
            let public = tool_specs().into_iter().any(|t| t.public && t.name == tool);
            if !public {
                return error(404, "not_found", format!("没有这个工具：{tool}"));
            }
            let text = match std::str::from_utf8(req.body) {
                Ok(t) => t.trim_start_matches('\u{feff}').trim(),
                Err(_) => return error(400, "invalid_argument", "请求体不是 UTF-8"),
            };
            let args = if text.is_empty() {
                json!({})
            } else {
                match serde_json::from_str(text) {
                    Ok(v) => v,
                    Err(e) => {
                        return error(400, "invalid_argument", format!("请求体不是合法 JSON：{e}"))
                    }
                }
            };
            run(backend, tool, args, &source_of(req))
        }
    }
}

/// Stateless Streamable HTTP: every request supplies its source, so another
/// client's initialize cannot change who is recorded as the writer.
fn handle_mcp(req: &Req, g: &Guard, backend: &dyn Backend) -> Reply {
    if let Some(refused) = mcp_gate(req) {
        return refused;
    }
    let message = match serde_json::from_slice(req.body) {
        Ok(message) => message,
        Err(error) => {
            return Reply {
                status: 400,
                body: mcp::error(Value::Null, -32700, format!("Parse error: {error}")),
                wrote: false,
            }
        }
    };
    let source = mcp::source_tag(header(req, "x-daynote-source").unwrap_or(""), "mcp");
    let mut wrote = false;
    let mut call = |name: &str, args| {
        let result = backend.call(name, args, &source);
        wrote |= result.is_ok() && is_write(name);
        result
    };
    let reply = match message {
        // This older revision allowed batches. Later revisions require one message
        // per POST; a missing version header explicitly falls back to 2025-03-26.
        Value::Array(items)
            if !items.is_empty()
                && header(req, "mcp-protocol-version").unwrap_or("2025-03-26") == "2025-03-26" =>
        {
            let replies: Vec<_> = items
                .into_iter()
                .filter_map(|message| {
                    mcp::handle(message, mcp::HTTP_SUPPORTED_VERSIONS, g.version, &mut call)
                })
                .collect();
            (!replies.is_empty()).then_some(Value::Array(replies))
        }
        message => mcp::handle(message, mcp::HTTP_SUPPORTED_VERSIONS, g.version, call),
    };
    match reply {
        Some(body) => Reply {
            status: if body.pointer("/error/code").and_then(Value::as_i64) == Some(-32600) {
                400
            } else {
                200
            },
            body,
            wrote,
        },
        None => Reply {
            status: 202,
            body: Value::Null,
            wrote: false,
        },
    }
}

/// The Streamable HTTP transport's request headers are checked before reading a
/// body, under the same authentication/Origin/Host gate as REST.
fn mcp_gate(req: &Req) -> Option<Reply> {
    let version = header(req, "mcp-protocol-version").unwrap_or("2025-03-26");
    if !mcp::HTTP_SUPPORTED_VERSIONS.contains(&version) {
        return Some(error(
            400,
            "unsupported_protocol_version",
            "不支持这个 MCP 协议版本",
        ));
    }
    if !req.method.eq_ignore_ascii_case("POST") {
        return Some(error(
            405,
            "method_not_allowed",
            "MCP 只支持 POST，不提供 SSE 流或会话删除",
        ));
    }
    let json_type = header(req, "content-type")
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if !json_type {
        return Some(error(
            415,
            "unsupported_media_type",
            "Content-Type 必须是 application/json",
        ));
    }
    let accepts = |wanted: &str| {
        req.headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("accept"))
            .flat_map(|(_, value)| value.split(','))
            .any(|entry| {
                let mut parts = entry.split(';');
                parts
                    .next()
                    .is_some_and(|media| media.trim().eq_ignore_ascii_case(wanted))
                    && parts.all(|parameter| match parameter.trim().split_once('=') {
                        Some((name, value)) if name.trim().eq_ignore_ascii_case("q") => value
                            .trim()
                            .parse::<f32>()
                            .is_ok_and(|q| q > 0.0 && q <= 1.0),
                        _ => true,
                    })
            })
    };
    if !accepts("application/json") || !accepts("text/event-stream") {
        return Some(error(
            406,
            "not_acceptable",
            "Accept 必须包含 application/json 和 text/event-stream",
        ));
    }
    None
}

// Token ---------------------------------------------------------------------

pub fn token_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("api-token.txt")
}

fn is_token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Read `<data_dir>\api-token.txt`, creating it with 32 random bytes (hex) if missing.
pub fn load_or_create_token(data_dir: &std::path::Path) -> Result<String, String> {
    let path = token_path(data_dir);
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim();
        if is_token(s) {
            return Ok(s.to_string());
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("系统随机源不可用：{e}"))?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let token: String = bytes
        .iter()
        .flat_map(|b| {
            [
                HEX[usize::from(b >> 4)] as char,
                HEX[usize::from(b & 15)] as char,
            ]
        })
        .collect();
    std::fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    std::fs::write(&path, &token).map_err(|e| format!("写入令牌文件失败：{e}"))?;
    Ok(token)
}

// Server loop ---------------------------------------------------------------

/// Replace the token with an atomic same-directory rename; a failed write leaves
/// the current file and live authentication untouched.
pub fn replace_token(data_dir: &std::path::Path) -> Result<String, String> {
    use std::io::Write;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("系统随机源不可用：{e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|e| format!("系统随机源不可用：{e}"))?;
    let suffix: String = suffix.iter().map(|b| format!("{b:02x}")).collect();
    let temporary = data_dir.join(format!(".api-token-{suffix}.tmp"));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, token_path(data_dir))
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!("更新令牌文件失败：{e}"));
    }
    Ok(token)
}

pub struct Server {
    pub port: u16,
    token: Arc<Mutex<String>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Bind `127.0.0.1:port` and serve on a background thread. `on_write` runs after a
    /// successful write tool.
    pub fn start(
        port: u16,
        token: String,
        version: String,
        backend: Box<dyn Backend + Send>,
        on_write: Box<dyn Fn() + Send>,
    ) -> Result<Server, String> {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let listener = {
            let _enter = runtime.enter();
            tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?
        };
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let token = Arc::new(Mutex::new(token));
        let shared_token = token.clone();
        let backend = Arc::new(Mutex::new(backend));
        let on_write = Arc::new(Mutex::new(on_write));
        let thread = std::thread::Builder::new()
            .name("daynote-http".into())
            .spawn(move || {
                runtime.block_on(async move {
                    let mut connections = tokio::task::JoinSet::new();
                    loop {
                        tokio::select! {
                            biased;
                            _ = &mut stopped => break,
                            _ = connections.join_next(), if !connections.is_empty() => {},
                            accepted = listener.accept() => {
                                let Ok((socket, _)) = accepted else { break };
                                // Bound memory, file descriptors and the number of slow clients.
                                if connections.len() >= 32 { continue; }
                                let token = shared_token.clone();
                                let version = version.clone();
                                let backend = backend.clone();
                                let on_write = on_write.clone();
                                connections.spawn(async move {
                                    let service = hyper::service::service_fn(move |request| {
                                        serve_request(request, port, token.clone(), version.clone(), backend.clone(), on_write.clone())
                                    });
                                    let mut http = hyper::server::conn::http1::Builder::new();
                                    http.keep_alive(false).max_headers(64).max_buf_size(8192);
                                    // One request per connection: this bounds header reads,
                                    // stalled bodies and responses to clients that never read.
                                    let _ = tokio::time::timeout(Duration::from_secs(5), http.serve_connection(hyper_util::rt::TokioIo::new(socket), service)).await;
                                });
                            }
                        }
                    }
                    connections.abort_all();
                    while connections.join_next().await.is_some() {}
                });
            })
            .map_err(|e| e.to_string())?;
        Ok(Server {
            port,
            token,
            stop: Some(stop),
            thread: Some(thread),
        })
    }

    /// Stop serving and wait until the port is released.
    pub fn stop(self) {
        drop(self);
    }

    pub fn set_token(&self, token: String) {
        *crate::state::lock(&self.token) = token;
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

async fn serve_request(
    request: hyper::Request<hyper::body::Incoming>,
    port: u16,
    token: Arc<Mutex<String>>,
    version: String,
    backend: Arc<Mutex<Box<dyn Backend + Send>>>,
    on_write: Arc<Mutex<Box<dyn Fn() + Send>>>,
) -> Result<hyper::Response<http_body_util::Full<hyper::body::Bytes>>, std::convert::Infallible> {
    use http_body_util::BodyExt;
    let (parts, body) = request.into_parts();
    let headers: Vec<_> = parts
        .headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let method = parts.method.as_str();
    let url = parts.uri.to_string();
    let head = Req {
        method,
        url: &url,
        headers: &headers,
        body: &[],
    };
    let current_token = crate::state::lock(&token).clone();
    let guard = Guard {
        port,
        token: &current_token,
        version: &version,
    };
    let reply = if let Some(refused) = gate(&head, &guard) {
        refused
    } else if let Some(refused) = (path_of(&url) == "/mcp").then(|| mcp_gate(&head)).flatten() {
        refused
    } else if method == "GET" {
        handle(&head, &guard, crate::state::lock(&backend).as_ref())
    } else if parts
        .headers
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .is_some_and(|n| n > MAX_BODY as u64)
    {
        error(400, "invalid_argument", "请求体过大")
    } else {
        match tokio::time::timeout(
            Duration::from_secs(4),
            http_body_util::Limited::new(body, MAX_BODY).collect(),
        )
        .await
        {
            Err(_) => error(408, "invalid_argument", "读取请求体超时"),
            Ok(Err(_)) => error(400, "invalid_argument", "请求体过大或读取失败"),
            Ok(Ok(body)) => {
                // Recheck against the live token after awaiting the body: rotation
                // must also invalidate requests that were already uploading.
                let current_token = crate::state::lock(&token);
                let guard = Guard {
                    token: &current_token,
                    ..guard
                };
                let bytes = body.to_bytes();
                handle(
                    &Req {
                        body: &bytes,
                        ..head
                    },
                    &guard,
                    crate::state::lock(&backend).as_ref(),
                )
            }
        }
    };
    if reply.wrote {
        crate::state::lock(&on_write)();
    }
    let body = if reply.status == 202 {
        Vec::new()
    } else {
        serde_json::to_vec(&reply.body).unwrap_or_else(|_| b"{}".to_vec())
    };
    let mut response = hyper::Response::builder()
        .status(reply.status)
        .header("Connection", "close");
    if reply.status != 202 {
        response = response.header("Content-Type", "application/json; charset=utf-8");
    }
    if path_of(&url) == "/mcp" && reply.status == 405 {
        response = response.header("Allow", "POST");
    }
    Ok(response
        .body(http_body_util::Full::new(hyper::body::Bytes::from(body)))
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    struct StoredBackend(Mutex<daynote_core::Store>);

    impl Backend for StoredBackend {
        fn call(&self, tool: &str, args: Value, source: &str) -> Result<Value, ApiError> {
            daynote_core::api::dispatch(
                &mut self.0.lock().unwrap(),
                tool,
                args,
                &daynote_core::api::Ctx {
                    source: source.into(),
                    now: daynote_core::now_local(),
                },
            )
        }
    }

    fn stored_server() -> (
        tempfile::TempDir,
        Server,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let store = daynote_core::Store::open(&dir.path().join("daynote.db")).unwrap();
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = writes.clone();
        let server = Server::start(
            0,
            "test-token".into(),
            "test".into(),
            Box::new(StoredBackend(Mutex::new(store))),
            Box::new(move || {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        )
        .unwrap();
        (dir, server, writes)
    }

    fn mcp_post(port: u16, source: &str, message: Value) -> (u16, String, Value) {
        let mut headers = mcp_headers(port);
        headers.push(("X-DayNote-Source".into(), source.into()));
        mcp_wire(port, "POST", &headers, &message.to_string())
    }

    fn mcp_headers(port: u16) -> Vec<(String, String)> {
        vec![
            ("Host".into(), format!("127.0.0.1:{port}")),
            ("Authorization".into(), "Bearer test-token".into()),
            ("Content-Type".into(), "application/json".into()),
            (
                "Accept".into(),
                "application/json, text/event-stream".into(),
            ),
            ("MCP-Protocol-Version".into(), "2025-11-25".into()),
        ]
    }

    fn mcp_wire(
        port: u16,
        method: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> (u16, String, Value) {
        let mut stream = connect(port);
        let mut request = format!(
            "{method} /mcp HTTP/1.1\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str(&format!("\r\n{body}"));
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = String::new();
        stream.read_to_string(&mut raw).unwrap();
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(body).unwrap()
        };
        (status, head.to_string(), value)
    }

    #[test]
    fn mcp_enforces_http_contract_before_dispatch() {
        let (_dir, server, writes) = stored_server();
        let port = server.port;
        let call = mcp_call(
            1,
            "add_tasks",
            json!({"tasks":[{"title":"must not be added"}]}),
        )
        .to_string();
        for (name, value, status) in [
            ("Content-Type", Some("text/plain"), 415),
            ("Content-Type", None, 415),
            ("Accept", None, 406),
            ("Accept", Some("application/json"), 406),
            (
                "Accept",
                Some("application/json, text/event-stream;q=0"),
                406,
            ),
            ("MCP-Protocol-Version", Some("2099-01-01"), 400),
            ("MCP-Protocol-Version", Some("2024-11-05"), 400),
            ("Host", Some("evil.example"), 403),
            ("Origin", Some("null"), 403),
            ("Origin", Some("https://evil.example"), 403),
            ("Authorization", None, 401),
            ("Authorization", Some("Bearer wrong-token"), 401),
        ] {
            let mut headers = mcp_headers(port);
            headers.retain(|(key, _)| key != name);
            if let Some(value) = value {
                headers.push((name.into(), value.into()));
            }
            let reply = mcp_wire(port, "POST", &headers, &call);
            assert_eq!(reply.0, status, "{name}={value:?}: {}", reply.2);
        }
        for method in ["GET", "DELETE", "PUT"] {
            let (status, head, _) = mcp_wire(port, method, &mcp_headers(port), "");
            assert_eq!(status, 405);
            assert!(head.to_lowercase().contains("allow: post"));
        }
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);
        let mut headers = mcp_headers(port);
        headers.retain(|(name, _)| name != "MCP-Protocol-Version");
        assert_eq!(
            mcp_wire(
                port,
                "POST",
                &headers,
                &json!({"jsonrpc":"2.0","id":1,"method":"ping"}).to_string()
            )
            .0,
            200
        );
    }

    #[test]
    fn mcp_json_rpc_errors_notifications_and_sources_are_isolated() {
        let (_dir, server, writes) = stored_server();
        let port = server.port;
        for (raw, status, code) in [
            ("{", 400, -32700),
            ("[]", 400, -32600),
            ("1", 400, -32600),
            (r#"{"id":7,"method":"ping"}"#, 400, -32600),
            (
                r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
                400,
                -32600,
            ),
            (r#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#, 400, -32600),
            (r#"{"jsonrpc":"2.0","id":7}"#, 400, -32600),
            (r#"{"jsonrpc":"2.0","id":7,"method":1}"#, 400, -32600),
            (
                r#"{"jsonrpc":"2.0","id":7,"method":"missing"}"#,
                200,
                -32601,
            ),
            (
                r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":[]}"#,
                200,
                -32602,
            ),
            (
                r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"add_tasks","arguments":[]}}"#,
                200,
                -32602,
            ),
            (
                r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"set_setting"}}"#,
                200,
                -32602,
            ),
            (
                r#"{"jsonrpc":"2.0","id":7,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#,
                200,
                -32602,
            ),
        ] {
            let reply = mcp_wire(port, "POST", &mcp_headers(port), raw);
            assert_eq!(
                (reply.0, reply.2["error"]["code"].as_i64().unwrap()),
                (status, code),
                "{raw}: {}",
                reply.2
            );
        }
        for message in [
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"result":{}}),
            json!({"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"unsupported"}}),
            json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"add_tasks","arguments":{"tasks":[{"title":"notification must not write"}]}}}),
        ] {
            let reply = mcp_post(port, "codex", message);
            assert_eq!((reply.0, reply.2), (202, Value::Null));
        }
        let (_, _, failed) = mcp_post(
            port,
            "codex",
            mcp_call(8, "complete_task", json!({"id":9999})),
        );
        assert_eq!(failed["result"]["isError"], true);
        assert_eq!(mcp_payload(&failed)["error"]["code"], "not_found");
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);
        let (_, _, future) = mcp_post(port, "", mcp_init("app", "2099-01-01"));
        assert_eq!(future["result"]["protocolVersion"], "2025-11-25");
        for source in ["user", "APP", "", " User "] {
            let (_, _, added) = mcp_post(
                port,
                source,
                mcp_call(9, "add_tasks", json!({"tasks":[{"title":"source check"}]})),
            );
            assert_eq!(mcp_payload(&added)["tasks"][0]["source"], "mcp");
        }
        let mut legacy_headers = mcp_headers(port);
        legacy_headers.retain(|(name, _)| name != "MCP-Protocol-Version");
        let batch = json!([
            {"jsonrpc":"2.0","id":"old","method":"ping"},
            {"jsonrpc":"2.0","method":"notifications/initialized"}
        ]);
        let (_, _, reply) = mcp_wire(port, "POST", &legacy_headers, &batch.to_string());
        assert_eq!(reply, json!([{"jsonrpc":"2.0","id":"old","result":{}}]));
    }

    #[test]
    fn mcp_parallel_clients_keep_their_own_sources() {
        let (_dir, server, writes) = stored_server();
        let port = server.port;
        let barrier = Arc::new(std::sync::Barrier::new(12));
        let clients: Vec<_> = (0..12)
            .map(|index| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let source = format!("chat-{index}");
                    assert_eq!(
                        mcp_post(port, &source, mcp_init(&source, "2025-11-25")).0,
                        200
                    );
                    barrier.wait();
                    let (status, _, reply) = mcp_post(
                        port,
                        &source,
                        mcp_call(2, "add_tasks", json!({"tasks":[{"title":source}]})),
                    );
                    assert_eq!(status, 200);
                    let task = mcp_payload(&reply)["tasks"][0].clone();
                    assert_eq!(task["title"], source);
                    assert_eq!(task["source"], source);
                })
            })
            .collect();
        for client in clients {
            client.join().unwrap();
        }
        let (_, _, reply) = mcp_post(port, "reader", mcp_call(3, "list_tasks", json!({})));
        assert_eq!(mcp_payload(&reply)["tasks"].as_array().unwrap().len(), 12);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 12);
        assert_eq!(server.port, port);
    }

    #[test]
    fn mcp_token_rotation_invalidates_inflight_upload_without_rebinding() {
        let (_dir, server, writes) = stored_server();
        let port = server.port;
        let body = mcp_call(
            1,
            "add_tasks",
            json!({"tasks":[{"title":"revoked request"}]}),
        )
        .to_string();
        let mut stalled = connect(port);
        let mut head = format!("POST /mcp HTTP/1.1\r\nContent-Length: {}\r\nExpect: 100-continue\r\nConnection: close\r\n", body.len());
        for (name, value) in mcp_headers(port) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        stalled.write_all(head.as_bytes()).unwrap();
        // Receiving 100 Continue proves the old token passed the header gate and
        // the server is waiting for the body. No timing guesses or sleeps.
        let mut interim = [0u8; 25];
        stalled.read_exact(&mut interim).unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        server.set_token("replacement-token".into());
        stalled.write_all(body.as_bytes()).unwrap();
        let mut rejected = String::new();
        stalled.read_to_string(&mut rejected).unwrap();
        assert!(rejected.starts_with("HTTP/1.1 401"), "{rejected}");
        assert_eq!(
            mcp_post(port, "codex", mcp_init("Codex", "2025-11-25")).0,
            401
        );
        let mut headers = mcp_headers(port);
        headers
            .iter_mut()
            .find(|(name, _)| name == "Authorization")
            .unwrap()
            .1 = "Bearer replacement-token".into();
        let (status, _, reply) = mcp_wire(
            port,
            "POST",
            &headers,
            &mcp_call(2, "list_tasks", json!({})).to_string(),
        );
        assert_eq!(status, 200);
        assert!(mcp_payload(&reply)["tasks"].as_array().unwrap().is_empty());
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(server.port, port);
    }

    fn mcp_init(name: &str, version: &str) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":version,"capabilities":{},"clientInfo":{"name":name,"version":"1"}
        }})
    }

    fn mcp_call(id: i64, name: &str, arguments: Value) -> Value {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
    }

    fn mcp_payload(reply: &Value) -> Value {
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn mcp_independent_clients_share_one_listener_and_store() {
        let (_dir, server, writes) = stored_server();
        let port = server.port;
        let (status, head, init) = mcp_post(port, "codex", mcp_init("Codex", "2025-11-25"));
        assert_eq!(status, 200, "{init}");
        assert!(head
            .to_lowercase()
            .contains("content-type: application/json"));
        assert!(!head.to_lowercase().contains("mcp-session-id"));
        assert_eq!(init["result"]["protocolVersion"], "2025-11-25");
        let (_, _, init2) = mcp_post(port, "claude-code", mcp_init("Claude", "2025-06-18"));
        assert_eq!(init2["result"]["protocolVersion"], "2025-06-18");
        let initialized = mcp_post(
            port,
            "codex",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        );
        assert_eq!((initialized.0, initialized.2), (202, Value::Null));
        let (_, _, catalog) = mcp_post(
            port,
            "codex",
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        );
        let tools = catalog["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "add_tasks"));
        assert!(!tools.iter().any(|tool| tool["name"] == "set_setting"));
        for (source, title) in [("codex", "first chat"), ("claude-code", "second chat")] {
            let (_, _, added) = mcp_post(
                port,
                source,
                mcp_call(3, "add_tasks", json!({"tasks":[{"title":title}]})),
            );
            assert_eq!(added["result"]["isError"], false);
            assert_eq!(mcp_payload(&added)["tasks"][0]["source"], source);
        }
        let (_, _, listed) = mcp_post(port, "codex", mcp_call(4, "list_tasks", json!({})));
        assert_eq!(mcp_payload(&listed)["tasks"].as_array().unwrap().len(), 2);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(TcpListener::bind(("127.0.0.1", port)).is_err());
        let rest = request(port, "/v1/tasks", "test-token", "");
        let rest: Value = serde_json::from_str(rest.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(rest["tasks"].as_array().unwrap().len(), 2);
        server.stop();
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    struct SocketBackend;
    impl Backend for SocketBackend {
        fn call(&self, _: &str, _: Value, _: &str) -> Result<Value, ApiError> {
            Ok(json!({}))
        }
    }

    fn socket_server() -> Server {
        let reserve = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = reserve.local_addr().unwrap().port();
        drop(reserve);
        Server::start(
            port,
            "test-token".into(),
            "test".into(),
            Box::new(SocketBackend),
            Box::new(|| {}),
        )
        .unwrap()
    }

    fn connect(port: u16) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
    }

    #[test]
    fn partial_request_body_cannot_block_other_requests() {
        let server = socket_server();
        let port = server.port;
        let mut stalled = connect(port);
        write!(stalled, "POST /v1/call/list_tasks HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer test-token\r\nContent-Length: 1025\r\n\r\n").unwrap();
        // Establish ordering without sleeping: the server accepts this complete header
        // before the subsequent independent connection.
        let mut health = connect(port);
        write!(
            health,
            "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut reply = [0u8; 512];
        let result = health.read(&mut reply);
        drop(stalled);
        drop(health);
        server.stop();
        assert!(
            result.as_ref().is_ok_and(|n| *n > 0),
            "health must respond while another client stalls: {result:?}"
        );
        assert!(String::from_utf8_lossy(&reply).contains("200 OK"));
    }

    fn request(port: u16, path: &str, token: &str, extra: &str) -> String {
        let mut stream = connect(port);
        write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n{extra}\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    }

    #[test]
    fn enormous_declared_body_is_denied_without_reading_or_allocating_it() {
        let server = socket_server();
        let reply = request(
            server.port,
            "/v1/tasks",
            "wrong-token",
            "Content-Length: 1099511627776\r\n",
        );
        assert!(reply.starts_with("HTTP/1.1 401"));
        let health = request(
            server.port,
            "/v1/health",
            "",
            "Content-Length: 1099511627776\r\n",
        );
        assert!(health.starts_with("HTTP/1.1 200"));
    }

    #[test]
    fn stop_cancels_incomplete_headers_and_bodies() {
        let server = socket_server();
        let mut header = connect(server.port);
        header
            .write_all(b"POST /v1/call/list_tasks HTTP/1.1\r\n")
            .unwrap();
        let mut body = connect(server.port);
        write!(body, "POST /v1/call/list_tasks HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer test-token\r\nContent-Length: 1025\r\n\r\n", server.port).unwrap();
        let now = std::time::Instant::now();
        server.stop();
        assert!(
            now.elapsed() < Duration::from_secs(1),
            "network clients cannot delay settings changes"
        );
    }

    #[test]
    fn live_token_rotation_invalidates_old_token_without_rebinding() {
        let server = socket_server();
        let port = server.port;
        assert!(request(port, "/v1/tasks", "test-token", "").starts_with("HTTP/1.1 200"));
        server.set_token("replacement-test-token".into());
        assert!(request(port, "/v1/tasks", "test-token", "").starts_with("HTTP/1.1 401"));
        assert!(
            request(port, "/v1/tasks", "replacement-test-token", "").starts_with("HTTP/1.1 200")
        );
        assert_eq!(server.port, port);
    }
    use std::cell::RefCell;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<(String, Value, String)>>,
        fail: Option<ApiError>,
    }

    impl Backend for Fake {
        fn call(&self, tool: &str, args: Value, source: &str) -> Result<Value, ApiError> {
            self.calls
                .borrow_mut()
                .push((tool.into(), args.clone(), source.into()));
            match &self.fail {
                Some(e) => Err(e.clone()),
                None => Ok(json!({"tool": tool, "args": args})),
            }
        }
    }

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn auth() -> String {
        format!("Bearer {TOKEN}")
    }

    fn go(fake: &Fake, method: &str, url: &str, headers: &[(String, String)], body: &str) -> Reply {
        let req = Req {
            method,
            url,
            headers,
            body: body.as_bytes(),
        };
        let g = Guard {
            port: 47821,
            token: TOKEN,
            version: "0.1.0",
        };
        handle(&req, &g, fake)
    }

    fn code(r: &Reply) -> &str {
        r.body["error"]["code"].as_str().unwrap_or("")
    }

    #[test]
    fn origin_is_refused_even_with_token() {
        let f = Fake::default();
        let hs = h(&[
            ("Host", "127.0.0.1:47821"),
            ("Authorization", &auth()),
            ("Origin", "http://evil.example"),
        ]);
        let r = go(&f, "GET", "/v1/tasks", &hs, "");
        assert_eq!(r.status, 403);
        assert_eq!(code(&r), "forbidden");
        let hs = h(&[("Host", "127.0.0.1:47821"), ("origin", "null")]);
        assert_eq!(go(&f, "GET", "/v1/health", &hs, "").status, 403);
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn wrong_or_missing_host_is_refused() {
        let f = Fake::default();
        for host in [
            "evil.example:47821",
            "127.0.0.1:1234",
            "127.0.0.1",
            "localhost",
        ] {
            let hs = h(&[("Host", host), ("Authorization", &auth())]);
            assert_eq!(go(&f, "GET", "/v1/tasks", &hs, "").status, 403, "{host}");
        }
        let hs = h(&[("Authorization", &auth())]);
        assert_eq!(go(&f, "GET", "/v1/tasks", &hs, "").status, 403);
        for host in ["127.0.0.1:47821", "localhost:47821", "LocalHost:47821"] {
            let hs = h(&[("Host", host), ("Authorization", &auth())]);
            assert_eq!(go(&f, "GET", "/v1/tasks", &hs, "").status, 200, "{host}");
        }
    }

    #[test]
    fn missing_or_wrong_token_is_401() {
        let f = Fake::default();
        let hs = h(&[("Host", "127.0.0.1:47821")]);
        let r = go(&f, "GET", "/v1/tasks", &hs, "");
        assert_eq!((r.status, code(&r)), (401, "unauthorized"));
        let bad = format!("Bearer {}", TOKEN.replace('0', "1"));
        for v in [bad.as_str(), "Bearer ", "Basic abc", TOKEN, "Bearer short"] {
            let hs = h(&[("Host", "127.0.0.1:47821"), ("Authorization", v)]);
            assert_eq!(
                go(&f, "POST", "/v1/call/add_tasks", &hs, "{}").status,
                401,
                "{v}"
            );
        }
        // unknown paths are not revealed without the token
        let hs = h(&[("Host", "127.0.0.1:47821")]);
        assert_eq!(go(&f, "GET", "/nope", &hs, "").status, 401);
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn health_needs_no_token() {
        let f = Fake::default();
        let hs = h(&[("Host", "localhost:47821")]);
        let r = go(&f, "GET", "/v1/health", &hs, "");
        assert_eq!(r.status, 200);
        assert_eq!(r.body, json!({"ok": true, "version": "0.1.0"}));
        assert_eq!(go(&f, "POST", "/v1/health", &hs, "").status, 405);
    }

    #[test]
    fn tools_lists_public_tools_only() {
        let f = Fake::default();
        let hs = h(&[("Host", "127.0.0.1:47821"), ("Authorization", &auth())]);
        let r = go(&f, "GET", "/v1/tools", &hs, "");
        assert_eq!(r.status, 200);
        let names: Vec<&str> = r.body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"add_tasks"));
        assert!(!names.contains(&"set_setting") && !names.contains(&"mark_reminded"));
        assert!(r.body["tools"][0]["inputSchema"].is_object());
    }

    #[test]
    fn reserved_source_names_cannot_be_claimed() {
        for claimed in ["user", " User ", "app"] {
            let f = Fake::default();
            let hs = h(&[
                ("Host", "127.0.0.1:47821"),
                ("Authorization", &auth()),
                ("X-DayNote-Source", claimed),
            ]);
            let r = go(
                &f,
                "POST",
                "/v1/call/add_tasks",
                &hs,
                r#"{"tasks":[{"title":"x"}]}"#,
            );
            assert_eq!(r.status, 200);
            assert_eq!(f.calls.borrow()[0].2, "http");
        }
    }

    #[test]
    fn gate_decides_from_the_head_alone() {
        let g = Guard {
            port: 47821,
            token: TOKEN,
            version: "0.1.0",
        };
        let head = |url: &'static str, hs: &[(String, String)]| {
            gate(
                &Req {
                    method: "POST",
                    url,
                    headers: hs,
                    body: &[],
                },
                &g,
            )
            .map(|r| r.status)
        };
        let host = ("Host", "127.0.0.1:47821");
        assert_eq!(head("/v1/call/add_tasks", &h(&[host])), Some(401));
        assert_eq!(
            head(
                "/v1/call/add_tasks",
                &h(&[host, ("Origin", "http://evil.example")])
            ),
            Some(403)
        );
        assert_eq!(
            head(
                "/v1/call/add_tasks",
                &h(&[("Host", "evil.example:47821"), ("Authorization", &auth())])
            ),
            Some(403)
        );
        assert_eq!(
            head(
                "/v1/call/add_tasks",
                &h(&[host, ("Authorization", &auth())])
            ),
            None
        );
        assert_eq!(head("/v1/health?x=1", &h(&[host])), None);
    }

    #[test]
    fn call_runs_public_tools_and_reports_writes() {
        let f = Fake::default();
        let hs = h(&[
            ("Host", "127.0.0.1:47821"),
            ("Authorization", &auth()),
            ("X-DayNote-Source", "  Claude-Code "),
        ]);
        let r = go(
            &f,
            "POST",
            "/v1/call/add_tasks",
            &hs,
            r#"{"tasks":[{"title":"写周报"}]}"#,
        );
        assert_eq!(r.status, 200);
        assert!(r.wrote);
        let calls = f.calls.borrow();
        assert_eq!(calls[0].0, "add_tasks");
        assert_eq!(calls[0].1["tasks"][0]["title"], "写周报");
        assert_eq!(calls[0].2, "claude-code");
    }

    #[test]
    fn empty_body_is_empty_object_and_reads_do_not_write() {
        let f = Fake::default();
        let hs = h(&[("Host", "127.0.0.1:47821"), ("Authorization", &auth())]);
        let r = go(&f, "POST", "/v1/call/list_tasks", &hs, "");
        assert_eq!(r.status, 200);
        assert!(!r.wrote);
        assert_eq!(f.calls.borrow()[0].1, json!({}));
        assert_eq!(f.calls.borrow()[0].2, "http");
        let r = go(&f, "POST", "/v1/call/add_tasks", &hs, "{not json");
        assert_eq!((r.status, code(&r)), (400, "invalid_argument"));
    }

    #[test]
    fn unknown_and_internal_tools_are_refused() {
        let f = Fake::default();
        let hs = h(&[("Host", "127.0.0.1:47821"), ("Authorization", &auth())]);
        for tool in [
            "nope",
            "set_setting",
            "get_settings",
            "mark_reminded",
            "due_reminders",
            "snooze_task",
        ] {
            let r = go(&f, "POST", &format!("/v1/call/{tool}"), &hs, "{}");
            assert_eq!((r.status, code(&r)), (404, "not_found"), "{tool}");
        }
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn routes_methods_and_unknown_paths() {
        let f = Fake::default();
        let hs = h(&[("Host", "127.0.0.1:47821"), ("Authorization", &auth())]);
        assert_eq!(go(&f, "GET", "/v1/call/add_tasks", &hs, "").status, 405);
        assert_eq!(go(&f, "POST", "/v1/tasks", &hs, "").status, 405);
        assert_eq!(go(&f, "DELETE", "/v1/tools", &hs, "").status, 405);
        let r = go(&f, "GET", "/v2/whatever", &hs, "");
        assert_eq!((r.status, code(&r)), (404, "not_found"));
        let r = go(
            &f,
            "GET",
            "/v1/tasks?date=2026-11-11&status=todo&bogus=1&from=",
            &hs,
            "",
        );
        assert_eq!(r.status, 200);
        let r = go(
            &f,
            "GET",
            "/v1/summary?from=2026-11-01&to=2026-11-30&status=x",
            &hs,
            "",
        );
        assert_eq!(r.status, 200);
        let calls = f.calls.borrow();
        assert_eq!(calls[0].0, "list_tasks");
        assert_eq!(calls[0].1, json!({"date": "2026-11-11", "status": "todo"}));
        assert_eq!(calls[1].0, "get_summary");
        assert_eq!(
            calls[1].1,
            json!({"from": "2026-11-01", "to": "2026-11-30"})
        );
    }

    #[test]
    fn core_errors_map_to_status_codes() {
        assert_eq!(status_of("invalid_argument"), 400);
        assert_eq!(status_of("not_found"), 404);
        assert_eq!(status_of("internal"), 500);
        assert_eq!(status_of("whatever"), 500);
        let hs = h(&[("Host", "127.0.0.1:47821"), ("Authorization", &auth())]);
        for (e, status) in [
            (ApiError::invalid("坏参数"), 400),
            (ApiError::not_found("找不到任务 9"), 404),
            (ApiError::internal("炸了"), 500),
        ] {
            let f = Fake {
                fail: Some(e.clone()),
                ..Fake::default()
            };
            let r = go(&f, "POST", "/v1/call/complete_task", &hs, r#"{"id":9}"#);
            assert_eq!(r.status, status);
            assert_eq!(
                r.body,
                json!({"error": {"code": e.code, "message": e.message}})
            );
            assert!(!r.wrote);
        }
    }

    #[test]
    fn query_values_are_percent_decoded() {
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("%E4%BB%8A"), "今");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }

    #[test]
    fn token_file_is_created_once() {
        let dir = std::env::temp_dir().join(format!("daynote-token-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = load_or_create_token(&dir).unwrap();
        assert!(is_token(&a));
        let b = load_or_create_token(&dir).unwrap();
        assert_eq!(a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
