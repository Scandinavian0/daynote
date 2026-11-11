//! daynote-core: storage and business rules shared by the app, the CLI and the MCP server.
//!
//! The only entry point for business operations is [`api::dispatch`]; every front end
//! (GUI, CLI, MCP, HTTP) is a thin shell over it so they behave identically.

pub mod api;
mod db;
mod notes;
mod recurring;
mod specs;
mod tasks;
mod time;
pub mod paths;

pub use db::{Error, Result, Store};

use chrono::{NaiveDateTime, Timelike};
pub use paths::data_dir;

/// Current local wall-clock time, truncated to whole seconds.
pub fn now_local() -> NaiveDateTime {
    let now = chrono::Local::now().naive_local();
    now.with_nanosecond(0).unwrap_or(now)
}
