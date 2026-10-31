//! Acervus Finance: a private lending portfolio manager, made by VZ Labs.
//!
//! - `domain`: pure financial rules (money, schedules, accrual, portfolio).
//! - `database`: SQLite schema, migrations, repositories, backup/restore.
//! - `services`: validated, transactional operations over the database.
//! - `ui`, `app`, `charts`: the GPUI interface.
//!
//! The program is `acervus.exe` and its folder `%LOCALAPPDATA%\VZ Labs\Acervus`.
//! Before 0.9 they were `vz-finance.exe` and `%LOCALAPPDATA%\VZFinance`; the
//! folder is moved on first start (`database::move_legacy_folder`).

pub mod app;
pub mod charts;
pub mod database;
pub mod diagnostics;
pub mod domain;
pub mod file_picker;
pub mod i18n;
pub mod services;
pub mod shell;
pub mod sync;
pub mod ui;
pub mod utils;

/// The product name. Fixed: it is not a setting.
pub const APP_NAME: &str = "Acervus Finance";
/// Who makes it, shown in the About section.
pub const DEVELOPER: &str = "VZ Labs";
/// Where people write for help.
pub const SUPPORT_EMAIL: &str = "officialvzlabs@gmail.com";
/// The Terms of Service and Privacy Policy, published with the web app.
pub const TERMS_URL: &str = "https://acervus-finance-pwa.vz-labs.workers.dev/terms";
pub const PRIVACY_URL: &str = "https://acervus-finance-pwa.vz-labs.workers.dev/privacy";
