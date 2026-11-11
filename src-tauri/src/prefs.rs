//! Shell settings: defaults, parsing from the `settings` table, and validation of
//! the keys the UI may change. Values are stored as strings in daynote-core.
use crate::geometry::Rect;
use chrono::{NaiveDate, NaiveTime};
use serde_json::{Map, Value};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+Space";
/// Tried in order when the default shortcut already belongs to another program.
pub const FALLBACK_HOTKEYS: &[&str] = &["Ctrl+Alt+N", "Ctrl+Alt+J", "Alt+Shift+N"];
pub const DEFAULT_PORT: u16 = 47821;

#[derive(Debug, Clone, PartialEq)]
pub struct Prefs {
    pub expanded: bool,
    pub pinned: bool,
    pub hotkey: String,
    pub morning_enabled: bool,
    pub morning_time: NaiveTime,
    pub evening_enabled: bool,
    pub evening_time: NaiveTime,
    pub sound: bool,
    pub http_enabled: bool,
    pub http_port: u16,
    pub win_compact: Option<Rect>,
    pub win_expanded: Option<Rect>,
    pub autostart_initialized: bool,
    pub last_morning: Option<NaiveDate>,
    pub last_evening: Option<NaiveDate>,
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap_or_default()
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            expanded: false,
            pinned: false,
            hotkey: DEFAULT_HOTKEY.into(),
            morning_enabled: true,
            morning_time: hm(9, 0),
            evening_enabled: true,
            evening_time: hm(18, 0),
            sound: true,
            http_enabled: true,
            http_port: DEFAULT_PORT,
            win_compact: None,
            win_expanded: None,
            autostart_initialized: false,
            last_morning: None,
            last_evening: None,
        }
    }
}

pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let t = s.trim();
    NaiveTime::parse_from_str(t, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(t, "%H:%M:%S"))
        .ok()
}

pub fn fmt_hhmm(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

fn parse_port(s: &str) -> Option<u16> {
    s.trim().parse::<u16>().ok().filter(|p| *p >= 1024)
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

impl Prefs {
    /// Build from the `settings` object returned by `get_settings`; anything missing or
    /// unreadable falls back to its default.
    pub fn from_settings(map: &Map<String, Value>) -> Prefs {
        let get = |k: &str| map.get(k).and_then(Value::as_str);
        let d = Prefs::default();
        let b = |k: &str, def: bool| get(k).and_then(parse_bool).unwrap_or(def);
        Prefs {
            expanded: b("expanded", d.expanded),
            pinned: b("pinned", d.pinned),
            hotkey: get("hotkey")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .unwrap_or(d.hotkey),
            morning_enabled: b("morning_enabled", d.morning_enabled),
            morning_time: get("morning_time").and_then(parse_hhmm).unwrap_or(d.morning_time),
            evening_enabled: b("evening_enabled", d.evening_enabled),
            evening_time: get("evening_time").and_then(parse_hhmm).unwrap_or(d.evening_time),
            sound: b("sound", d.sound),
            http_enabled: b("http_enabled", d.http_enabled),
            http_port: get("http_port").and_then(parse_port).unwrap_or(d.http_port),
            win_compact: get("win_compact").and_then(Rect::parse),
            win_expanded: get("win_expanded").and_then(Rect::parse),
            autostart_initialized: get("autostart_initialized").is_some(),
            last_morning: get("last_morning").and_then(parse_date),
            last_evening: get("last_evening").and_then(parse_date),
        }
    }

    /// Apply a value already normalised by [`validate_pref`].
    pub fn apply(&mut self, key: &str, value: &str) {
        match key {
            "morning_enabled" => self.morning_enabled = value == "true",
            "evening_enabled" => self.evening_enabled = value == "true",
            "sound" => self.sound = value == "true",
            "http_enabled" => self.http_enabled = value == "true",
            "morning_time" => {
                if let Some(t) = parse_hhmm(value) {
                    self.morning_time = t
                }
            }
            "evening_time" => {
                if let Some(t) = parse_hhmm(value) {
                    self.evening_time = t
                }
            }
            "http_port" => {
                if let Some(p) = parse_port(value) {
                    self.http_port = p
                }
            }
            _ => {}
        }
    }
}

/// Check one `set_pref` key/value and return the normalised value to store.
pub fn validate_pref(key: &str, value: &str) -> Result<String, String> {
    match key {
        "morning_enabled" | "evening_enabled" | "sound" | "http_enabled" => parse_bool(value)
            .map(|b| b.to_string())
            .ok_or_else(|| format!("{key} 只能是 true 或 false")),
        "morning_time" | "evening_time" => parse_hhmm(value)
            .map(fmt_hhmm)
            .ok_or_else(|| format!("时间格式应为 HH:MM：{value}")),
        "http_port" => parse_port(value)
            .map(|p| p.to_string())
            .ok_or_else(|| format!("端口应为 1024–65535 之间的整数：{value}")),
        _ => Err(format!("不能修改设置项 {key}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_when_empty() {
        let p = Prefs::from_settings(&Map::new());
        assert_eq!(p, Prefs::default());
        assert_eq!(p.hotkey, "Ctrl+Alt+Space");
        assert_eq!(fmt_hhmm(p.morning_time), "09:00");
        assert_eq!(fmt_hhmm(p.evening_time), "18:00");
        assert_eq!(p.http_port, 47821);
        assert!(p.sound && p.http_enabled && p.morning_enabled && p.evening_enabled);
    }

    #[test]
    fn reads_stored_values_and_ignores_garbage() {
        let m = json!({
            "expanded": "true", "pinned": "1", "hotkey": "Ctrl+Shift+K",
            "morning_time": "08:30", "evening_enabled": "false", "http_port": "80",
            "win_compact": "1,2,380,640", "win_expanded": "junk",
            "autostart_initialized": "1", "last_morning": "2026-11-11", "sound": "maybe"
        });
        let p = Prefs::from_settings(m.as_object().unwrap());
        assert!(p.expanded && p.pinned);
        assert_eq!(p.hotkey, "Ctrl+Shift+K");
        assert_eq!(fmt_hhmm(p.morning_time), "08:30");
        assert!(!p.evening_enabled);
        assert_eq!(p.http_port, DEFAULT_PORT, "privileged port falls back");
        assert_eq!(p.win_compact, Some(Rect::new(1, 2, 380, 640)));
        assert_eq!(p.win_expanded, None);
        assert!(p.autostart_initialized);
        assert_eq!(p.last_morning, NaiveDate::from_ymd_opt(2026, 11, 11));
        assert!(p.sound, "unreadable bool keeps default");
    }

    #[test]
    fn validates_prefs() {
        assert_eq!(validate_pref("sound", "false").unwrap(), "false");
        assert_eq!(validate_pref("morning_time", "7:05").unwrap(), "07:05");
        assert!(validate_pref("morning_time", "25:00").is_err());
        assert_eq!(validate_pref("http_port", " 50000 ").unwrap(), "50000");
        assert!(validate_pref("http_port", "80").is_err());
        assert!(validate_pref("http_port", "70000").is_err());
        assert!(validate_pref("expanded", "true").is_err());
        assert!(validate_pref("sound", "yes").is_err());
        let mut p = Prefs::default();
        p.apply("http_port", "50000");
        p.apply("evening_enabled", "false");
        assert_eq!(p.http_port, 50000);
        assert!(!p.evening_enabled);
    }
}
