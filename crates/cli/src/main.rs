//! daynote-cli: command line and MCP (stdio) front end over `daynote_core::api::dispatch`.
mod mcp;

use daynote_core::api::{self, ApiError, Ctx};
use daynote_core::{now_local, Store};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};

const USAGE: &str = "\
daynote-cli [--source 名字] <命令> ...

  call <tool> [<json参数> | @文件 | -]   直通调用任意工具（- 表示从 stdin 读 JSON）
  list [--date D] [--from D --to D] [--status all|todo|done|overdue]
  add \"标题\" [--date D] [--remind T] [--note N] [--important]
  done <id> | undone <id> | delete <id> | restore <id>
  summary [--date D | --from D --to D]
  tools                                   打印公开工具的名字/描述/schema
  mcp                                     以 stdio 运行 MCP 服务

日期 YYYY-MM-DD，省略即今天；时间 \"YYYY-MM-DD HH:MM\" 或 \"HH:MM\"。
数据目录：DAYNOTE_DATA_DIR 优先，其次为设置页保存的目录；旧版 data 数据库/portable.txt 使用程序旁的 data；新安装使用 %LOCALAPPDATA%\\DayNoteData。
--source 默认 cli（mcp 模式下默认取客户端名）。输出为 UTF-8 JSON。";

fn main() {
    console::use_utf8();
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(args));
}

fn run(args: Vec<String>) -> i32 {
    let (source, rest) = match take_source(args) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let Some((cmd, rest)) = rest.split_first() else {
        eprintln!("{USAGE}");
        return fail(&ApiError::invalid("缺少命令，见 daynote-cli help"));
    };
    match cmd.as_str() {
        "help" | "--help" | "-h" => {
            write_stdout(&format!("{USAGE}\n"));
            0
        }
        "mcp" => mcp::serve(source),
        "tools" => {
            let tools: Vec<Value> = api::tool_specs()
                .into_iter()
                .filter(|t| t.public)
                .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.input_schema}))
                .collect();
            print_json(&json!({ "tools": tools }));
            0
        }
        _ => match build_call(cmd, rest) {
            Ok((tool, args)) => call(&tool, args, source.as_deref().unwrap_or("cli")),
            Err(e) => fail(&e),
        },
    }
}

/// Pull `--source X` / `--source=X` out of the argument list (anywhere before `--`).
fn take_source(args: Vec<String>) -> Result<(Option<String>, Vec<String>), ApiError> {
    let mut source = None;
    let mut rest = Vec::new();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--" {
            rest.push(a);
            rest.extend(it.by_ref());
            break;
        }
        if a == "--source" {
            source = Some(it.next().ok_or_else(|| ApiError::invalid("--source 后面需要名字"))?);
        } else if let Some(v) = a.strip_prefix("--source=") {
            source = Some(v.to_string());
        } else {
            rest.push(a);
        }
    }
    if let Some(s) = &source {
        if s.trim().is_empty() {
            return Err(ApiError::invalid("--source 不能为空"));
        }
    }
    Ok((source, rest))
}

/// Parsed `--flag value` options plus positional arguments.
struct Opts {
    flags: Map<String, Value>,
    positional: Vec<String>,
}

fn parse_opts(args: &[String], with_value: &[&str], switches: &[&str]) -> Result<Opts, ApiError> {
    let mut o = Opts {
        flags: Map::new(),
        positional: Vec::new(),
    };
    let mut i = 0;
    let mut only_positional = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if only_positional || !a.starts_with("--") {
            o.positional.push(a.clone());
            continue;
        }
        if a == "--" {
            only_positional = true;
            continue;
        }
        let (name, inline) = match a[2..].split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (a[2..].to_string(), None),
        };
        if with_value.contains(&name.as_str()) {
            let v = match inline {
                Some(v) => v,
                None => {
                    let v = args
                        .get(i)
                        .cloned()
                        .ok_or_else(|| ApiError::invalid(format!("--{name} 后面需要值")))?;
                    i += 1;
                    v
                }
            };
            o.flags.insert(name, Value::String(v));
        } else if switches.contains(&name.as_str()) && inline.is_none() {
            o.flags.insert(name, Value::Bool(true));
        } else {
            return Err(ApiError::invalid(format!("不认识的选项 --{name}")));
        }
    }
    Ok(o)
}

fn one_id(cmd: &str, o: &Opts) -> Result<i64, ApiError> {
    match o.positional.as_slice() {
        [id] => id
            .parse()
            .map_err(|_| ApiError::invalid(format!("{cmd} 需要整数 id：{id}"))),
        _ => Err(ApiError::invalid(format!("用法：daynote-cli {cmd} <id>"))),
    }
}

fn no_positional(cmd: &str, o: &Opts) -> Result<(), ApiError> {
    match o.positional.first() {
        None => Ok(()),
        Some(p) => Err(ApiError::invalid(format!("{cmd} 不接受参数 {p}"))),
    }
}

