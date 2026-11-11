//! Reminder rules as pure functions: given a snapshot of the data and the clock,
//! decide which cards to show and what to record. No windows, no clock, no database.
use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

/// Startup backlog above this many due reminders becomes one "missed" card.
pub const MISSED_THRESHOLD: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Task,
    Missed,
    Morning,
    Evening,
}

/// One card in the reminder window (`Card` in ui/src/lib/types.ts).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Card {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub tasks: Vec<Value>,
}

pub const MISSED_ID: &str = "missed";
pub const MORNING_ID: &str = "morning";
pub const EVENING_ID: &str = "evening";

pub fn task_card_id(id: i64) -> String {
    format!("task-{id}")
}

pub fn parse_task_card_id(card: &str) -> Option<i64> {
    card.strip_prefix("task-")?.parse().ok()
}

// Task JSON accessors -------------------------------------------------------

pub fn task_id(t: &Value) -> Option<i64> {
    t.get("id")?.as_i64()
}

fn text<'a>(t: &'a Value, k: &str) -> Option<&'a str> {
    t.get(k)?.as_str()
}

fn stored_dt(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()
}

/// When the reminder fires: the snooze time if set, else `remind_at`.
pub fn effective_time(t: &Value) -> Option<NaiveDateTime> {
    text(t, "snooze_until")
        .or_else(|| text(t, "remind_at"))
        .and_then(stored_dt)
}

pub fn is_open(t: &Value) -> bool {
    text(t, "status") == Some("todo")
}

pub fn task_date(t: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text(t, "date")?, "%Y-%m-%d").ok()
}

/// `HH:MM · 今天`, or `HH:MM · M月D日` when the task is not on `today`.
pub fn task_subtitle(t: &Value, today: NaiveDate) -> String {
    let day = match task_date(t) {
        Some(d) if d != today => format!("{}月{}日", d.month(), d.day()),
        _ => "今天".to_string(),
    };
    match text(t, "remind_at").and_then(stored_dt) {
        Some(dt) => format!("{} · {day}", dt.format("%H:%M")),
        None => day,
    }
}

fn task_card(t: &Value, today: NaiveDate) -> Option<Card> {
    Some(Card {
        id: task_card_id(task_id(t)?),
        kind: Kind::Task,
        title: text(t, "title").unwrap_or_default().to_string(),
        subtitle: task_subtitle(t, today),
        tasks: vec![t.clone()],
    })
}

fn missed_card(tasks: Vec<Value>) -> Card {
    Card {
        id: MISSED_ID.into(),
        kind: Kind::Missed,
        title: format!("错过了 {} 个提醒", tasks.len()),
        subtitle: String::new(),
        tasks,
    }
}

fn morning_card(today_open: &[Value], overdue: usize) -> Card {
    Card {
        id: MORNING_ID.into(),
        kind: Kind::Morning,
        title: format!("今天 {} 件事", today_open.len()),
        subtitle: if overdue > 0 {
            format!("另有 {overdue} 件遗留")
        } else {
            String::new()
        },
        tasks: today_open.to_vec(),
    }
}

fn evening_card(today_open: &[Value]) -> Card {
    Card {
        id: EVENING_ID.into(),
        kind: Kind::Evening,
        title: format!("还有 {} 件没做完", today_open.len()),
        subtitle: "今天".into(),
        tasks: today_open.to_vec(),
    }
}

// Planning ------------------------------------------------------------------

/// Everything one scheduling round looks at.
pub struct Snapshot<'a> {
    pub now: NaiveDateTime,
    /// Morning briefing time, `None` when the briefing is switched off.
    pub morning: Option<NaiveTime>,
    /// Evening review time, `None` when switched off.
    pub evening: Option<NaiveTime>,
    /// Result of `due_reminders`.
    pub due: &'a [Value],
    /// Today's open top-level tasks.
    pub today_open: &'a [Value],
    /// Number of overdue (earlier, still open) top-level tasks.
    pub overdue: usize,
    pub last_morning: Option<NaiveDate>,
    pub last_evening: Option<NaiveDate>,
    /// First round since the app started.
    pub first_round: bool,
    /// That first round follows a start with Windows (`--autostart`): the day's briefing
    /// is shown straight away rather than waiting for its clock time.
    pub at_login: bool,
    /// Ids of the cards already queued.
    pub queued: &'a [String],
}

/// What a round decided: cards to enqueue (a card whose id is already queued replaces it)
/// and what to write back.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub cards: Vec<Card>,
    pub mark_reminded: Vec<i64>,
    pub last_morning: Option<NaiveDate>,
    pub last_evening: Option<NaiveDate>,
}

