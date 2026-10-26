//! Late fees assessed on overdue installments: one row per installment,
//! kept up to date with the loan's payments.

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{Connection, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::money::{Cents, Rate};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LateFee {
    pub loan_id: i64,
    pub seq: u32,
    pub assessed_on: NaiveDate,
    /// What was unpaid on the installment when the fee was assessed.
    pub base_amount: Cents,
    pub rate: Rate,
    pub amount: Cents,
    pub waived_at: Option<String>,
    pub waive_reason: String,
    pub created_at: String,
    pub updated_at: String,
}

impl LateFee {
    pub fn is_waived(&self) -> bool {
        self.waived_at.is_some()
    }
}

pub fn list(conn: &Connection) -> Result<Vec<LateFee>> {
    let mut stmt = conn.prepare(
        "SELECT loan_id, seq, assessed_on, base_amount, rate, amount, waived_at, waive_reason, created_at, updated_at
         FROM late_fees ORDER BY loan_id, seq",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let on: Day = r.get(2)?;
            Ok(LateFee {
                loan_id: r.get(0)?,
                seq: r.get::<_, i64>(1)? as u32,
                assessed_on: on.0,
                base_amount: r.get(3)?,
                rate: Rate(r.get(4)?),
                amount: r.get(5)?,
                waived_at: r.get(6)?,
                waive_reason: r.get(7)?,
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn for_loan(conn: &Connection, loan_id: i64) -> Result<Vec<LateFee>> {
    Ok(list(conn)?.into_iter().filter(|f| f.loan_id == loan_id).collect())
}

/// Records an assessment, or brings an existing one up to date. The
/// installment is the key, so the same fee is never added twice.
pub fn upsert(conn: &Connection, loan_id: i64, seq: u32, assessed_on: NaiveDate, base: Cents, rate: Rate, amount: Cents) -> Result<()> {
    let now = now_timestamp();
    conn.execute(
        "INSERT INTO late_fees (loan_id, seq, assessed_on, base_amount, rate, amount, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT (loan_id, seq) DO UPDATE SET assessed_on = excluded.assessed_on, base_amount = excluded.base_amount,
             rate = excluded.rate, amount = excluded.amount, updated_at = excluded.updated_at",
        params![loan_id, seq, Day(assessed_on), base, rate.0, amount, now],
    )?;
    Ok(())
}

pub fn remove(conn: &Connection, loan_id: i64, seq: u32) -> Result<()> {
    conn.execute("DELETE FROM late_fees WHERE loan_id = ?1 AND seq = ?2", params![loan_id, seq])?;
    Ok(())
}

pub fn set_waived(conn: &Connection, loan_id: i64, seq: u32, waived: bool, reason: &str) -> Result<()> {
    let at = waived.then(now_timestamp);
    conn.execute(
        "UPDATE late_fees SET waived_at = ?1, waive_reason = ?2, updated_at = ?3 WHERE loan_id = ?4 AND seq = ?5",
        params![at, if waived { reason.trim() } else { "" }, now_timestamp(), loan_id, seq],
    )?;
    Ok(())
}
