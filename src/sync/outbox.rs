//! Changes saved on this PC that the cloud hasn't accepted yet, sent oldest
//! first (`outbox` table, migration 0007).
//!
//! Outcomes, as in the PWA's outbox:
//! - accepted: the entry is removed; the next copy from the cloud has it.
//! - no connection: it stays and is sent again later. Only the kinds the
//!   cloud records once however often they arrive (`change::OFFLINE`, with
//!   an `opId`) are sent again; any other change whose answer was lost is
//!   put aside for review, so it can never be made twice.
//! - refused by a rule: put aside with the reason, for a person to review.
//!   It no longer counts in the books.
//!
//! Nothing is dropped without a person deciding.

use std::hash::{BuildHasher, Hasher};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::change::{Change, OFFLINE, Op};
use crate::database::now_timestamp;

/// A change waiting to be sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    pub op_id: String,
    pub op: Op,
}

/// A change the cloud refused or that may not have arrived, kept for review.
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    pub op_id: String,
    pub op: Op,
    pub at: String,
    /// The cloud's answer (`{ "code": ... }`), or `{ "code": "unknown" }`
    /// when the answer was lost.
    pub error: Value,
}

/// What the cloud answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Accepted(Value),
    Refused(Value),
    /// No answer: offline, or the connection dropped while waiting.
    NoAnswer,
    /// Not sent: there was no connection. It waits, whatever its kind.
    NotSent,
}

/// A new operation id, unique across devices (time plus a random number
/// from the system).
pub fn new_op_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(nanos);
    format!("desktop-{nanos:x}-{:016x}", h.finish())
}

fn mutation_name(name: &str) -> &'static str {
    // Ops are only ever built by `Change::op`; this maps the stored name back
    // to the same static string.
    const ALL: &[&str] = &[
        "clients:create",
        "clients:update",
        "clients:setArchived",
        "clients:remove",
        "loans:create",
        "loans:update",
        "loans:remove",
        "loans:markDefaulted",
        "loans:reinstate",
        "loans:setLateFeeWaived",
        "payments:record",
        "payments:update",
        "payments:remove",
        "money:addCapital",
        "money:updateCapital",
        "money:removeCapital",
        "money:addEntry",
        "money:adjustBalances",
        "money:updateEntry",
        "money:removeEntry",
        "settings:update",
        "clients:setPhoto",
        "clients:removePhoto",
        "settings:setLogo",
    ];
    ALL.iter().find(|m| **m == name).copied().unwrap_or("unknown")
}

fn read_op(kind: String, args: String) -> rusqlite::Result<Op> {
    Ok(Op { mutation: mutation_name(&kind), args: serde_json::from_str(&args).unwrap_or(Value::Null) })
}

/// Queues `op`. The offline kinds get their `opId` here.
pub fn push(conn: &Connection, mut op: Op) -> Result<Queued> {
    let op_id = new_op_id();
    if op.works_offline() {
        op.args["opId"] = Value::String(op_id.clone());
    }
    conn.execute(
        "INSERT INTO outbox (op_id, kind, args, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![op_id, op.mutation, op.args.to_string(), now_timestamp()],
    )?;
    Ok(Queued { op_id, op })
}

/// The changes still to send, oldest first.
pub fn waiting(conn: &Connection) -> Result<Vec<Queued>> {
    let mut stmt = conn.prepare("SELECT op_id, kind, args FROM outbox WHERE refused_at IS NULL ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok(Queued { op_id: r.get(0)?, op: read_op(r.get(1)?, r.get(2)?)? }))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The changes put aside for review, oldest first.
pub fn refused(conn: &Connection) -> Result<Vec<Refused>> {
    let mut stmt = conn.prepare("SELECT op_id, kind, args, refused_at, error FROM outbox WHERE refused_at IS NOT NULL ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Refused {
            op_id: r.get(0)?,
            op: read_op(r.get(1)?, r.get(2)?)?,
            at: r.get(3)?,
            error: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Value::Null),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Records what the cloud answered for `q`. Returns whether the outbox
/// changed (and so the books on screen should be worked out again).
pub fn finish(conn: &Connection, q: &Queued, outcome: &Outcome) -> Result<bool> {
    match outcome {
        Outcome::Accepted(result) => {
            conn.execute("DELETE FROM outbox WHERE op_id = ?1", [&q.op_id])?;
            if q.op.mutation == "payments:record" {
                rebase_loan_version(conn, q, result)?;
            }
            Ok(true)
        }
        Outcome::Refused(error) => put_aside(conn, &q.op_id, error),
        Outcome::NoAnswer if !OFFLINE.contains(&q.op.mutation) => put_aside(conn, &q.op_id, &serde_json::json!({ "code": "unknown" })),
        Outcome::NoAnswer | Outcome::NotSent => Ok(false),
    }
}

fn put_aside(conn: &Connection, op_id: &str, error: &Value) -> Result<bool> {
    conn.execute("UPDATE outbox SET refused_at = ?1, error = ?2 WHERE op_id = ?3", params![now_timestamp(), error.to_string(), op_id])?;
    Ok(true)
}

/// Payments queued on the same loan from the same copy of it saw the loan's
/// version before this one; this payment doesn't make them stale.
fn rebase_loan_version(conn: &Connection, q: &Queued, result: &Value) -> Result<()> {
    let (Some(before), Some(after)) = (result["loanVersionBefore"].as_i64(), result["loanVersionAfter"].as_i64()) else { return Ok(()) };
    for later in waiting(conn)? {
        if later.op.mutation == "payments:record"
            && later.op.args["loanId"] == q.op.args["loanId"]
            && later.op.args["loanVersion"] == before
        {
            let mut args = later.op.args;
            args["loanVersion"] = after.into();
            conn.execute("UPDATE outbox SET args = ?1 WHERE op_id = ?2", params![args.to_string(), later.op_id])?;
        }
    }
    Ok(())
}

/// Sends a refused change again, as it was or with new arguments (same
/// operation id: it was never recorded).
pub fn retry(conn: &Connection, op_id: &str, args: Option<&Value>) -> Result<()> {
    let current: Option<String> = conn.query_row("SELECT args FROM outbox WHERE op_id = ?1", [op_id], |r| r.get(0)).optional()?;
    let Some(current) = current else { return Ok(()) };
    let args = args.map(Value::to_string).unwrap_or(current);
    conn.execute("UPDATE outbox SET refused_at = NULL, error = '', args = ?1 WHERE op_id = ?2", params![args, op_id])?;
    Ok(())
}

/// Removes a refused change for good, after a person decided to.
pub fn discard(conn: &Connection, op_id: &str) -> Result<()> {
    conn.execute("DELETE FROM outbox WHERE op_id = ?1 AND refused_at IS NOT NULL", [op_id])?;
    Ok(())
}

/// The waiting changes that can be shown on top of a copy from the cloud.
pub(crate) fn replayable(conn: &Connection) -> Result<Vec<Change>> {
    let mut out = Vec::new();
    for q in waiting(conn)?.into_iter().filter(|q| q.op.works_offline()) {
        match Change::from_op(conn, &q.op) {
            Ok(c) => out.push(c),
            Err(e) => log::info!("waiting change not shown: {e}"),
        }
    }
    Ok(out)
}