pub fn plan(s: &Snapshot) -> Plan {
    let today = s.now.date();
    let mut p = Plan::default();
    let queued = |id: &str| s.queued.iter().any(|q| q == id);

    // 1 + 2: single reminders, or one "missed" card for a startup backlog.
    let mut fresh = Vec::new();
    for t in s.due {
        let Some(id) = task_id(t) else { continue };
        p.mark_reminded.push(id);
        if !queued(&task_card_id(id)) {
            fresh.push(t);
        }
    }
    if s.first_round && fresh.len() > MISSED_THRESHOLD {
        p.cards
            .push(missed_card(fresh.into_iter().cloned().collect()));
    } else {
        p.cards.extend(fresh.into_iter().filter_map(|t| task_card(t, today)));
    }

    // 3: morning briefing. Starting the PC after the evening review time would raise both
    // cards at once with the same list; the evening one alone is enough then.
    let evening_covers = !s.today_open.is_empty()
        && s.evening.is_some_and(|at| s.now >= today.and_time(at));
    if let Some(at) = s.morning {
        // Switching the PC on counts as "morning" from 05:00; before that it is still
        // last night, and the briefing waits for its time.
        let just_logged_in = s.at_login && s.now.hour() >= 5;
        if (s.now >= today.and_time(at) || just_logged_in) && s.last_morning != Some(today) {
            if (!s.today_open.is_empty() || s.overdue > 0) && !evening_covers {
                p.cards.push(morning_card(s.today_open, s.overdue));
            }
            p.last_morning = Some(today);
        }
    }

    // 4: evening review.
    if let Some(at) = s.evening {
        if s.now >= today.and_time(at) && s.last_evening != Some(today) {
            if !s.today_open.is_empty() {
                p.cards.push(evening_card(s.today_open));
            }
            p.last_evening = Some(today);
        }
    }
    p
}

/// Rule 5: re-check queued cards against the latest data. Task cards whose task is gone,
/// done, or whose reminder now lies in the future are dropped; the others get fresh
/// contents. `lookup` maps task id → latest task JSON (top-level tasks and subtasks).
pub fn refresh(
    queue: &[Card],
    lookup: &HashMap<i64, Value>,
    today_open: &[Value],
    overdue: usize,
    now: NaiveDateTime,
) -> Vec<Card> {
    let today = now.date();
    queue
        .iter()
        .filter_map(|card| match card.kind {
            Kind::Task => {
                let t = lookup.get(&parse_task_card_id(&card.id)?)?;
                let due = effective_time(t).is_some_and(|at| at <= now);
                if !is_open(t) || !due {
                    return None;
                }
                task_card(t, today)
            }
            Kind::Missed => {
                let tasks: Vec<Value> = card
                    .tasks
                    .iter()
                    .filter_map(|t| lookup.get(&task_id(t)?))
                    .filter(|t| is_open(t))
                    .cloned()
                    .collect();
                (!tasks.is_empty()).then(|| missed_card(tasks))
            }
            // A briefing or review still on screen after midnight belongs to yesterday:
            // refilling it with the new day's list would make its buttons act on the
            // wrong tasks, so it is dropped instead.
            Kind::Morning | Kind::Evening
                if card.tasks.iter().filter_map(task_date).any(|d| d != today) =>
            {
                None
            }
            Kind::Morning => {
                (!today_open.is_empty() || overdue > 0).then(|| morning_card(today_open, overdue))
            }
            Kind::Evening => (!today_open.is_empty()).then(|| evening_card(today_open)),
        })
        .collect()
}

/// Ids of the tasks a queue refers to, with the date each was on (to look them up).
pub fn referenced_tasks(queue: &[Card]) -> Vec<(i64, Option<NaiveDate>)> {
    queue
        .iter()
        .filter(|c| matches!(c.kind, Kind::Task | Kind::Missed))
        .flat_map(|c| c.tasks.iter())
        .filter_map(|t| Some((task_id(t)?, task_date(t))))
        .collect()
}

