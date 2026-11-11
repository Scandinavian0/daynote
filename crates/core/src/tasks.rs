//! Task rows, their JSON shape, and the task tools.
use crate::api::{ApiError, ApiResult, Args, Ctx, Obj};
use crate::time::{fmt_date, fmt_dt, parse_date, parse_datetime};
use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};

pub(crate) const COLS: &str = "id, date, title, note, status, completed_at, completed_by, remind_at, \
     snooze_until, reminded_at, priority, sort_order, parent_id, series_id, source, created_at, \
     updated_at, deleted_at";

#[derive(Debug, Clone)]
pub(crate) struct Task {
    pub id: i64,
    pub date: String,
    pub title: String,
    pub note: Option<String>,
    pub status: String,
    pub completed_at: Option<String>,
    pub completed_by: Option<String>,
    pub remind_at: Option<String>,
    pub snooze_until: Option<String>,
    pub reminded_at: Option<String>,
    pub priority: i64,
    pub sort_order: i64,
    pub parent_id: Option<i64>,
    pub series_id: Option<i64>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
}

impl Task {
    fn from_row(r: &Row) -> rusqlite::Result<Task> {
        Ok(Task {
            id: r.get(0)?,
            date: r.get(1)?,
            title: r.get(2)?,
            note: r.get(3)?,
            status: r.get(4)?,
            completed_at: r.get(5)?,
            completed_by: r.get(6)?,
            remind_at: r.get(7)?,
            snooze_until: r.get(8)?,
            reminded_at: r.get(9)?,
            priority: r.get(10)?,
            sort_order: r.get(11)?,
            parent_id: r.get(12)?,
            series_id: r.get(13)?,
            source: r.get(14)?,
            created_at: r.get(15)?,
            updated_at: r.get(16)?,
            deleted_at: r.get(17)?,
        })
    }

    pub fn is_done(&self) -> bool {
        self.status == "done"
    }

    fn base_json(&self) -> Obj {
        let v = json!({
            "id": self.id,
            "date": self.date,
            "title": self.title,
            "note": self.note,
            "status": self.status,
            "completed_at": self.completed_at,
            "completed_by": self.completed_by,
            "remind_at": self.remind_at,
            "snooze_until": self.snooze_until,
            "reminded_at": self.reminded_at,
            "priority": self.priority,
            "sort_order": self.sort_order,
            "parent_id": self.parent_id,
            "series_id": self.series_id,
            "source": self.source,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "deleted_at": self.deleted_at,
        });
        match v {
            Value::Object(m) => m,
            _ => unreachable!(),
        }
    }
}

