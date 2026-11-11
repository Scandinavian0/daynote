#![allow(dead_code)]
use chrono::NaiveDateTime;
use daynote_core::api::{self, ApiError, Ctx};
use daynote_core::Store;
use serde_json::Value;

pub fn at(s: &str) -> Ctx {
    Ctx {
        source: "test".into(),
        now: NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap(),
    }
}

pub fn by(source: &str, s: &str) -> Ctx {
    Ctx {
        source: source.into(),
        ..at(s)
    }
}

pub fn store() -> Store {
    Store::open_in_memory().unwrap()
}

pub fn call(store: &mut Store, tool: &str, args: Value, ctx: &Ctx) -> Value {
    match api::dispatch(store, tool, args, ctx) {
        Ok(v) => v,
        Err(e) => panic!("{tool} failed: {} {}", e.code, e.message),
    }
}

pub fn call_err(store: &mut Store, tool: &str, args: Value, ctx: &Ctx) -> ApiError {
    match api::dispatch(store, tool, args, ctx) {
        Ok(v) => panic!("{tool} unexpectedly succeeded: {v}"),
        Err(e) => e,
    }
}

pub fn titles(tasks: &Value) -> Vec<String> {
    tasks
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_string())
        .collect()
}

pub fn id(task: &Value) -> i64 {
    task["id"].as_i64().unwrap()
}
