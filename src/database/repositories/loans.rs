use anyhow::{Context as _, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::ledger::ContractStatus;
use crate::domain::loans::{Frequency, LoanTerms};
use crate::domain::money::Rate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loan {
    pub id: i64,
    pub client_id: i64,
    pub terms: LoanTerms,
    pub status: ContractStatus,
    pub status_date: Option<NaiveDate>,
    pub notes: String,
    pub created_at: String,
}

const COLUMNS: &str = "id, client_id, principal, period_rate, frequency, start_date, status, status_date, notes, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Loan> {
    let bad = |what: &str, v: String| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, format!("bad {what} {v:?}").into());
    let frequency: String = r.get(4)?;
    let status: String = r.get(6)?;
    let start: Day = r.get(5)?;
    let status_date: Option<Day> = r.get(7)?;
    Ok(Loan {
        id: r.get(0)?,
        client_id: r.get(1)?,
        terms: LoanTerms {
            principal: r.get(2)?,
            rate: Rate(r.get(3)?),
            frequency: Frequency::from_key(&frequency).ok_or_else(|| bad("frequency", frequency.clone()))?,
            start_date: start.0,
        },
        status: ContractStatus::from_key(&status).ok_or_else(|| bad("status", status.clone()))?,
        status_date: status_date.map(|d| d.0),
        notes: r.get(8)?,
        created_at: r.get(9)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Loan>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM loans ORDER BY start_date, id"))?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Loan>> {
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM loans WHERE id = ?1"), [id], from_row).optional()?)
}

/// Inserts a loan. Call inside a transaction.
pub fn insert(conn: &Connection, client_id: i64, terms: &LoanTerms, notes: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO loans (client_id, principal, period_rate, frequency, start_date, status, notes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7)",
        params![client_id, terms.principal, terms.rate.0, terms.frequency.key(), Day(terms.start_date), notes.trim(), now_timestamp()],
    )
    .context("Could not save the loan")?;
    Ok(conn.last_insert_rowid())
}

pub fn set_status(conn: &Connection, loan_id: i64, status: ContractStatus, on: NaiveDate, note: &str) -> Result<()> {
    conn.execute("UPDATE loans SET status = ?1, status_date = ?2 WHERE id = ?3", params![status.key(), Day(on), loan_id])?;
    conn.execute(
        "INSERT INTO loan_status_events (loan_id, status, occurred_on, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![loan_id, status.key(), Day(on), note.trim(), now_timestamp()],
    )?;
    Ok(())
}
