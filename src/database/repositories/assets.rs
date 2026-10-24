//! Small files stored with the data, such as the business logo.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

use crate::database::now_timestamp;

pub const LOGO: &str = "logo";

pub fn get(conn: &Connection, key: &str) -> Result<Option<Vec<u8>>> {
    Ok(conn.query_row("SELECT data FROM app_assets WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

pub fn set(conn: &Connection, key: &str, mime: &str, data: &[u8]) -> Result<()> {
    conn.execute(
        "INSERT INTO app_assets (key, mime, data, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (key) DO UPDATE SET mime = excluded.mime, data = excluded.data, updated_at = excluded.updated_at",
        params![key, mime, data, now_timestamp()],
    )?;
    Ok(())
}

pub fn remove(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM app_assets WHERE key = ?1", [key])?;
    Ok(())
}
