use std::collections::HashMap;

use anyhow::{Context as _, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::Day;
use crate::database::now_timestamp;
use crate::domain::installments::{Applied, Instruction};
use crate::domain::ledger::{Allocation, PaymentFact};
use crate::domain::money::Cents;

/// How a payment on an installment loan was asked to be applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AllocationMode {
    /// Oldest first.
    #[default]
    Auto,
    /// To one installment.
    Installment,
    /// Chosen amounts per installment.
    Spread,
    /// Chosen capital, interest and late-fee totals.
    Custom,
}

impl AllocationMode {
    pub fn key(self) -> &'static str {
        match self {
            AllocationMode::Auto => "auto",
            AllocationMode::Installment => "installment",
            AllocationMode::Spread => "spread",
            AllocationMode::Custom => "custom",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "installment" => AllocationMode::Installment,
            "spread" => AllocationMode::Spread,
            "custom" => AllocationMode::Custom,
            _ => AllocationMode::Auto,
        }
    }
}

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
    pub mode: AllocationMode,
    pub target_seq: Option<u32>,
    /// Chosen amounts per installment (spread payments).
    pub targets: Vec<(u32, Cents)>,
    /// How it was split per installment (installment loans).
    pub applied: Vec<Applied>,
}

impl Payment {
    pub fn is_reversed(&self) -> bool {
        self.reversed_at.is_some()
    }
    pub fn fact(&self) -> PaymentFact {
        PaymentFact { paid_on: self.paid_on, principal: self.principal, interest: self.interest, fee: self.fee }
    }
    /// How it should be applied when the loan's splits are worked out again.
    pub fn instruction(&self) -> Instruction {
        match self.mode {
            AllocationMode::Auto => Instruction::Auto,
            AllocationMode::Installment => self.target_seq.map(Instruction::Installment).unwrap_or(Instruction::Auto),
            AllocationMode::Spread => Instruction::Spread(self.targets.clone()),
            AllocationMode::Custom => Instruction::Custom { principal: self.principal, interest: self.interest, fee: self.fee },
        }
    }
}

const SELECT: &str = "SELECT p.id, p.loan_id, p.paid_on, p.amount, a.principal, a.interest, a.fee, p.notes, p.created_at, \
                      p.reversed_at, p.reversal_reason, p.allocation_mode, p.target_seq
                      FROM payments p JOIN payment_allocations a ON a.payment_id = p.id";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Payment> {
    let paid_on: Day = r.get(2)?;
    let mode: String = r.get(11)?;
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
        mode: AllocationMode::from_key(&mode),
        target_seq: r.get::<_, Option<i64>>(12)?.map(|s| s as u32),
        targets: Vec::new(),
        applied: Vec::new(),
    })
}

/// Fills in targets and per-installment splits.
fn attach(conn: &Connection, rows: &mut [Payment]) -> Result<()> {
    let mut targets: HashMap<i64, Vec<(u32, Cents)>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT payment_id, seq, amount FROM payment_targets ORDER BY payment_id, seq")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u32, r.get::<_, Cents>(2)?)))? {
        let (id, seq, amount) = row?;
        targets.entry(id).or_default().push((seq, amount));
    }
    let mut applied: HashMap<i64, Vec<Applied>> = HashMap::new();
    let mut stmt =
        conn.prepare("SELECT payment_id, seq, principal, interest, late_fee FROM installment_allocations ORDER BY payment_id, seq")?;
    for row in stmt.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, Applied { seq: r.get::<_, i64>(1)? as u32, principal: r.get(2)?, interest: r.get(3)?, fee: r.get(4)? }))
    })? {
        let (id, a) = row?;
        applied.entry(id).or_default().push(a);
    }
    for p in rows {
        p.targets = targets.remove(&p.id).unwrap_or_default();
        p.applied = applied.remove(&p.id).unwrap_or_default();
    }
    Ok(())
}

/// All payments, including reversed ones, in ledger order.
pub fn list(conn: &Connection) -> Result<Vec<Payment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY p.paid_on, p.id"))?;
    let mut rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    attach(conn, &mut rows)?;
    Ok(rows)
}

pub fn for_loan(conn: &Connection, loan_id: i64) -> Result<Vec<Payment>> {
    let mut stmt = conn.prepare(&format!("{SELECT} WHERE p.loan_id = ?1 ORDER BY p.paid_on, p.id"))?;
    let mut rows = stmt.query_map([loan_id], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    attach(conn, &mut rows)?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Payment>> {
    let Some(p) = conn.query_row(&format!("{SELECT} WHERE p.id = ?1"), [id], from_row).optional()? else {
        return Ok(None);
    };
    let mut rows = [p];
    attach(conn, &mut rows)?;
    let [p] = rows;
    Ok(Some(p))
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

/// Stores how a payment on an installment loan should be applied. A custom
/// split's totals become its allocation.
pub fn set_instruction(conn: &Connection, id: i64, instruction: &Instruction) -> Result<()> {
    let (mode, target) = match instruction {
        Instruction::Installment(seq) => (AllocationMode::Installment, Some(*seq)),
        Instruction::Spread(_) => (AllocationMode::Spread, None),
        Instruction::Custom { .. } => (AllocationMode::Custom, None),
        Instruction::Auto | Instruction::Fixed(_) => (AllocationMode::Auto, None),
    };
    conn.execute("UPDATE payments SET allocation_mode = ?1, target_seq = ?2 WHERE id = ?3", params![mode.key(), target, id])?;
    conn.execute("DELETE FROM payment_targets WHERE payment_id = ?1", [id])?;
    if let Instruction::Spread(parts) = instruction {
        let mut stmt = conn.prepare("INSERT INTO payment_targets (payment_id, seq, amount) VALUES (?1, ?2, ?3)")?;
        for (seq, amount) in parts.iter().filter(|(_, a)| *a > 0) {
            stmt.execute(params![id, seq, amount])?;
        }
    }
    if let Instruction::Custom { principal, interest, fee } = instruction {
        conn.execute(
            "UPDATE payment_allocations SET principal = ?1, interest = ?2, fee = ?3 WHERE payment_id = ?4",
            params![principal, interest, fee, id],
        )?;
    }
    Ok(())
}

pub fn set_allocation(conn: &Connection, id: i64, alloc: &Allocation) -> Result<()> {
    conn.execute(
        "UPDATE payment_allocations SET principal = ?1, interest = ?2, fee = ?3 WHERE payment_id = ?4",
        params![alloc.principal, alloc.interest, alloc.fee, id],
    )?;
    Ok(())
}

/// Stores a payment's split per installment and its totals.
pub fn set_split(conn: &Connection, id: i64, applied: &[Applied]) -> Result<()> {
    let total = |f: fn(&Applied) -> Cents| applied.iter().map(f).sum::<Cents>();
    conn.execute(
        "UPDATE payment_allocations SET principal = ?1, interest = ?2, fee = ?3 WHERE payment_id = ?4",
        params![total(|a| a.principal), total(|a| a.interest), total(|a| a.fee), id],
    )?;
    conn.execute("DELETE FROM installment_allocations WHERE payment_id = ?1", [id])?;
    let mut stmt =
        conn.prepare("INSERT INTO installment_allocations (payment_id, seq, principal, interest, late_fee) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for a in applied.iter().filter(|a| a.total() > 0) {
        stmt.execute(params![id, a.seq, a.principal, a.interest, a.fee])?;
    }
    Ok(())
}

/// Removes per-installment splits (when a loan becomes interest only).
pub fn clear_split(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM installment_allocations WHERE payment_id = ?1", [id])?;
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
