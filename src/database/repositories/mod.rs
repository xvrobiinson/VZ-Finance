//! Typed access to each table. Repositories only read and write rows; rules
//! about what may be written live in `services`.

pub mod assets;
pub mod capital;
pub mod changes;
pub mod clients;
pub mod entries;
pub mod late_fees;
pub mod loans;
pub mod payments;
pub mod settings;

use chrono::NaiveDate;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

/// `NaiveDate` stored as ISO text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day(pub NaiveDate);

impl ToSql for Day {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(crate::utils::dates::to_iso(self.0)))
    }
}

impl FromSql for Day {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = value.as_str()?;
        crate::utils::dates::parse_iso(s).map(Day).ok_or_else(|| FromSqlError::Other(format!("bad date {s:?}").into()))
    }
}

pub(crate) fn opt_text(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}
