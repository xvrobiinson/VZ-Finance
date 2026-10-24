use anyhow::{Context as _, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::ledger::{Allocation, PaymentFact};
use crate::domain::money::Cents;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Payment {
    pub id: i64,
    pub loan_id: i64,
    pub paid_on: NaiveDate,
    pub amount: Cents,
    pub principal: Cents,
    pub interest: Cents,
    pub fee: Cents,
    pub notes: String,
    pub created_at: String,
    pub reversed_at: Option<String>,
    pub reversal_reason: Option<String>,
}

impl Payment {
    pub fn is_reversed(&self) -> bool {
        self.reversed_at.is_some()
    }
    pub fn fact(&self) -> PaymentFact {
        PaymentFact { paid_on: self.paid_on, principal: self.principal, interest: self.interest, fee: self.fee }
    }
}

const SELECT: &str =
    "SELECT p.id, p.loan_id, p.paid_on, p.amount, a.principal, a.interest, a.fee, p.notes, p.created_at, p.reversed_at, p.reversal_reason
     FROM payments p JOIN payment_allocations a ON a.payment_id = p.id";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Payment> {
    let paid_on: Day = r.get(2)?;
    Ok(Payment {
        id: r.get(0)?,
        loan_id: r.get(1)?,
        paid_on: paid_on.0,
        amount: r.get(3)?,
        principal: r.get(4)?,
        interest: r.get(5)?,
        fee: r.get(6)?,
        notes: r.get(7)?,
        created_at: r.get(8)?,
        reversed_at: r.get(9)?,
        reversal_reason: r.get(10)?,
    })
}

/// All payments, including reversed ones, in ledger order.
pub fn list(conn: &Connection) -> Result<Vec<Payment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY p.paid_on, p.id"))?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn for_loan(conn: &Connection, loan_id: i64) -> Result<Vec<Payment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE p.loan_id = ?1 ORDER BY p.paid_on, p.id"))?;
    let rows = stmt.query_map([loan_id], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Payment>> {
    Ok(conn.query_row(&format!("{SELECT} WHERE p.id = ?1"), [id], from_row).optional()?)
}

/// Inserts a payment and its allocation. Call inside a transaction; the
/// database rejects an allocation that does not add up to the amount.
pub fn insert(conn: &Connection, loan_id: i64, paid_on: NaiveDate, amount: Cents, alloc: &Allocation, notes: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO payments (loan_id, paid_on, amount, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![loan_id, Day(paid_on), amount, notes.trim(), now_timestamp()],
    )
    .context("Could not save the payment")?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO payment_allocations (payment_id, principal, interest, fee) VALUES (?1, ?2, ?3, ?4)",
        params![id, alloc.principal, alloc.interest, alloc.fee],
    )
    .context("Could not save the payment allocation")?;
    Ok(id)
}

/// Changes what was received. The allocation is set to fee + capital as a
/// placeholder that adds up; the caller recalculates the loan's splits.
pub fn update(conn: &Connection, id: i64, paid_on: NaiveDate, amount: Cents, fee: Cents, notes: &str) -> Result<()> {
    conn.execute(
        "UPDATE payments SET paid_on = ?1, amount = ?2, notes = ?3 WHERE id = ?4",
        params![Day(paid_on), amount, notes.trim(), id],
    )
    .context("Could not save the payment")?;
    conn.execute(
        "UPDATE payment_allocations SET fee = ?1, interest = 0, principal = ?2 WHERE payment_id = ?3",
        params![fee, amount - fee, id],
    )?;
    Ok(())
}

pub fn set_allocation(conn: &Connection, id: i64, alloc: &Allocation) -> Result<()> {
    conn.execute(
        "UPDATE payment_allocations SET principal = ?1, interest = ?2, fee = ?3 WHERE payment_id = ?4",
        params![alloc.principal, alloc.interest, alloc.fee, id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM payments WHERE id = ?1", [id])?;
    Ok(())
}

pub fn delete_for_loan(conn: &Connection, loan_id: i64) -> Result<()> {
    conn.execute("DELETE FROM payments WHERE loan_id = ?1", [loan_id])?;
    Ok(())
}
