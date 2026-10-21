use anyhow::{Context as _, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::ledger::ContractStatus;
use crate::domain::loans::{Frequency, Installment, InterestMethod, LoanTerms};
use crate::domain::money::Rate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loan {
    pub id: i64,
    pub client_id: i64,
    pub terms: LoanTerms,
    pub maturity_date: NaiveDate,
    pub status: ContractStatus,
    pub status_date: Option<NaiveDate>,
    pub notes: String,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEvent {
    pub id: i64,
    pub loan_id: i64,
    pub status: ContractStatus,
    pub occurred_on: NaiveDate,
    pub note: String,
}

const COLUMNS: &str =
    "id, client_id, principal, annual_rate, method, frequency, term_count, start_date, maturity_date, status, status_date, notes, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Loan> {
    let bad = |what: &str, v: String| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, format!("bad {what} {v:?}").into());
    let method: String = r.get(4)?;
    let frequency: String = r.get(5)?;
    let status: String = r.get(9)?;
    let start: Day = r.get(7)?;
    let maturity: Day = r.get(8)?;
    let status_date: Option<Day> = r.get(10)?;
    let term_count: i64 = r.get(6)?;
    Ok(Loan {
        id: r.get(0)?,
        client_id: r.get(1)?,
        terms: LoanTerms {
            principal: r.get(2)?,
            rate: Rate(r.get(3)?),
            method: InterestMethod::from_key(&method).ok_or_else(|| bad("method", method.clone()))?,
            frequency: Frequency::from_key(&frequency).ok_or_else(|| bad("frequency", frequency.clone()))?,
            term_count: term_count as u32,
            start_date: start.0,
        },
        maturity_date: maturity.0,
        status: ContractStatus::from_key(&status).ok_or_else(|| bad("status", status.clone()))?,
        status_date: status_date.map(|d| d.0),
        notes: r.get(11)?,
        created_at: r.get(12)?,
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

/// Inserts the loan and its schedule. Call inside a transaction.
pub fn insert(conn: &Connection, client_id: i64, terms: &LoanTerms, schedule: &[Installment], notes: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO loans (client_id, principal, annual_rate, method, frequency, term_count, start_date, maturity_date, status, notes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?10)",
        params![
            client_id,
            terms.principal,
            terms.rate.0,
            terms.method.key(),
            terms.frequency.key(),
            terms.term_count,
            Day(terms.start_date),
            Day(terms.maturity_date()),
            notes.trim(),
            now_timestamp(),
        ],
    )
    .context("Could not save the loan")?;
    let loan_id = conn.last_insert_rowid();
    let mut stmt = conn.prepare("INSERT INTO loan_schedules (loan_id, seq, due_date, principal, interest) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for row in schedule {
        stmt.execute(params![loan_id, row.seq, Day(row.due_date), row.principal, row.interest])?;
    }
    Ok(loan_id)
}

pub fn set_status(conn: &Connection, loan_id: i64, status: ContractStatus, on: NaiveDate, note: &str) -> Result<()> {
    conn.execute("UPDATE loans SET status = ?1, status_date = ?2 WHERE id = ?3", params![status.key(), Day(on), loan_id])?;
    conn.execute(
        "INSERT INTO loan_status_events (loan_id, status, occurred_on, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![loan_id, status.key(), Day(on), note.trim(), now_timestamp()],
    )?;
    Ok(())
}

pub fn update_notes(conn: &Connection, loan_id: i64, notes: &str) -> Result<()> {
    conn.execute("UPDATE loans SET notes = ?1 WHERE id = ?2", params![notes.trim(), loan_id])?;
    Ok(())
}

/// Every schedule row, grouped by loan id in seq order.
pub fn all_schedules(conn: &Connection) -> Result<Vec<(i64, Installment)>> {
    let mut stmt = conn.prepare("SELECT loan_id, seq, due_date, principal, interest FROM loan_schedules ORDER BY loan_id, seq")?;
    let rows = stmt
        .query_map([], |r| {
            let due: Day = r.get(2)?;
            let seq: i64 = r.get(1)?;
            Ok((r.get(0)?, Installment { seq: seq as u32, due_date: due.0, principal: r.get(3)?, interest: r.get(4)? }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn status_events(conn: &Connection, loan_id: i64) -> Result<Vec<StatusEvent>> {
    let mut stmt =
        conn.prepare("SELECT id, loan_id, status, occurred_on, note FROM loan_status_events WHERE loan_id = ?1 ORDER BY id")?;
    let rows = stmt
        .query_map([loan_id], |r| {
            let status: String = r.get(2)?;
            let on: Day = r.get(3)?;
            Ok(StatusEvent {
                id: r.get(0)?,
                loan_id: r.get(1)?,
                status: ContractStatus::from_key(&status).unwrap_or(ContractStatus::Active),
                occurred_on: on.0,
                note: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
