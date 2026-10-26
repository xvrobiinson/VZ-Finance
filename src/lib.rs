//! Acervus Finance: a private, offline lending portfolio manager, developed by
//! VZ Finance.
//!
//! - `domain`: pure financial rules (money, schedules, accrual, portfolio).
//! - `database`: SQLite schema, migrations, repositories, backup/restore.
//! - `services`: validated, transactional operations over the database.
//! - `ui`, `app`, `charts`: the GPUI interface.
//!
//! Internal names (the `vz_finance` crate, the `VZFinance` data folder, the
//! `vz-finance.exe` program) predate the product name and are kept so
//! existing installations, data and backups keep working.

pub mod app;
pub mod charts;
pub mod database;
pub mod diagnostics;
pub mod domain;
pub mod i18n;
pub mod services;
pub mod ui;
pub mod utils;

/// The product name. Fixed: it is not a setting.
pub const APP_NAME: &str = "Acervus Finance";
/// Shown in the About section.
pub const DEVELOPER: &str = "VZ Finance";