pub(crate) fn query_tasks(
    c: &Connection,
    where_sql: &str,
    p: &[&dyn rusqlite::ToSql],
) -> ApiResult<Vec<Task>> {
    let sql = format!("SELECT {COLS} FROM tasks WHERE {where_sql}");
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map(p, Task::from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Any row, deleted or not.
pub(crate) fn load_any(c: &Connection, id: i64) -> ApiResult<Option<Task>> {
    Ok(c.query_row(
        &format!("SELECT {COLS} FROM tasks WHERE id = ?1"),
        [id],
        Task::from_row,
    )
    .optional()?)
}

/// A live (not deleted) task or `not_found`.
pub(crate) fn load(c: &Connection, id: i64) -> ApiResult<Task> {
    match load_any(c, id)? {
        Some(t) if t.deleted_at.is_none() => Ok(t),
        _ => Err(ApiError::not_found(format!("找不到任务 {id}"))),
    }
}

pub(crate) fn live_children(c: &Connection, parent: i64) -> ApiResult<Vec<Task>> {
    query_tasks(
        c,
        "parent_id = ?1 AND deleted_at IS NULL ORDER BY sort_order, id",
        &[&parent],
    )
}

/// JSON for a top-level task (with `subtasks`) or a subtask (without).
pub(crate) fn task_json(c: &Connection, t: &Task) -> ApiResult<Value> {
    let mut m = t.base_json();
    if t.parent_id.is_none() {
        let subs = if t.deleted_at.is_some() {
            query_tasks(
                c,
                "parent_id = ?1 AND deleted_at IS NOT NULL AND id IN \
                 (SELECT child_id FROM task_delete_children WHERE parent_id = ?1) \
                 ORDER BY sort_order, id",
                &[&t.id],
            )?
        } else {
            live_children(c, t.id)?
        };
        let subs: Vec<Value> = subs.iter().map(|s| Value::Object(s.base_json())).collect();
        m.insert("subtasks".into(), Value::Array(subs));
    }
    Ok(Value::Object(m))
}

pub(crate) fn tasks_json(c: &Connection, ts: &[Task]) -> ApiResult<Value> {
    Ok(Value::Array(
        ts.iter().map(|t| task_json(c, t)).collect::<ApiResult<Vec<_>>>()?,
    ))
}

/// `{"task": ..., "parent"?: ...}` for an operation on `id`.
pub(crate) fn task_result(c: &Connection, id: i64) -> ApiResult<Obj> {
    let t = load_any(c, id)?.ok_or_else(|| ApiError::not_found(format!("找不到任务 {id}")))?;
    let mut out = Obj::new();
    out.insert("task".into(), task_json(c, &t)?);
    if let Some(pid) = t.parent_id {
        if let Some(p) = load_any(c, pid)? {
            out.insert("parent".into(), task_json(c, &p)?);
        }
    }
    Ok(out)
}

pub(crate) fn date_arg(a: &Args, k: &str, ctx: &Ctx) -> ApiResult<NaiveDate> {
    match a.opt_str(k)? {
        Some(s) => parse_date(s),
        None => Ok(ctx.today()),
    }
}

fn next_sort(c: &Connection, date: &str) -> ApiResult<i64> {
    Ok(c.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM tasks \
         WHERE date = ?1 AND parent_id IS NULL AND deleted_at IS NULL",
        [date],
        |r| r.get(0),
    )?)
}

fn next_sub_sort(c: &Connection, parent: i64) -> ApiResult<i64> {
    Ok(c.query_row(
        "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM tasks WHERE parent_id = ?1",
        [parent],
        |r| r.get(0),
    )?)
}

fn clean_title(v: Option<&str>) -> ApiResult<String> {
    let t = v.map(str::trim).unwrap_or("");
    if t.is_empty() {
        return Err(ApiError::invalid("任务标题不能为空"));
    }
    Ok(t.to_string())
}

fn priority_of(v: Option<&Value>) -> ApiResult<i64> {
    match v {
        None | Some(Value::Null) => Ok(0),
        Some(Value::Bool(b)) => Ok(*b as i64),
        Some(v) => match crate::api::as_i64(v) {
            Some(p @ (0 | 1)) => Ok(p),
            _ => Err(ApiError::invalid("priority 只能是 0（普通）或 1（重要）")),
        },
    }
}

pub(crate) struct NewTask<'a> {
    pub date: NaiveDate,
    pub title: String,
    pub note: Option<String>,
    pub remind_at: Option<String>,
    pub priority: i64,
    pub parent_id: Option<i64>,
    pub series_id: Option<i64>,
    pub source: &'a str,
}

pub(crate) fn insert(c: &Connection, t: &NewTask, ctx: &Ctx) -> ApiResult<i64> {
    let date = fmt_date(t.date);
    let sort = match t.parent_id {
        Some(p) => next_sub_sort(c, p)?,
        None => next_sort(c, &date)?,
    };
    let now = fmt_dt(ctx.now);
    c.execute(
        "INSERT INTO tasks (date, title, note, status, remind_at, priority, sort_order, parent_id, \
         series_id, source, created_at, updated_at) \
         VALUES (?1, ?2, ?3, 'todo', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        params![
            date, t.title, t.note, t.remind_at, t.priority, sort, t.parent_id, t.series_id, t.source,
            now
        ],
    )?;
    Ok(c.last_insert_rowid())
}

fn insert_subtasks(c: &Connection, parent: &Task, subs: &[Value], ctx: &Ctx) -> ApiResult<()> {
    let date = parse_date(&parent.date)?;
    for s in subs {
        let (title, note) = match s {
            Value::String(t) => (clean_title(Some(t))?, None),
            Value::Object(o) => {
                let sa = Args(o);
                (clean_title(sa.opt_str("title")?)?, sa.opt_str("note")?.map(String::from))
            }
            _ => return Err(ApiError::invalid("子任务应为字符串或 {title, note?}")),
        };
        insert(
            c,
            &NewTask {
                date,
                title,
                note,
                remind_at: None,
                priority: 0,
                parent_id: Some(parent.id),
                series_id: None,
                source: &ctx.source,
            },
            ctx,
        )?;
    }
    Ok(())
}

