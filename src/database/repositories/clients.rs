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
    /// National ID, passport or other document number.
    pub id_number: Option<String>,
    pub address: Option<String>,
    pub notes: String,
    pub created_at: String,
    pub archived_at: Option<String>,
    /// A photo is on file. The picture itself is loaded only when shown.
    pub has_photo: bool,
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
    pub id_number: String,
    pub address: String,
    pub notes: String,
}

const COLUMNS: &str = "id, full_name, phone, email, id_number, address, notes, created_at, archived_at,
    EXISTS (SELECT 1 FROM client_photos p WHERE p.client_id = clients.id)";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Client> {
    Ok(Client {
        id: r.get(0)?,
        full_name: r.get(1)?,
        phone: r.get(2)?,
        email: r.get(3)?,
        id_number: r.get(4)?,
        address: r.get(5)?,
        notes: r.get(6)?,
        created_at: r.get(7)?,
        archived_at: r.get(8)?,
        has_photo: r.get(9)?,
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
        "INSERT INTO clients (full_name, phone, email, id_number, address, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            input.full_name.trim(),
            opt_text(&input.phone),
            opt_text(&input.email),
            opt_text(&input.id_number),
            opt_text(&input.address),
            input.notes.trim(),
            now_timestamp()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: &ClientInput) -> Result<()> {
    conn.execute(
        "UPDATE clients SET full_name = ?1, phone = ?2, email = ?3, id_number = ?4, address = ?5, notes = ?6 WHERE id = ?7",
        params![
            input.full_name.trim(),
            opt_text(&input.phone),
            opt_text(&input.email),
            opt_text(&input.id_number),
            opt_text(&input.address),
            input.notes.trim(),
            id
        ],
    )?;
    Ok(())
}

/// Deletes a client with no loans (loans keep their client). Their photo
/// goes with them.
pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM clients WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_archived(conn: &Connection, id: i64, archived: bool) -> Result<()> {
    let value = archived.then(now_timestamp);
    conn.execute("UPDATE clients SET archived_at = ?1 WHERE id = ?2", params![value, id])?;
    Ok(())
}

/// The client's photo as stored (JPEG), if any.
pub fn photo(conn: &Connection, id: i64) -> Result<Option<Vec<u8>>> {
    Ok(conn.query_row("SELECT data FROM client_photos WHERE client_id = ?1", [id], |r| r.get(0)).optional()?)
}

pub fn set_photo(conn: &Connection, id: i64, jpeg: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO client_photos (client_id, mime, data, updated_at) VALUES (?1, 'image/jpeg', ?2, ?3)
         ON CONFLICT (client_id) DO UPDATE SET mime = excluded.mime, data = excluded.data, updated_at = excluded.updated_at",
        params![id, jpeg, now_timestamp()],
    )?;
    Ok(())
}

pub fn remove_photo(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM client_photos WHERE client_id = ?1", [id])?;
    Ok(())
}
