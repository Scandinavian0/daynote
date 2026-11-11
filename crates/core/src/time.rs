//! Date/time parsing and formatting. All times are local wall-clock, no zone.
use crate::api::ApiError;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

pub const DATE_FMT: &str = "%Y-%m-%d";
pub const DT_FMT: &str = "%Y-%m-%dT%H:%M:%S";

pub fn fmt_date(d: NaiveDate) -> String {
    d.format(DATE_FMT).to_string()
}

pub fn fmt_dt(dt: NaiveDateTime) -> String {
    dt.format(DT_FMT).to_string()
}

pub fn parse_date(s: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s.trim(), DATE_FMT)
        .map_err(|_| ApiError::invalid(format!("日期格式应为 YYYY-MM-DD：{s}")))
}

/// Parse a stored or user-supplied datetime. A bare `HH:MM[:SS]` is taken on `base`.
pub fn parse_datetime(s: &str, base: NaiveDate) -> Result<NaiveDateTime, ApiError> {
    let t = s.trim();
    for f in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(t, f) {
            return Ok(dt);
        }
    }
    if let Ok(time) = parse_time(t) {
        return Ok(base.and_time(time));
    }
    Err(ApiError::invalid(format!(
        "时间格式应为 YYYY-MM-DD HH:MM、YYYY-MM-DDTHH:MM[:SS] 或 HH:MM：{s}"
    )))
}

pub fn parse_time(s: &str) -> Result<NaiveTime, ApiError> {
    let t = s.trim();
    NaiveTime::parse_from_str(t, "%H:%M:%S")
        .or_else(|_| NaiveTime::parse_from_str(t, "%H:%M"))
        .map_err(|_| ApiError::invalid(format!("时刻格式应为 HH:MM：{s}")))
}

/// Parse a value that this crate itself stored (always `DT_FMT`).
pub fn stored_dt(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, DT_FMT).ok()
}
