use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::opt_text;
use crate::database::now_timestamp;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Client {
    pub id: i64,
    pub full_name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: String,
    pub created_at: String,
    pub archived_at: Option<String>,
}

impl Client {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ClientInput {
    pub full_name: String,
    pub phone: String,
    pub email: String,
    pub notes: String,
}

const COLUMNS: &str = "id, full_name, phone, email, notes, created_at, archived_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Client> {
    Ok(Client {
        id: r.get(0)?,
        full_name: r.get(1)?,
        phone: r.get(2)?,
        email: r.get(3)?,
        notes: r.get(4)?,
        created_at: r.get(5)?,
        archived_at: r.get(6)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<Client>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM clients ORDER BY full_name COLLATE NOCASE, id"))?;
    let rows = stmt.query_map([], from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Client>> {
    Ok(conn.query_row(&format!("SELECT {COLUMNS} FROM clients WHERE id = ?1"), [id], from_row).optional()?)
}

pub fn insert(conn: &Connection, input: &ClientInput) -> Result<i64> {
    conn.execute(
        "INSERT INTO clients (full_name, phone, email, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![input.full_name.trim(), opt_text(&input.phone), opt_text(&input.email), input.notes.trim(), now_timestamp()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &ClientInput) -> Result<()> {
    conn.execute(
        "UPDATE clients SET full_name = ?1, phone = ?2, email = ?3, notes = ?4 WHERE id = ?5",
        params![input.full_name.trim(), opt_text(&input.phone), opt_text(&input.email), input.notes.trim(), id],
    )?;
    Ok(())
}

pub fn set_archived(conn: &Connection, id: i64, archived: bool) -> Result<()> {
    let value = archived.then(now_timestamp);
    conn.execute("UPDATE clients SET archived_at = ?1 WHERE id = ?2", params![value, id])?;
    Ok(())
}