/// Map a CLI command to `(tool, args)`.
fn build_call(cmd: &str, rest: &[String]) -> Result<(String, Value), ApiError> {
    match cmd {
        "call" => {
            let (tool, input) = match rest {
                [tool] => (tool, None),
                [tool, input] => (tool, Some(input.as_str())),
                _ => return Err(ApiError::invalid("用法：daynote-cli call <tool> [<json> | @文件 | -]")),
            };
            Ok((tool.clone(), read_json_arg(input)?))
        }
        "list" => {
            let o = parse_opts(rest, &["date", "from", "to", "status"], &[])?;
            no_positional(cmd, &o)?;
            Ok(("list_tasks".into(), Value::Object(o.flags)))
        }
        "summary" => {
            let o = parse_opts(rest, &["date", "from", "to"], &[])?;
            no_positional(cmd, &o)?;
            Ok(("get_summary".into(), Value::Object(o.flags)))
        }
        "add" => {
            let o = parse_opts(rest, &["date", "remind", "note"], &["important"])?;
            let title = match o.positional.as_slice() {
                [t] => t.clone(),
                _ => return Err(ApiError::invalid("用法：daynote-cli add \"标题\" [--date D] [--remind T] [--note N] [--important]")),
            };
            let mut task = Map::new();
            task.insert("title".into(), json!(title));
            if let Some(n) = o.flags.get("note") {
                task.insert("note".into(), n.clone());
            }
            if let Some(r) = o.flags.get("remind") {
                task.insert("remind_at".into(), r.clone());
            }
            if o.flags.contains_key("important") {
                task.insert("priority".into(), json!(1));
            }
            let mut args = Map::new();
            if let Some(d) = o.flags.get("date") {
                args.insert("date".into(), d.clone());
            }
            args.insert("tasks".into(), json!([task]));
            Ok(("add_tasks".into(), Value::Object(args)))
        }
        "done" | "undone" => {
            let id = one_id(cmd, &parse_opts(rest, &[], &[])?)?;
            Ok(("complete_task".into(), json!({"id": id, "done": cmd == "done"})))
        }
        "delete" | "restore" => {
            let id = one_id(cmd, &parse_opts(rest, &[], &[])?)?;
            let tool = if cmd == "delete" { "delete_task" } else { "restore_task" };
            Ok((tool.into(), json!({ "id": id })))
        }
        _ => Err(ApiError::invalid(format!("不认识的命令 {cmd}，见 daynote-cli help"))),
    }
}

/// `<json>` literal, `@path` file, or `-` for stdin. Missing → `{}`.
fn read_json_arg(input: Option<&str>) -> Result<Value, ApiError> {
    let text = match input {
        None => return Ok(json!({})),
        Some("-") => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| ApiError::invalid(format!("读取 stdin 失败：{e}")))?;
            utf8(buf)?
        }
        Some(s) if s.starts_with('@') => {
            let buf = std::fs::read(&s[1..])
                .map_err(|e| ApiError::invalid(format!("读取文件 {} 失败：{e}", &s[1..])))?;
            utf8(buf)?
        }
        Some(s) => s.to_string(),
    };
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text).map_err(|e| ApiError::invalid(format!("参数不是合法 JSON：{e}")))
}

fn utf8(buf: Vec<u8>) -> Result<String, ApiError> {
    String::from_utf8(buf).map_err(|_| ApiError::invalid("输入不是 UTF-8 编码"))
}

fn call(tool: &str, args: Value, source: &str) -> i32 {
    let mut store = match Store::open_default() {
        Ok(s) => s,
        Err(e) => return fail(&ApiError::internal(format!("打开数据库失败：{e}"))),
    };
    let ctx = Ctx {
        source: source.to_string(),
        now: now_local(),
    };
    match api::dispatch(&mut store, tool, args, &ctx) {
        Ok(v) => {
            print_json(&v);
            0
        }
        Err(e) => fail(&e),
    }
}

fn fail(e: &ApiError) -> i32 {
    print_json(&e.to_json());
    1
}

fn print_json(v: &Value) {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".into());
    write_stdout(&format!("{text}\n"));
}

/// Raw UTF-8 bytes to stdout.
pub(crate) fn write_stdout(s: &str) {
    let out = std::io::stdout();
    let mut lock = out.lock();
    let _ = lock.write_all(s.as_bytes());
    let _ = lock.flush();
}

mod console {
    /// Make the Windows console interpret our UTF-8 output (and input) correctly.
    #[cfg(windows)]
    pub fn use_utf8() {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetConsoleOutputCP(code_page: u32) -> i32;
            fn SetConsoleCP(code_page: u32) -> i32;
        }
        const CP_UTF8: u32 = 65001;
        // SAFETY: plain Win32 calls with a constant argument; failure (no console) is harmless.
        unsafe {
            SetConsoleOutputCP(CP_UTF8);
            SetConsoleCP(CP_UTF8);
        }
    }

    #[cfg(not(windows))]
    pub fn use_utf8() {}
}