/// Merge a round's result into the live queue. Cards removed meanwhile (the user acted
/// on them) stay removed: only cards still present take the refreshed version.
pub fn merge(live: &mut Vec<Card>, refreshed: &[Card], new_cards: Vec<Card>) {
    live.retain(|c| refreshed.iter().any(|r| r.id == c.id));
    for c in live.iter_mut() {
        if let Some(r) = refreshed.iter().find(|r| r.id == c.id) {
            *c = r.clone();
        }
    }
    for card in new_cards {
        match live.iter_mut().find(|c| c.id == card.id) {
            Some(slot) => *slot = card,
            None => live.push(card),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }
    fn t(h: u32, m: u32) -> Option<NaiveTime> {
        NaiveTime::from_hms_opt(h, m, 0)
    }
    fn task(id: i64, date: &str, title: &str, remind: Option<&str>) -> Value {
        json!({"id": id, "date": date, "title": title, "status": "todo",
               "remind_at": remind, "snooze_until": null, "parent_id": null})
    }

    struct S {
        now: NaiveDateTime,
        morning: Option<NaiveTime>,
        evening: Option<NaiveTime>,
        due: Vec<Value>,
        open: Vec<Value>,
        overdue: usize,
        last_morning: Option<NaiveDate>,
        last_evening: Option<NaiveDate>,
        first: bool,
        at_login: bool,
        queued: Vec<String>,
    }

    impl S {
        /// Mid-afternoon, briefings already done today, nothing due.
        fn new(now: &str) -> S {
            let now = dt(now);
            S {
                now,
                morning: t(9, 0),
                evening: t(18, 0),
                due: vec![],
                open: vec![],
                overdue: 0,
                last_morning: Some(now.date()),
                last_evening: Some(now.date()),
                first: false,
                at_login: false,
                queued: vec![],
            }
        }
        fn plan(&self) -> Plan {
            plan(&Snapshot {
                now: self.now,
                morning: self.morning,
                evening: self.evening,
                due: &self.due,
                today_open: &self.open,
                overdue: self.overdue,
                last_morning: self.last_morning,
                last_evening: self.last_evening,
                first_round: self.first,
                at_login: self.at_login,
                queued: &self.queued,
            })
        }
    }

    #[test]
    fn briefing_shows_right_after_login_even_before_its_time() {
        let mut s = S::new("2026-11-11 08:20");
        s.last_morning = None;
        s.first = true;
        s.at_login = true;
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        assert_eq!(p.cards.len(), 1);
        assert_eq!(p.cards[0].kind, Kind::Morning);
        assert_eq!(p.last_morning, Some(d("2026-11-11")));
    }

    #[test]
    fn login_in_the_small_hours_does_not_brief() {
        let mut s = S::new("2026-11-11 01:30");
        s.last_morning = None;
        s.first = true;
        s.at_login = true;
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.last_morning, None);
    }

    #[test]
    fn refresh_drops_a_daily_card_left_over_from_yesterday() {
        let yesterday = vec![task(1, "2026-11-11", "a", None)];
        let queue = vec![evening_card(&yesterday), morning_card(&yesterday, 0)];
        let today_open = vec![task(2, "2026-11-12", "b", None)];
        let out = refresh(&queue, &HashMap::new(), &today_open, 0, dt("2026-11-12 00:05"));
        assert!(out.is_empty());
    }

    // Rule 1 ---------------------------------------------------------------

    #[test]
    fn single_reminder_becomes_a_task_card() {
        let mut s = S::new("2026-11-11 15:01");
        s.due = vec![task(7, "2026-11-11", "回访客户", Some("2026-11-11T15:00:00"))];
        let p = s.plan();
        assert_eq!(p.mark_reminded, vec![7]);
        assert_eq!(p.cards.len(), 1);
        let c = &p.cards[0];
        assert_eq!(c.id, "task-7");
        assert_eq!(c.kind, Kind::Task);
        assert_eq!(c.title, "回访客户");
        assert_eq!(c.subtitle, "15:00 · 今天");
        assert_eq!(c.tasks.len(), 1);
    }

    #[test]
    fn task_on_another_day_shows_its_date() {
        let mut s = S::new("2026-11-11 15:01");
        s.due = vec![task(8, "2026-11-10", "对账", Some("2026-11-10T09:05:00"))];
        assert_eq!(s.plan().cards[0].subtitle, "09:05 · 11月10日");
    }

    #[test]
    fn already_queued_task_is_not_duplicated() {
        let mut s = S::new("2026-11-11 15:01");
        s.due = vec![task(7, "2026-11-11", "回访客户", Some("2026-11-11T15:00:00"))];
        s.queued = vec!["task-7".into()];
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.mark_reminded, vec![7], "still recorded so it does not pop again");
    }

    #[test]
    fn three_due_at_startup_are_still_shown_one_by_one() {
        let mut s = S::new("2026-11-11 08:00");
        s.first = true;
        s.last_morning = None;
        s.morning = None;
        s.due = (1..=3).map(|i| task(i, "2026-11-11", "x", Some("2026-11-11T07:00:00"))).collect();
        let p = s.plan();
        assert_eq!(p.cards.len(), 3);
        assert!(p.cards.iter().all(|c| c.kind == Kind::Task));
    }

    // Rule 2 ---------------------------------------------------------------

    #[test]
    fn startup_backlog_is_merged_into_one_missed_card() {
        let mut s = S::new("2026-11-11 08:00");
        s.first = true;
        s.due = (1..=4).map(|i| task(i, "2026-11-10", "x", Some("2026-11-10T20:00:00"))).collect();
        let p = s.plan();
        assert_eq!(p.cards.len(), 1);
        let c = &p.cards[0];
        assert_eq!((c.id.as_str(), c.kind), ("missed", Kind::Missed));
        assert_eq!(c.title, "错过了 4 个提醒");
        assert_eq!(c.tasks.len(), 4);
        assert_eq!(p.mark_reminded, vec![1, 2, 3, 4]);
    }

    #[test]
    fn backlog_after_the_first_round_is_shown_one_by_one() {
        let mut s = S::new("2026-11-11 08:00");
        s.due = (1..=5).map(|i| task(i, "2026-11-11", "x", Some("2026-11-11T07:59:00"))).collect();
        let p = s.plan();
        assert_eq!(p.cards.len(), 5);
        assert!(p.cards.iter().all(|c| c.kind == Kind::Task));
    }

    // Rule 3 ---------------------------------------------------------------

    #[test]
    fn morning_briefing_with_tasks_and_leftovers() {
        let mut s = S::new("2026-11-11 09:00");
        s.last_morning = Some(d("2026-11-10"));
        s.open = vec![task(1, "2026-11-11", "a", None), task(2, "2026-11-11", "b", None)];
        s.overdue = 3;
        let p = s.plan();
        assert_eq!(p.last_morning, Some(d("2026-11-11")));
        let c = &p.cards[0];
        assert_eq!((c.id.as_str(), c.kind), ("morning", Kind::Morning));
        assert_eq!(c.title, "今天 2 件事");
        assert_eq!(c.subtitle, "另有 3 件遗留");
        assert_eq!(c.tasks.len(), 2);
    }

    #[test]
    fn morning_briefing_without_leftovers_has_empty_subtitle() {
        let mut s = S::new("2026-11-11 10:00");
        s.last_morning = None;
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        assert_eq!(p.cards[0].title, "今天 1 件事");
        assert_eq!(p.cards[0].subtitle, "");
    }

    #[test]
    fn empty_day_records_the_morning_without_a_card() {
        let mut s = S::new("2026-11-11 09:30");
        s.last_morning = Some(d("2026-11-10"));
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.last_morning, Some(d("2026-11-11")));
    }

    #[test]
    fn morning_waits_for_its_time_and_runs_once_a_day() {
        let mut s = S::new("2026-11-11 08:59");
        s.last_morning = Some(d("2026-11-10"));
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.last_morning, None);
        // already shown today
        s.now = dt("2026-11-11 11:00");
        s.last_morning = Some(d("2026-11-11"));
        assert!(s.plan().cards.is_empty());
        // switched off
        s.last_morning = Some(d("2026-11-10"));
        s.morning = None;
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.last_morning, None);
    }

    #[test]
    fn leftovers_alone_still_trigger_the_briefing() {
        let mut s = S::new("2026-11-11 09:00");
        s.last_morning = None;
        s.overdue = 2;
        let c = &s.plan().cards[0];
        assert_eq!(c.title, "今天 0 件事");
        assert_eq!(c.subtitle, "另有 2 件遗留");
    }

    // Rule 4 ---------------------------------------------------------------

    #[test]
    fn evening_review_with_open_tasks() {
        let mut s = S::new("2026-11-11 18:00");
        s.last_evening = Some(d("2026-11-10"));
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        assert_eq!(p.last_evening, Some(d("2026-11-11")));
        let c = &p.cards[0];
        assert_eq!((c.id.as_str(), c.kind), ("evening", Kind::Evening));
        assert_eq!(c.title, "还有 1 件没做完");
        assert_eq!(c.subtitle, "今天");
    }

    #[test]
    fn evening_with_everything_done_records_without_a_card() {
        let mut s = S::new("2026-11-11 19:00");
        s.last_evening = None;
        s.overdue = 4;
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.last_evening, Some(d("2026-11-11")));
        // before its time nothing happens
        s.now = dt("2026-11-11 17:59");
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        assert!(p.cards.is_empty());
        assert_eq!(p.last_evening, None);
    }

    #[test]
    fn late_start_shows_only_the_evening_review() {
        let mut s = S::new("2026-11-11 19:00");
        s.last_morning = None;
        s.last_evening = None;
        s.open = vec![task(1, "2026-11-11", "a", None)];
        let p = s.plan();
        let ids: Vec<&str> = p.cards.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["evening"]);
        // The briefing still counts as handled for today.
        assert_eq!(p.last_morning, Some(d("2026-11-11")));
        assert_eq!(p.last_evening, Some(d("2026-11-11")));
    }

    #[test]
    fn late_start_with_only_leftovers_still_briefs() {
        let mut s = S::new("2026-11-11 19:00");
        s.last_morning = None;
        s.last_evening = None;
        s.overdue = 2;
        let ids: Vec<String> = s.plan().cards.into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["morning"]);
    }

    // Rule 5 ---------------------------------------------------------------

    fn lookup(ts: &[Value]) -> HashMap<i64, Value> {
        ts.iter().map(|t| (task_id(t).unwrap(), t.clone())).collect()
    }

    #[test]
    fn refresh_drops_done_deleted_and_rescheduled_task_cards() {
        let now = dt("2026-11-11 15:10");
        let today = now.date();
        let mk = |id| task_card(&task(id, "2026-11-11", "old", Some("2026-11-11T15:00:00")), today).unwrap();
        let queue = vec![mk(1), mk(2), mk(3), mk(4), mk(5)];
        let mut done = task(1, "2026-11-11", "a", Some("2026-11-11T15:00:00"));
        done["status"] = json!("done");
        let later = task(3, "2026-11-11", "c", Some("2026-11-11T17:00:00"));
        let mut snoozed = task(4, "2026-11-11", "d", Some("2026-11-11T15:00:00"));
        snoozed["snooze_until"] = json!("2026-11-11T15:40:00");
        let renamed = task(5, "2026-11-11", "新标题", Some("2026-11-11T15:00:00"));
        // task 2 was deleted: not in the lookup
        let map = lookup(&[done, later, snoozed, renamed]);
        let out = refresh(&queue, &map, &[], 0, now);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "task-5");
        assert_eq!(out[0].title, "新标题", "contents are refreshed");
    }

    #[test]
    fn refresh_updates_and_drops_summary_cards() {
        let now = dt("2026-11-11 19:00");
        let a = task(1, "2026-11-11", "a", None);
        let b = task(2, "2026-11-11", "b", None);
        let queue = vec![
            morning_card(&[a.clone(), b.clone()], 1),
            evening_card(&[a.clone(), b.clone()]),
            missed_card(vec![a.clone(), b.clone()]),
        ];
        let mut b_done = b.clone();
        b_done["status"] = json!("done");
        let map = lookup(&[a.clone(), b_done]);
        let out = refresh(&queue, &map, &[a.clone()], 1, now);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].title, "今天 1 件事");
        assert_eq!(out[1].title, "还有 1 件没做完");
        assert_eq!(out[2].title, "错过了 1 个提醒");
        assert_eq!(out[2].tasks.len(), 1);
        // everything done, nothing left over
        let out = refresh(&queue, &HashMap::new(), &[], 0, now);
        assert!(out.is_empty());
    }

    #[test]
    fn merge_keeps_user_dismissals() {
        let today = d("2026-11-11");
        let c = |id| task_card(&task(id, "2026-11-11", "x", Some("2026-11-11T15:00:00")), today).unwrap();
        // snapshot had cards 1 and 2; meanwhile the user dismissed card 1
        let mut live = vec![c(2)];
        let refreshed = vec![c(1), c(2)];
        merge(&mut live, &refreshed, vec![c(3)]);
        let ids: Vec<&str> = live.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["task-2", "task-3"]);
    }

    #[test]
    fn referenced_tasks_lists_task_and_missed_cards() {
        let today = d("2026-11-11");
        let a = task(1, "2026-11-11", "a", Some("2026-11-11T15:00:00"));
        let b = task(2, "2026-11-10", "b", Some("2026-11-10T15:00:00"));
        let q = vec![task_card(&a, today).unwrap(), missed_card(vec![b]), evening_card(&[a.clone()])];
        assert_eq!(
            referenced_tasks(&q),
            vec![(1, Some(d("2026-11-11"))), (2, Some(d("2026-11-10")))]
        );
    }
}
