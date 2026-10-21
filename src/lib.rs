//! VZ Finance: a private, offline lending portfolio manager.
//!
//! - `domain`: pure financial rules (money, schedules, accrual, portfolio).
//! - `database`: SQLite schema, migrations, repositories, backup/restore.
//! - `services`: validated, transactional operations over the database.
//! - `ui`, `app`, `charts`: the GPUI interface.

pub mod app;
pub mod charts;
pub mod database;
pub mod domain;
pub mod services;
pub mod ui;
pub mod utils;
