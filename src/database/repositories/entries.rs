//! Expenses, transfers between accounts and balance adjustments.

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::money::Cents;
use crate::domain::portfolio::{Account, EntryFact, EntryKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CashEntry {
    pub id: i64,
    pub kind: EntryKind,
    /// Expense: the account that paid. Transfer: the account money left.
    /// Adjustment: the account corrected.
    pub account: Account,
    /// Positive for expenses and transfers; signed for adjustments.
    pub amount: Cents,
    pub occurred_on: NaiveDate,
    pub description: String,
    pub created_at: String,
}

impl CashEntry {
    pub fn fact(&self) -> EntryFact {
        EntryFact { id: self.id, kind: self.kind, account: self.account, amount: self.amount, occurred_on: self.occurred_on }
    }
}

const COLUMNS: &str = "id, kind, account, amount, occurred_on, description, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<CashEntry> {
    let bad = |what: &str, v: String| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, format!("bad {what} {v:?}").into())
    };
    let kind: String = r.get(1)?;
    let account: String = r.get(2)?;
    let on: Day = r.get(4)?;
    Ok(CashEntry {
        id: r.get(0)?,
        kind: EntryKind::from_key(&kind).ok_or_else(|| bad("kind", kind.clone()))?,
        account: Account::from_key(&account).ok_or_else(|| bad("account", account.clone()))?,
        amount: r.get(3)?,
        occurred_on: on.0,
        description: r.get(5)?,
        created_at: r.get(6)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<CashEntry>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM cash_entries ORDER BY occurred_on, id"))?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<CashEntry>> {
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM cash_entries WHERE id = ?1"), [id], from_row).optional()?)
}

pub fn insert(conn: &Connection, kind: EntryKind, account: Account, amount: Cents, on: NaiveDate, description: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO cash_entries (kind, account, amount, occurred_on, description, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![kind.key(), account.key(), amount, Day(on), description.trim(), now_timestamp()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, account: Account, amount: Cents, on: NaiveDate, description: &str) -> Result<()> {
    conn.execute(
        "UPDATE cash_entries SET account = ?1, amount = ?2, occurred_on = ?3, description = ?4 WHERE id = ?5",
        params![account.key(), amount, Day(on), description.trim(), id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM cash_entries WHERE id = ?1", [id])?;
    Ok(())
}
