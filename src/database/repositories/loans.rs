use std::collections::HashMap;

use anyhow::{Context as _, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::installments::Installment;
use crate::domain::ledger::ContractStatus;
use crate::domain::loans::{Frequency, LoanTerms, Method};
use crate::domain::money::Rate;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loan {
    pub id: i64,
    pub client_id: i64,
    pub terms: LoanTerms,
    /// The installment schedule (empty for interest-only loans).
    pub schedule: Vec<Installment>,
    /// The schedule was changed by hand.
    pub schedule_edited: bool,
    pub status: ContractStatus,
    pub status_date: Option<NaiveDate>,
    pub notes: String,
    pub created_at: String,
}

const COLUMNS: &str = "id, client_id, principal, period_rate, interest_period, frequency, method, installments, schedule_edited, \
                       late_fee_rate, start_date, status, status_date, notes, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Loan> {
    let bad = |what: &str, v: String| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, format!("bad {what} {v:?}").into())
    };
    let freq = |i: usize| -> rusqlite::Result<Frequency> {
        let v: String = r.get(i)?;
        Frequency::from_key(&v).ok_or_else(|| bad("frequency", v))
    };
    let method: String = r.get(6)?;
    let installments: Option<i64> = r.get(7)?;
    let start: Day = r.get(10)?;
    let status: String = r.get(11)?;
    let status_date: Option<Day> = r.get(12)?;
    Ok(Loan {
        id: r.get(0)?,
        client_id: r.get(1)?,
        terms: LoanTerms {
            principal: r.get(2)?,
            rate: Rate(r.get(3)?),
            interest_period: freq(4)?,
            frequency: freq(5)?,
            method: Method::from_key(&method).ok_or_else(|| bad("method", method.clone()))?,
            installments: installments.map(|n| n as u32),
            late_fee_rate: Rate(r.get(9)?),
            start_date: start.0,
        },
        schedule: Vec::new(),
        schedule_edited: r.get::<_, i64>(8)? != 0,
        status: ContractStatus::from_key(&status).ok_or_else(|| bad("status", status.clone()))?,
        status_date: status_date.map(|d| d.0),
        notes: r.get(13)?,
        created_at: r.get(14)?,
    })
}

fn schedules(conn: &Connection, loan_id: Option<i64>) -> Result<HashMap<i64, Vec<Installment>>> {
    let mut stmt = conn.prepare(
        "SELECT loan_id, seq, due_date, principal, interest FROM loan_installments
         WHERE ?1 IS NULL OR loan_id = ?1 ORDER BY loan_id, seq",
    )?;
    let mut out: HashMap<i64, Vec<Installment>> = HashMap::new();
    let rows = stmt.query_map([loan_id], |r| {
        let due: Day = r.get(2)?;
        Ok((r.get::<_, i64>(0)?, Installment { seq: r.get::<_, i64>(1)? as u32, due: due.0, principal: r.get(3)?, interest: r.get(4)? }))
    })?;
    for row in rows {
        let (id, x) = row?;
        out.entry(id).or_default().push(x);
    }
    Ok(out)
}

pub fn list(conn: &Connection) -> Result<Vec<Loan>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM loans ORDER BY start_date, id"))?;
    let mut rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut all = schedules(conn, None)?;
    for l in &mut rows {
        l.schedule = all.remove(&l.id).unwrap_or_default();
    }
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Loan>> {
    let Some(mut loan) = conn.query_row(&format!("SELECT {COLUMNS} FROM loans WHERE id = ?1"), [id], from_row).optional()? else {
        return Ok(None);
    };
    loan.schedule = schedules(conn, Some(id))?.remove(&id).unwrap_or_default();
    Ok(Some(loan))
}

fn write_schedule(conn: &Connection, loan_id: i64, schedule: &[Installment]) -> Result<()> {
    conn.execute("DELETE FROM loan_installments WHERE loan_id = ?1", [loan_id])?;
    let mut stmt =
        conn.prepare("INSERT INTO loan_installments (loan_id, seq, due_date, principal, interest) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for x in schedule {
        stmt.execute(params![loan_id, x.seq, Day(x.due), x.principal, x.interest])?;
    }
    Ok(())
}

/// Inserts a loan and its schedule. Call inside a transaction.
pub fn insert(conn: &Connection, client_id: i64, terms: &LoanTerms, schedule: &[Installment], edited: bool, notes: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO loans (client_id, principal, period_rate, interest_period, frequency, method, installments, schedule_edited,
                            late_fee_rate, start_date, status, notes, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'active', ?11, ?12)",
        params![
            client_id,
            terms.principal,
            terms.rate.0,
            terms.interest_period.key(),
            terms.frequency.key(),
            terms.method.key(),
            terms.installments,
            edited as i64,
            terms.late_fee_rate.0,
            Day(terms.start_date),
            notes.trim(),
            now_timestamp()
        ],
    )
    .context("Could not save the loan")?;
    let id = conn.last_insert_rowid();
    write_schedule(conn, id, schedule)?;
    Ok(id)
}

pub fn update(
    conn: &Connection,
    id: i64,
    client_id: i64,
    terms: &LoanTerms,
    schedule: &[Installment],
    edited: bool,
    notes: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE loans SET client_id = ?1, principal = ?2, period_rate = ?3, interest_period = ?4, frequency = ?5, method = ?6,
                          installments = ?7, schedule_edited = ?8, late_fee_rate = ?9, start_date = ?10, notes = ?11
         WHERE id = ?12",
        params![
            client_id,
            terms.principal,
            terms.rate.0,
            terms.interest_period.key(),
            terms.frequency.key(),
            terms.method.key(),
            terms.installments,
            edited as i64,
            terms.late_fee_rate.0,
            Day(terms.start_date),
            notes.trim(),
            id
        ],
    )
    .context("Could not save the loan")?;
    write_schedule(conn, id, schedule)
}

/// Deletes a loan, its schedule, late fees and status history. Delete its
/// payments first.
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM loans WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_status(conn: &Connection, loan_id: i64, status: ContractStatus, on: NaiveDate, note: &str) -> Result<()> {
    conn.execute("UPDATE loans SET status = ?1, status_date = ?2 WHERE id = ?3", params![status.key(), Day(on), loan_id])?;
    conn.execute(
        "INSERT INTO loan_status_events (loan_id, status, occurred_on, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![loan_id, status.key(), Day(on), note.trim(), now_timestamp()],
    )?;
    Ok(())
}
