//! Recurring series and on-demand instance generation.
//!
//! Instances are only generated inside `[max(start, today), min(end, today + 366)]`, never
//! before today. `recurring_generated (series_id, date)` remembers which days were already
//! produced, so a deleted or moved instance does not grow back.
use crate::api::{ApiError, ApiResult, Args, Ctx, Obj};
use crate::tasks::{insert, NewTask};
use crate::time::{fmt_date, fmt_dt, parse_date, parse_time};
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};

pub(crate) const HORIZON_DAYS: i64 = 366;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Rule {
    Daily,
    Weekdays,
    /// ISO weekday numbers, 1 = Monday … 7 = Sunday.
    Weekly(Vec<u32>),
    /// Day of month; clamped to the month's last day.
    Monthly(u32),
}

impl Rule {
    fn parse(s: &str) -> ApiResult<Rule> {
        let bad = || {
            ApiError::invalid(format!(
                "rule 只能是 daily、weekdays、weekly:1,3,5（1=周一…7=周日）或 monthly:15（1-31）：{s}"
            ))
        };
        let s = s.trim();
        match s {
            "daily" => return Ok(Rule::Daily),
            "weekdays" => return Ok(Rule::Weekdays),
            _ => {}
        }
        if let Some(rest) = s.strip_prefix("weekly:") {
            let mut days = Vec::new();
            for part in rest.split(',') {
                let d: u32 = part.trim().parse().map_err(|_| bad())?;
                if !(1..=7).contains(&d) {
                    return Err(bad());
                }
                if !days.contains(&d) {
                    days.push(d);
                }
            }
            days.sort_unstable();
            return Ok(Rule::Weekly(days));
        }
        if let Some(rest) = s.strip_prefix("monthly:") {
            let d: u32 = rest.trim().parse().map_err(|_| bad())?;
            if !(1..=31).contains(&d) {
                return Err(bad());
            }
            return Ok(Rule::Monthly(d));
        }
        Err(bad())
    }

    fn canonical(&self) -> String {
        match self {
            Rule::Daily => "daily".into(),
            Rule::Weekdays => "weekdays".into(),
            Rule::Weekly(days) => format!(
                "weekly:{}",
                days.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
            ),
            Rule::Monthly(d) => format!("monthly:{d}"),
        }
    }

    fn matches(&self, date: NaiveDate) -> bool {
        let wd = date.weekday().number_from_monday();
        match self {
            Rule::Daily => true,
            Rule::Weekdays => wd <= 5,
            Rule::Weekly(days) => days.contains(&wd),
            Rule::Monthly(d) => date.day() == (*d).min(last_day_of_month(date)),
        }
    }
}

fn last_day_of_month(date: NaiveDate) -> u32 {
    let (y, m) = (date.year(), date.month());
    let first_next = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    };
    first_next
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

const COLS: &str =
    "id, title, note, rule, remind_time, start_date, end_date, active, source, created_at, updated_at";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Series {
    id: i64,
    title: String,
    note: Option<String>,
    rule: String,
    remind_time: Option<String>,
    start_date: String,
    end_date: Option<String>,
    active: bool,
    source: String,
    created_at: String,
    updated_at: String,
}

