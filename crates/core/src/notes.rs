//! Per-day free-text notes and app settings.
use crate::api::{ApiError, ApiResult, Args, Ctx, Obj};
use crate::tasks::date_arg;
use crate::time::{fmt_date, fmt_dt};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

fn note_result(c: &Connection, date: &str) -> ApiResult<Obj> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT content, updated_at FROM day_notes WHERE date = ?1",
            [date],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (content, updated_at) = match row {
        Some((c, u)) => (c, Some(u)),
        None => (String::new(), None),
    };
    let mut out = Obj::new();
    out.insert("date".into(), json!(date));
    out.insert("content".into(), json!(content));
    out.insert("updated_at".into(), json!(updated_at));
    Ok(out)
}

pub(crate) fn get_day_note(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    note_result(c, &fmt_date(date_arg(a, "date", ctx)?))
}

pub(crate) fn set_day_note(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let date = fmt_date(date_arg(a, "date", ctx)?);
    if !a.has("content") {
        return Err(ApiError::invalid("缺少参数 content"));
    }
    let content = a.opt_str("content")?.unwrap_or("");
    c.execute(
        "INSERT INTO day_notes (date, content, source, updated_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(date) DO UPDATE SET content=excluded.content, source=excluded.source, \
         updated_at=excluded.updated_at",
        params![date, content, ctx.source, fmt_dt(ctx.now)],
    )?;
    note_result(c, &date)
}

fn all_settings(c: &Connection) -> ApiResult<Obj> {
    let mut st = c.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = Obj::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, Value::String(v));
    }
    let mut out = Obj::new();
    out.insert("settings".into(), Value::Object(map));
    Ok(out)
}

pub(crate) fn get_settings(c: &Connection, _a: &Args, _ctx: &Ctx) -> ApiResult<Obj> {
    all_settings(c)
}

pub(crate) fn set_setting(c: &Connection, a: &Args, _ctx: &Ctx) -> ApiResult<Obj> {
    let key = a.str("key")?.trim();
    if key.is_empty() {
        return Err(ApiError::invalid("key 不能为空"));
    }
    let value = match a.get("value") {
        None => return Err(ApiError::invalid("缺少参数 value")),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    };
    c.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    all_settings(c)
}