pub(crate) fn add_tasks(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let date = date_arg(a, "date", ctx)?;
    let items = a.array("tasks")?;
    if items.is_empty() {
        return Err(ApiError::invalid("tasks 不能为空"));
    }
    let mut created = Vec::new();
    for item in items {
        let o = item
            .as_object()
            .ok_or_else(|| ApiError::invalid("tasks 的每一项应为对象 {title, ...}"))?;
        let ta = Args(o);
        let remind_at = match ta.opt_str("remind_at")? {
            Some(s) => Some(fmt_dt(parse_datetime(s, date)?)),
            None => None,
        };
        let id = insert(
            c,
            &NewTask {
                date,
                title: clean_title(ta.opt_str("title")?)?,
                note: ta.opt_str("note")?.map(String::from),
                remind_at,
                priority: priority_of(ta.get("priority"))?,
                parent_id: None,
                series_id: None,
                source: &ctx.source,
            },
            ctx,
        )?;
        let parent = load(c, id)?;
        if let Some(subs) = ta.get("subtasks") {
            let subs = subs
                .as_array()
                .ok_or_else(|| ApiError::invalid("subtasks 应为数组"))?;
            insert_subtasks(c, &parent, subs, ctx)?;
        }
        created.push(parent);
    }
    let mut out = Obj::new();
    out.insert("tasks".into(), tasks_json(c, &created)?);
    Ok(out)
}

pub(crate) fn add_subtasks(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let parent = load(c, a.i64("parent_id")?)?;
    if parent.parent_id.is_some() {
        return Err(ApiError::invalid("子任务只有一层，不能给子任务再加子任务"));
    }
    let subs = a.array("subtasks")?;
    if subs.is_empty() {
        return Err(ApiError::invalid("subtasks 不能为空"));
    }
    insert_subtasks(c, &parent, subs, ctx)?;
    // A done parent never has open subtasks: new open work reopens it.
    if parent.is_done() {
        set_status(c, &parent, false, ctx)?;
    }
    let parent = load(c, parent.id)?;
    let mut out = Obj::new();
    out.insert("tasks".into(), tasks_json(c, &[parent])?);
    Ok(out)
}

/// Flip one row's status; no-op if already in that state (keeps original completion record).
fn set_status(c: &Connection, t: &Task, done: bool, ctx: &Ctx) -> ApiResult<()> {
    if t.is_done() == done {
        return Ok(());
    }
    let now = fmt_dt(ctx.now);
    if done {
        c.execute(
            "UPDATE tasks SET status='done', completed_at=?2, completed_by=?3, updated_at=?2 WHERE id=?1",
            params![t.id, now, ctx.source],
        )?;
    } else {
        c.execute(
            "UPDATE tasks SET status='todo', completed_at=NULL, completed_by=NULL, updated_at=?2 WHERE id=?1",
            params![t.id, now],
        )?;
    }
    Ok(())
}

pub(crate) fn complete_task(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let t = load(c, a.i64("id")?)?;
    let done = a.opt_bool("done")?.unwrap_or(true);
    set_status(c, &t, done, ctx)?;
    match (t.parent_id, done) {
        (None, true) => {
            for child in live_children(c, t.id)? {
                set_status(c, &child, true, ctx)?;
            }
        }
        (None, false) => {}
        (Some(pid), true) => {
            let all_done = live_children(c, pid)?.iter().all(Task::is_done);
            if all_done {
                set_status(c, &load(c, pid)?, true, ctx)?;
            }
        }
        (Some(pid), false) => set_status(c, &load(c, pid)?, false, ctx)?,
    }
    task_result(c, t.id)
}

/// Set `remind_at`; any actual change clears `reminded_at` and `snooze_until`.
fn set_remind(c: &Connection, t: &Task, new: Option<String>, ctx: &Ctx) -> ApiResult<()> {
    if t.remind_at == new {
        return Ok(());
    }
    c.execute(
        "UPDATE tasks SET remind_at=?2, reminded_at=NULL, snooze_until=NULL, updated_at=?3 WHERE id=?1",
        params![t.id, new, fmt_dt(ctx.now)],
    )?;
    Ok(())
}

