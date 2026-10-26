//! The change history: what every edit and deletion changed, and why.

use anyhow::Result;
use rusqlite::{Connection, Row, params};

use crate::database::now_timestamp;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Entity {
    Client,
    Loan,
    Payment,
    Capital,
    Entry,
    /// A late fee, identified by its installment number (`entity_id`) and
    /// `loan_id`.
    LateFee,
}

impl Entity {
    pub fn key(self) -> &'static str {
        match self {
            Entity::Client => "client",
            Entity::Loan => "loan",
            Entity::Payment => "payment",
            Entity::Capital => "capital",
            Entity::Entry => "entry",
            Entity::LateFee => "late_fee",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "client" => Some(Entity::Client),
            "loan" => Some(Entity::Loan),
            "payment" => Some(Entity::Payment),
            "capital" => Some(Entity::Capital),
            "entry" => Some(Entity::Entry),
            "late_fee" => Some(Entity::LateFee),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Update,
    Delete,
}

/// One field's value before and after, as raw stored values (cents, ISO
/// dates, keys). A deletion keeps the old values and leaves `new` empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldChange {
    pub field: String,
    pub old: String,
    pub new: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub id: i64,
    pub entity: Entity,
    pub entity_id: i64,
    /// The loan a payment belongs (or belonged) to.
    pub loan_id: Option<i64>,
    pub action: Action,
    pub fields: Vec<FieldChange>,
    pub reason: String,
    pub created_at: String,
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n").replace('\r', "")
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn encode(fields: &[FieldChange]) -> String {
    fields.iter().map(|f| format!("{}\t{}\t{}", escape(&f.field), escape(&f.old), escape(&f.new))).collect::<Vec<_>>().join("\n")
}

fn decode(text: &str) -> Vec<FieldChange> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut parts = l.splitn(3, '\t').map(unescape);
            FieldChange {
                field: parts.next().unwrap_or_default(),
                old: parts.next().unwrap_or_default(),
                new: parts.next().unwrap_or_default(),
            }
        })
        .collect()
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Change> {
    let entity: String = r.get(1)?;
    let action: String = r.get(4)?;
    let changes: String = r.get(5)?;
    Ok(Change {
        id: r.get(0)?,
        entity: Entity::from_key(&entity).unwrap_or(Entity::Entry),
        entity_id: r.get(2)?,
        loan_id: r.get(3)?,
        action: if action == "delete" { Action::Delete } else { Action::Update },
        fields: decode(&changes),
        reason: r.get(6)?,
        created_at: r.get(7)?,
    })
}

/// Newest first.
pub fn list(conn: &Connection) -> Result<Vec<Change>> {
    let mut stmt =
        conn.prepare("SELECT id, entity, entity_id, loan_id, action, changes, reason, created_at FROM change_log ORDER BY id DESC")?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn insert(
    conn: &Connection,
    entity: Entity,
    entity_id: i64,
    loan_id: Option<i64>,
    action: Action,
    fields: &[FieldChange],
    reason: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO change_log (entity, entity_id, loan_id, action, changes, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            entity.key(),
            entity_id,
            loan_id,
            if action == Action::Delete { "delete" } else { "update" },
            encode(fields),
            reason.trim(),
            now_timestamp()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_values_round_trip() {
        let fields = vec![
            FieldChange { field: "notes".into(), old: "line one\nline\ttwo \\ end".into(), new: String::new() },
            FieldChange { field: "principal".into(), old: "100000".into(), new: "150000".into() },
        ];
        assert_eq!(decode(&encode(&fields)), fields);
    }
}
