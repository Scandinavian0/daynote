//! The single dispatch entry point shared by every front end.
use crate::{notes, recurring, tasks, Store};
use chrono::NaiveDateTime;
use rusqlite::{Connection, TransactionBehavior};
use serde_json::{Map, Value};

pub struct Ctx {
    pub source: String,
    pub now: NaiveDateTime,
}

impl Ctx {
    pub(crate) fn today(&self) -> chrono::NaiveDate {
        self.now.date()
    }
}

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub public: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    /// `not_found` | `invalid_argument` | `internal`
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn not_found(message: impl Into<String>) -> Self {
        ApiError {
            code: "not_found",
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        ApiError {
            code: "invalid_argument",
            message: message.into(),
        }
    }
    pub fn internal(message: impl Into<String>) -> Self {
        ApiError {
            code: "internal",
            message: message.into(),
        }
    }
    pub fn to_json(&self) -> Value {
        serde_json::json!({"error": {"code": self.code, "message": self.message}})
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::internal(format!("数据库错误：{e}"))
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;
pub(crate) type Obj = Map<String, Value>;

/// Typed accessors over the argument object; `null` counts as absent.
pub(crate) struct Args<'a>(pub &'a Obj);

impl<'a> Args<'a> {
    pub fn get(&self, k: &str) -> Option<&'a Value> {
        self.0.get(k).filter(|v| !v.is_null())
    }
    /// Key present at all (including explicit `null`).
    pub fn has(&self, k: &str) -> bool {
        self.0.contains_key(k)
    }
    pub fn opt_str(&self, k: &str) -> ApiResult<Option<&'a str>> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(ApiError::invalid(format!("参数 {k} 应为字符串"))),
        }
    }
    pub fn str(&self, k: &str) -> ApiResult<&'a str> {
        self.opt_str(k)?
            .ok_or_else(|| ApiError::invalid(format!("缺少参数 {k}")))
    }
    pub fn opt_i64(&self, k: &str) -> ApiResult<Option<i64>> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => as_i64(v)
                .map(Some)
                .ok_or_else(|| ApiError::invalid(format!("参数 {k} 应为整数"))),
        }
    }
    pub fn i64(&self, k: &str) -> ApiResult<i64> {
        self.opt_i64(k)?
            .ok_or_else(|| ApiError::invalid(format!("缺少参数 {k}")))
    }
    pub fn opt_bool(&self, k: &str) -> ApiResult<Option<bool>> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(ApiError::invalid(format!("参数 {k} 应为 true/false"))),
        }
    }
    pub fn array(&self, k: &str) -> ApiResult<&'a Vec<Value>> {
        match self.get(k) {
            Some(Value::Array(a)) => Ok(a),
            None => Err(ApiError::invalid(format!("缺少参数 {k}"))),
            Some(_) => Err(ApiError::invalid(format!("参数 {k} 应为数组"))),
        }
    }
    pub fn ids(&self, k: &str) -> ApiResult<Vec<i64>> {
        self.array(k)?
            .iter()
            .map(|v| as_i64(v).ok_or_else(|| ApiError::invalid(format!("{k} 里应全是整数 id"))))
            .collect()
    }
}

/// Integers, also accepted as numeric strings (AI clients sometimes quote ids).
pub(crate) fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// All tools; only `public == true` ones are exposed over MCP.
pub fn tool_specs() -> Vec<ToolSpec> {
    crate::specs::all()
}

/// Run one tool. Every success value is an object carrying `today` and `now`.
pub fn dispatch(store: &mut Store, tool: &str, args: Value, ctx: &Ctx) -> Result<Value, ApiError> {
    let args = match args {
        Value::Null => Map::new(),
        Value::Object(m) => m,
        _ => return Err(ApiError::invalid("参数必须是 JSON 对象")),
    };
    let a = Args(&args);
    let tx = store
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut out = run(&tx, tool, &a, ctx)?;
    tx.commit()?;
    out.insert("today".into(), Value::String(crate::time::fmt_date(ctx.today())));
    out.insert("now".into(), Value::String(crate::time::fmt_dt(ctx.now)));
    Ok(Value::Object(out))
}

fn run(c: &Connection, tool: &str, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    match tool {
        "list_tasks" => tasks::list_tasks(c, a, ctx),
        "add_tasks" => tasks::add_tasks(c, a, ctx),
        "add_subtasks" => tasks::add_subtasks(c, a, ctx),
        "complete_task" => tasks::complete_task(c, a, ctx),
        "update_task" => tasks::update_task(c, a, ctx),
        "move_tasks" => tasks::move_tasks(c, a, ctx),
        "mark_reminded" => tasks::mark_reminded(c, a, ctx),
        "snooze_task" => tasks::snooze_task(c, a, ctx),
        "due_reminders" => tasks::due_reminders(c, a, ctx),
        "delete_task" => tasks::delete_task(c, a, ctx),
        "restore_task" => tasks::restore_task(c, a, ctx),
        "get_summary" => tasks::get_summary(c, a, ctx),
        "reorder_tasks" => tasks::reorder_tasks(c, a, ctx),
        "add_recurring" => recurring::add_recurring(c, a, ctx),
        "list_recurring" => recurring::list_recurring(c, a, ctx),
        "update_recurring" => recurring::update_recurring(c, a, ctx),
        "delete_recurring" => recurring::delete_recurring(c, a, ctx),
        "get_day_note" => notes::get_day_note(c, a, ctx),
        "set_day_note" => notes::set_day_note(c, a, ctx),
        "get_settings" => notes::get_settings(c, a, ctx),
        "set_setting" => notes::set_setting(c, a, ctx),
        _ => Err(ApiError::not_found(format!("没有这个工具：{tool}"))),
    }
}