/// The same wall-clock time of `remind_at`, moved onto `date`.
fn shifted(remind_at: &Option<String>, date: NaiveDate) -> Option<String> {
    remind_at
        .as_deref()
        .and_then(crate::time::stored_dt)
        .map(|dt| fmt_dt(date.and_time(dt.time())))
}

/// Move a top-level task (and all its subtasks) to `date`, appended at the end of that day.
/// A recurring instance that is moved is detached from its series.
pub(crate) fn move_task(c: &Connection, t: &Task, date: NaiveDate, ctx: &Ctx) -> ApiResult<()> {
    if t.parent_id.is_some() {
        return Err(ApiError::invalid(format!(
            "任务 {} 是子任务，日期跟随父任务，请移动父任务 {}",
            t.id,
            t.parent_id.unwrap_or_default()
        )));
    }
    let new_date = fmt_date(date);
    if t.date == new_date {
        return Ok(());
    }
    let sort = next_sort(c, &new_date)?;
    let now = fmt_dt(ctx.now);
    c.execute(
        "UPDATE tasks SET date=?2, sort_order=?3, series_id=NULL, updated_at=?4 WHERE id=?1",
        params![t.id, new_date, sort, now],
    )?;
    let children = query_tasks(c, "parent_id = ?1", &[&t.id])?;
    c.execute(
        "UPDATE tasks SET date=?2, updated_at=?3 WHERE parent_id=?1",
        params![t.id, new_date, now],
    )?;
    for row in std::iter::once(t).chain(children.iter()) {
        set_remind(c, row, shifted(&row.remind_at, date), ctx)?;
    }
    Ok(())
}

pub(crate) fn update_task(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let t = load(c, a.i64("id")?)?;
    let now = fmt_dt(ctx.now);
    if a.has("title") {
        let title = clean_title(a.opt_str("title")?)?;
        c.execute(
            "UPDATE tasks SET title=?2, updated_at=?3 WHERE id=?1",
            params![t.id, title, now],
        )?;
    }
    if a.has("note") {
        let note = a.opt_str("note")?;
        c.execute(
            "UPDATE tasks SET note=?2, updated_at=?3 WHERE id=?1",
            params![t.id, note, now],
        )?;
    }
    if a.has("priority") {
        let p = priority_of(a.get("priority"))?;
        c.execute(
            "UPDATE tasks SET priority=?2, updated_at=?3 WHERE id=?1",
            params![t.id, p, now],
        )?;
    }
    let mut date = parse_date(&t.date)?;
    if let Some(d) = a.opt_str("date")? {
        date = parse_date(d)?;
        if fmt_date(date) != t.date {
            move_task(c, &t, date, ctx)?;
        }
    }
    if a.has("remind_at") {
        let new = match a.opt_str("remind_at")? {
            Some(s) => Some(fmt_dt(parse_datetime(s, date)?)),
            None => None,
        };
        set_remind(c, &load(c, t.id)?, new, ctx)?;
    }
    task_result(c, t.id)
}

pub(crate) fn move_tasks(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let ids = a.ids("ids")?;
    let date = parse_date(a.str("date")?)?;
    let mut moved = Vec::new();
    for id in ids {
        let t = load(c, id)?;
        move_task(c, &t, date, ctx)?;
        moved.push(load(c, id)?);
    }
    let mut out = Obj::new();
    out.insert("tasks".into(), tasks_json(c, &moved)?);
    Ok(out)
}

pub(crate) fn mark_reminded(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let t = load(c, a.i64("id")?)?;
    c.execute(
        "UPDATE tasks SET reminded_at=?2 WHERE id=?1",
        params![t.id, fmt_dt(ctx.now)],
    )?;
    task_result(c, t.id)
}

pub(crate) fn snooze_task(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let t = load(c, a.i64("id")?)?;
    if t.remind_at.is_none() {
        return Err(ApiError::invalid("这个任务没有设置提醒，不能稍后提醒"));
    }
    let until = match (a.opt_str("until")?, a.opt_i64("minutes")?) {
        (Some(s), _) => parse_datetime(s, ctx.today())?,
        (None, Some(m)) if m > 0 => chrono::Duration::try_minutes(m)
            .and_then(|duration| ctx.now.checked_add_signed(duration))
            .ok_or_else(|| ApiError::invalid("minutes 超出可表示的日期范围"))?,
        (None, Some(_)) => return Err(ApiError::invalid("minutes 必须大于 0")),
        (None, None) => return Err(ApiError::invalid("需要 until 或 minutes")),
    };
    c.execute(
        "UPDATE tasks SET snooze_until=?2 WHERE id=?1",
        params![t.id, fmt_dt(until)],
    )?;
    task_result(c, t.id)
}