impl Series {
    fn from_row(r: &Row) -> rusqlite::Result<Series> {
        Ok(Series {
            id: r.get(0)?,
            title: r.get(1)?,
            note: r.get(2)?,
            rule: r.get(3)?,
            remind_time: r.get(4)?,
            start_date: r.get(5)?,
            end_date: r.get(6)?,
            active: r.get::<_, i64>(7)? != 0,
            source: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "note": self.note,
            "rule": self.rule,
            "remind_time": self.remind_time,
            "start_date": self.start_date,
            "end_date": self.end_date,
            "active": self.active,
            "source": self.source,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }

    /// Same definition, ignoring bookkeeping timestamps.
    fn same_definition(&self, o: &Series) -> bool {
        (&self.title, &self.note, &self.rule, &self.remind_time, &self.start_date, &self.end_date, self.active)
            == (&o.title, &o.note, &o.rule, &o.remind_time, &o.start_date, &o.end_date, o.active)
    }
}

fn load(c: &Connection, id: i64) -> ApiResult<Series> {
    c.query_row(
        &format!("SELECT {COLS} FROM recurring WHERE id = ?1"),
        [id],
        Series::from_row,
    )
    .optional()?
    .ok_or_else(|| ApiError::not_found(format!("找不到重复任务系列 {id}")))
}

/// Generate missing instances for every active series over `[from, to]` ∩ `[today, today+366]`.
pub(crate) fn ensure_generated(
    c: &Connection,
    from: NaiveDate,
    to: NaiveDate,
    ctx: &Ctx,
) -> ApiResult<()> {
    let today = ctx.today();
    let lo = from.max(today);
    let hi = to.min(today + Duration::days(HORIZON_DAYS));
    if lo > hi {
        return Ok(());
    }
    let all: Vec<Series> = {
        let mut st = c.prepare(&format!(
            "SELECT {COLS} FROM recurring WHERE active = 1 AND start_date <= ?1 \
             AND (end_date IS NULL OR end_date >= ?2) ORDER BY id"
        ))?;
        let rows = st.query_map(params![fmt_date(hi), fmt_date(lo)], Series::from_row)?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for s in all {
        let rule = Rule::parse(&s.rule)?;
        let mut d = lo.max(parse_date(&s.start_date)?);
        let end = match &s.end_date {
            Some(e) => hi.min(parse_date(e)?),
            None => hi,
        };
        let time = s.remind_time.as_deref().map(parse_time).transpose()?;
        while d <= end {
            if rule.matches(d) {
                let claimed = c.execute(
                    "INSERT OR IGNORE INTO recurring_generated (series_id, date) VALUES (?1, ?2)",
                    params![s.id, fmt_date(d)],
                )?;
                if claimed == 1 {
                    insert(
                        c,
                        &NewTask {
                            date: d,
                            title: s.title.clone(),
                            note: s.note.clone(),
                            remind_at: time.map(|t| fmt_dt(d.and_time(t))),
                            priority: 0,
                            parent_id: None,
                            series_id: Some(s.id),
                            source: &s.source,
                        },
                        ctx,
                    )?;
                }
            }
            d = match d.succ_opt() {
                Some(n) => n,
                None => break,
            };
        }
    }
    Ok(())
}

/// Physically remove future (date > today), still-open, still-attached, live instances of a
/// series together with their generation records, so they are rebuilt by the current rule.
/// Instances the user deleted keep their record (the skip is preserved).
fn purge_future(c: &Connection, series_id: i64, ctx: &Ctx) -> ApiResult<()> {
    let today = fmt_date(ctx.today());
    let pick = "SELECT id FROM tasks WHERE series_id = ?1 AND date > ?2 AND status = 'todo' \
                AND deleted_at IS NULL AND parent_id IS NULL";
    c.execute(
        &format!(
            "DELETE FROM recurring_generated WHERE series_id = ?1 AND date IN \
             (SELECT date FROM tasks WHERE id IN ({pick}))"
        ),
        params![series_id, today],
    )?;
    c.execute(
        &format!("DELETE FROM tasks WHERE parent_id IN ({pick})"),
        params![series_id, today],
    )?;
    c.execute(
        &format!("DELETE FROM tasks WHERE id IN ({pick})"),
        params![series_id, today],
    )?;
    Ok(())
}

fn clean_title(v: Option<&str>) -> ApiResult<String> {
    let t = v.map(str::trim).unwrap_or("");
    if t.is_empty() {
        return Err(ApiError::invalid("标题不能为空"));
    }
    Ok(t.to_string())
}

fn remind_time_of(v: Option<&str>) -> ApiResult<Option<String>> {
    v.map(|s| parse_time(s).map(|t| t.format("%H:%M").to_string()))
        .transpose()
}

fn check_dates(start: &str, end: &Option<String>) -> ApiResult<()> {
    if let Some(e) = end {
        if e.as_str() < start {
            return Err(ApiError::invalid("end_date 不能早于 start_date"));
        }
    }
    Ok(())
}

fn one(s: &Series) -> Obj {
    let mut out = Obj::new();
    out.insert("series".into(), s.json());
    out
}

pub(crate) fn add_recurring(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let title = clean_title(a.opt_str("title")?)?;
    let rule = Rule::parse(a.str("rule")?)?.canonical();
    let remind_time = remind_time_of(a.opt_str("remind_time")?)?;
    let start = match a.opt_str("start_date")? {
        Some(s) => fmt_date(parse_date(s)?),
        None => fmt_date(ctx.today()),
    };
    let end = a.opt_str("end_date")?.map(parse_date).transpose()?.map(fmt_date);
    check_dates(&start, &end)?;
    let now = fmt_dt(ctx.now);
    c.execute(
        "INSERT INTO recurring (title, note, rule, remind_time, start_date, end_date, active, \
         source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
        params![title, a.opt_str("note")?, rule, remind_time, start, end, ctx.source, now],
    )?;
    Ok(one(&load(c, c.last_insert_rowid())?))
}

pub(crate) fn list_recurring(c: &Connection, _a: &Args, _ctx: &Ctx) -> ApiResult<Obj> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM recurring ORDER BY id"))?;
    let rows = st.query_map([], Series::from_row)?;
    let all = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Obj::new();
    out.insert(
        "series".into(),
        Value::Array(all.iter().map(Series::json).collect()),
    );
    Ok(out)
}

pub(crate) fn update_recurring(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let old = load(c, a.i64("id")?)?;
    let mut new = old.clone();
    if a.has("title") {
        new.title = clean_title(a.opt_str("title")?)?;
    }
    if a.has("note") {
        new.note = a.opt_str("note")?.map(String::from);
    }
    if a.has("rule") {
        new.rule = Rule::parse(a.str("rule")?)?.canonical();
    }
    if a.has("remind_time") {
        new.remind_time = remind_time_of(a.opt_str("remind_time")?)?;
    }
    if a.has("start_date") {
        new.start_date = fmt_date(parse_date(a.str("start_date")?)?);
    }
    if a.has("end_date") {
        new.end_date = a.opt_str("end_date")?.map(parse_date).transpose()?.map(fmt_date);
    }
    if let Some(active) = a.opt_bool("active")? {
        new.active = active;
    }
    check_dates(&new.start_date, &new.end_date)?;
    if new.same_definition(&old) {
        return Ok(one(&old));
    }
    c.execute(
        "UPDATE recurring SET title=?2, note=?3, rule=?4, remind_time=?5, start_date=?6, \
         end_date=?7, active=?8, updated_at=?9 WHERE id=?1",
        params![
            new.id,
            new.title,
            new.note,
            new.rule,
            new.remind_time,
            new.start_date,
            new.end_date,
            new.active as i64,
            fmt_dt(ctx.now)
        ],
    )?;
    purge_future(c, new.id, ctx)?;
    Ok(one(&load(c, new.id)?))
}

pub(crate) fn delete_recurring(c: &Connection, a: &Args, ctx: &Ctx) -> ApiResult<Obj> {
    let s = load(c, a.i64("id")?)?;
    purge_future(c, s.id, ctx)?;
    c.execute("DELETE FROM recurring_generated WHERE series_id = ?1", [s.id])?;
    c.execute("DELETE FROM recurring WHERE id = ?1", [s.id])?;
    Ok(one(&s))
}
