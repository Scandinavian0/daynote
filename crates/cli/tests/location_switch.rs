//! Changing the data directory, seen from real `daynote-cli` processes. LOCALAPPDATA
//! points at a temporary folder, so the user's own `location.json` is never touched.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Sandbox {
    root: PathBuf,
    exe: PathBuf,
    local: PathBuf,
}

fn sandbox(label: &str) -> Sandbox {
    let root = std::env::temp_dir().join(format!("daynote-switch-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let program = root.join("program");
    std::fs::create_dir_all(&program).unwrap();
    // A copy outside target/, so no `data` folder beside the binary can interfere.
    let exe = program.join("daynote-cli.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_daynote-cli"), &exe).unwrap();
    Sandbox { local: root.join("local"), exe, root }
}

impl Sandbox {
    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config(&self) -> PathBuf {
        self.local.join("DayNoteData/location.json")
    }

    fn write_config(&self, value: Value) {
        std::fs::create_dir_all(self.config().parent().unwrap()).unwrap();
        std::fs::write(self.config(), serde_json::to_vec(&value).unwrap()).unwrap();
    }

    /// The CLI as the user runs it: no DAYNOTE_DATA_DIR, so location.json decides.
    fn cli(&self) -> Command {
        let mut c = Command::new(&self.exe);
        c.env_remove("DAYNOTE_DATA_DIR").env("LOCALAPPDATA", &self.local);
        c
    }

    fn add_directly(&self, dir: &Path, title: &str) {
        let out = Command::new(&self.exe).args(["add", title])
            .env("DAYNOTE_DATA_DIR", dir).env("LOCALAPPDATA", &self.local).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_pending_choice_leaves_the_cli_on_the_current_directory() {
    let sb = sandbox("cli");
    let a = sb.dir("a");
    let b = sb.dir("b");
    sb.add_directly(&a, "already in A");
    sb.write_config(json!({"current": a, "pending": b}));

    let out = sb.cli().args(["add", "written while pending"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = sb.cli().arg("list").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let listed = String::from_utf8_lossy(&out.stdout);
    assert!(listed.contains("already in A") && listed.contains("written while pending"), "{listed}");
    assert!(!b.join("daynote.db").exists(), "nothing may be written to the pending directory");
}

struct Mcp {
    child: std::process::Child,
    out: BufReader<std::process::ChildStdout>,
    next_id: i64,
}

impl Mcp {
    fn start(sb: &Sandbox) -> Mcp {
        let mut child = sb.cli().arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped())
            .stderr(Stdio::null()).spawn().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut mcp = Mcp { child, out, next_id: 1 };
        mcp.request("initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}));
        mcp
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
        let mut reply = String::new();
        assert!(self.out.read_line(&mut reply).unwrap() > 0, "the MCP server exited");
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["id"], id);
        reply
    }

    /// `(isError, text)` of a `list_tasks` call.
    fn list(&mut self) -> (bool, String) {
        let reply = self.request("tools/call", json!({"name": "list_tasks", "arguments": {}}));
        let result = &reply["result"];
        (result["isError"].as_bool().unwrap(), result["content"][0]["text"].as_str().unwrap().to_string())
    }

    fn finish(mut self) {
        drop(self.child.stdin.take());
        assert!(self.child.wait().unwrap().success());
    }
}

#[test]
fn a_running_mcp_server_follows_a_promotion() {
    let sb = sandbox("mcp");
    let a = sb.dir("a");
    let b = sb.dir("b");
    sb.add_directly(&a, "only in A");
    sb.add_directly(&b, "only in B");
    sb.write_config(json!({"current": a, "pending": b}));

    let mut mcp = Mcp::start(&sb);
    let (failed, text) = mcp.list();
    assert!(!failed && text.contains("only in A") && !text.contains("only in B"), "{text}");

    // What the desktop app does at its next start.
    let promoted = daynote_core::paths::promote_pending_at(&sb.config()).unwrap();
    assert!(matches!(promoted, daynote_core::paths::Promotion::Promoted(_)), "{promoted:?}");
    let (failed, text) = mcp.list();
    assert!(!failed && text.contains("only in B") && !text.contains("only in A"), "{text}");

    // A configuration that cannot be read fails that one call; the server stays up.
    let good = std::fs::read(sb.config()).unwrap();
    std::fs::write(sb.config(), "broken json").unwrap();
    let (failed, text) = mcp.list();
    assert!(failed, "{text}");
    assert!(text.contains("location.json"), "the error names the file: {text}");
    std::fs::write(sb.config(), good).unwrap();
    let (failed, text) = mcp.list();
    assert!(!failed && text.contains("only in B"), "{text}");
    mcp.finish();
}