/// Open, live tasks whose effective reminder time (snooze, else remind_at) has passed and
/// which have not been reminded since that time. Stored times sort lexicographically.
pub(crate) fn due_reminders(c: &Connection, _a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    crate::recurring::ensure_generated(c, ctx.today(), ctx.today(), ctx)?;
    let now = fmt_dt(ctx.now);
    let ts = query_tasks(
        c,
        "status = 'todo' AND deleted_at IS NULL AND remind_at IS NOT NULL \
         AND COALESCE(snooze_until, remind_at) <= ?1 \
         AND (reminded_at IS NULL OR reminded_at < COALESCE(snooze_until, remind_at)) \
         ORDER BY COALESCE(snooze_until, remind_at), id",
        &[&now],
    )?;
    let mut out = Obj::new();
    out.insert("tasks".into(), tasks_json(c, &ts)?);
    Ok(out)
}

/// Listed ids first in the given order, then the day's other tasks in their old order.
pub(crate) fn reorder_tasks(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let date = fmt_date(parse_date(a.str("date")?)?);
    let ids = a.ids("ids")?;
    let day = query_tasks(
        c,
        "parent_id IS NULL AND deleted_at IS NULL AND date = ?1 ORDER BY sort_order, id",
        &[&date],
    )?;
    let mut order: Vec<i64> = Vec::with_capacity(day.len());
    for id in &ids {
        if !day.iter().any(|t| t.id == *id) {
            return Err(ApiError::invalid(format!(
                "任务 {id} 不是 {date} 当天的顶层任务"
            )));
        }
        if order.contains(id) {
            return Err(ApiError::invalid(format!("ids 里任务 {id} 重复")));
        }
        order.push(*id);
    }
    order.extend(day.iter().map(|t| t.id).filter(|id| !ids.contains(id)));
    let now = fmt_dt(ctx.now);
    for (i, id) in order.iter().enumerate() {
        c.execute(
            "UPDATE tasks SET sort_order=?2, updated_at=?3 WHERE id=?1",
            params![id, i as i64 + 1, now],
        )?;
    }
    let mut args = Obj::new();
    args.insert("date".into(), json!(date));
    list_tasks(c, &Args(&args), ctx)
}

pub(crate) fn delete_task(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let t = load(c, a.i64("id")?)?;
    let now = fmt_dt(ctx.now);
    if t.parent_id.is_none() {
        c.execute("DELETE FROM task_delete_children WHERE parent_id=?1", [t.id])?;
        c.execute(
            "INSERT INTO task_delete_children (parent_id, child_id) \
             SELECT ?1, id FROM tasks WHERE parent_id=?1 AND deleted_at IS NULL",
            [t.id],
        )?;
    }
    c.execute(
        "UPDATE tasks SET deleted_at=?2 WHERE id=?1 OR (parent_id=?1 AND deleted_at IS NULL)",
        params![t.id, now],
    )?;
    task_result(c, t.id)
}

pub(crate) fn restore_task(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let id = a.i64("id")?;
    let t = load_any(c, id)?.ok_or_else(|| ApiError::not_found(format!("找不到任务 {id}")))?;
    if t.deleted_at.is_none() {
        return task_result(c, id);
    }
    if let Some(pid) = t.parent_id {
        if load(c, pid).is_err() {
            return Err(ApiError::invalid(format!(
                "父任务 {pid} 已删除，请先恢复父任务"
            )));
        }
    }
    c.execute(
        "UPDATE tasks SET deleted_at=NULL, updated_at=?2 \
         WHERE id=?1 OR (parent_id=?1 AND id IN \
         (SELECT child_id FROM task_delete_children WHERE parent_id=?1))",
        params![id, fmt_dt(ctx.now)],
    )?;
    c.execute("DELETE FROM task_delete_children WHERE parent_id=?1", [id])?;
    if let Some(pid) = t.parent_id {
        let parent = load(c, pid)?;
        if !t.is_done() && parent.is_done() {
            set_status(c, &parent, false, ctx)?;
        }
    }
    task_result(c, id)
}

