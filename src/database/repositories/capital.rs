use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::money::Cents;
use crate::domain::portfolio::{CapitalFact, CapitalKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapitalTx {
    pub id: i64,
    pub kind: CapitalKind,
    pub amount: Cents,
    pub occurred_on: NaiveDate,
    pub notes: String,
    pub created_at: String,
}

impl CapitalTx {
    pub fn fact(&self) -> CapitalFact {
        CapitalFact { id: self.id, kind: self.kind, amount: self.amount, occurred_on: self.occurred_on }
    }
}

const COLUMNS: &str = "id, kind, amount, occurred_on, notes, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<CapitalTx> {
    let kind: String = r.get(1)?;
    let on: Day = r.get(3)?;
    Ok(CapitalTx {
        id: r.get(0)?,
        kind: CapitalKind::from_key(&kind).unwrap_or(CapitalKind::Contribution),
        amount: r.get(2)?,
        occurred_on: on.0,
        notes: r.get(4)?,
        created_at: r.get(5)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<CapitalTx>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM capital_transactions ORDER BY occurred_on, id"))?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<CapitalTx>> {
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM capital_transactions WHERE id = ?1"), [id], from_row).optional()?)
}

pub fn insert(conn: &Connection, kind: CapitalKind, amount: Cents, on: NaiveDate, notes: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO capital_transactions (kind, amount, occurred_on, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind.key(), amount, Day(on), notes.trim(), now_timestamp()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, kind: CapitalKind, amount: Cents, on: NaiveDate, notes: &str) -> Result<()> {
    conn.execute(
        "UPDATE capital_transactions SET kind = ?1, amount = ?2, occurred_on = ?3, notes = ?4 WHERE id = ?5",
        params![kind.key(), amount, Day(on), notes.trim(), id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM capital_transactions WHERE id = ?1", [id])?;
    Ok(())
}