/// `date` → that day; `from`+`to` → that range; nothing → today.
pub(crate) fn range_args(a: &Args, ctx: &Ctx) -> ApiResult<(NaiveDate, NaiveDate)> {
    let date = a.opt_str("date")?;
    let from = a.opt_str("from")?;
    let to = a.opt_str("to")?;
    let (f, t) = match (date, from, to) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            return Err(ApiError::invalid("date 与 from/to 只能二选一"))
        }
        (Some(d), None, None) => {
            let d = parse_date(d)?;
            (d, d)
        }
        (None, Some(f), Some(t)) => (parse_date(f)?, parse_date(t)?),
        (None, Some(_), None) | (None, None, Some(_)) => {
            return Err(ApiError::invalid("from 和 to 需要同时给出"))
        }
        (None, None, None) => (ctx.today(), ctx.today()),
    };
    if f > t {
        return Err(ApiError::invalid("from 不能晚于 to"));
    }
    if (t - f).num_days() > MAX_RANGE_DAYS {
        return Err(ApiError::invalid(format!(
            "日期范围最长 {} 天",
            MAX_RANGE_DAYS + 1
        )));
    }
    Ok((f, t))
}

const MAX_RANGE_DAYS: i64 = 365;

fn overdue(c: &Connection, ctx: &Ctx) -> ApiResult<Vec<Task>> {
    let today = fmt_date(ctx.today());
    query_tasks(
        c,
        "parent_id IS NULL AND deleted_at IS NULL AND status = 'todo' AND date < ?1 \
         ORDER BY date, sort_order, id",
        &[&today],
    )
}

pub(crate) fn list_tasks(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let status = a.opt_str("status")?.unwrap_or("all");
    let ts = if status == "overdue" {
        overdue(c, ctx)?
    } else {
        let filter = match status {
            "all" => "",
            "todo" => " AND status = 'todo'",
            "done" => " AND status = 'done'",
            _ => {
                return Err(ApiError::invalid(
                    "status 只能是 all / todo / done / overdue",
                ))
            }
        };
        let (f, t) = range_args(a, ctx)?;
        crate::recurring::ensure_generated(c, f, t, ctx)?;
        query_tasks(
            c,
            &format!(
                "parent_id IS NULL AND deleted_at IS NULL AND date BETWEEN ?1 AND ?2{filter} \
                 ORDER BY date, sort_order, id"
            ),
            &[&fmt_date(f), &fmt_date(t)],
        )?
    };
    let mut out = Obj::new();
    out.insert("tasks".into(), tasks_json(c, &ts)?);
    Ok(out)
}

pub(crate) fn get_summary(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let (f, t) = range_args(a, ctx)?;
    crate::recurring::ensure_generated(c, f, t, ctx)?;
    let mut counts = std::collections::HashMap::new();
    {
        let mut st = c.prepare(
            "SELECT date, COUNT(*), COALESCE(SUM(status = 'done'), 0) FROM tasks \
             WHERE parent_id IS NULL AND deleted_at IS NULL AND date BETWEEN ?1 AND ?2 \
             GROUP BY date",
        )?;
        let rows = st.query_map(params![fmt_date(f), fmt_date(t)], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        })?;
        for row in rows {
            let (d, total, done) = row?;
            counts.insert(d, (total, done));
        }
    }
    let (mut total, mut done) = (0, 0);
    let mut days = Vec::new();
    let mut d = f;
    while d <= t {
        let key = fmt_date(d);
        let (dt, dd) = counts.get(&key).copied().unwrap_or((0, 0));
        total += dt;
        done += dd;
        days.push(json!({"date": key, "total": dt, "done": dd, "todo": dt - dd}));
        d = d.succ_opt().ok_or_else(|| ApiError::invalid("日期超出范围"))?;
    }
    let mut out = Obj::new();
    out.insert("from".into(), json!(fmt_date(f)));
    out.insert("to".into(), json!(fmt_date(t)));
    out.insert("total".into(), json!(total));
    out.insert("done".into(), json!(done));
    out.insert("todo".into(), json!(total - done));
    out.insert("days".into(), Value::Array(days));
    out.insert("overdue".into(), tasks_json(c, &overdue(c, ctx)?)?);
    Ok(out)
}
