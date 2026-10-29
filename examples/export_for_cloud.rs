//! Prepares a desktop database for the cloud import (the PWA's
//! `scripts/import-desktop.mjs`, docs/DESKTOP-SYNC-PLAN.md phase 2).
//!
//! Usage: cargo run --example export_for_cloud -- <source.db> <out-dir>
//!
//! The source is opened read-only and copied with SQLite's backup API, so a
//! database in use is copied consistently and never changed. The copy
//! (`<out-dir>/book.db`) is brought up to the current schema and its late
//! fees and paid-off status are brought up to today, as the app does on
//! start. `<out-dir>/figures.json` then holds the desktop engine's figures
//! as of today: the portfolio summary and each loan's position, which the
//! importer compares with the cloud's.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};
use vz_finance::services::Store;
use vz_finance::sync::figures::figures;
use vz_finance::utils::dates::today;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(source), Some(out_dir)) = (args.next(), args.next()) else {
        bail!("usage: export_for_cloud <source.db> <out-dir>");
    };
    let out_dir = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir)?;
    let book = out_dir.join("book.db");
    if book.exists() {
        bail!("{} already exists; use an empty folder", book.display());
    }

    let src = Connection::open_with_flags(&source, OpenFlags::SQLITE_OPEN_READ_ONLY).with_context(|| format!("opening {source}"))?;
    let mut dst = Connection::open(&book)?;
    rusqlite::backup::Backup::new(&src, &mut dst)?.run_to_completion(256, std::time::Duration::ZERO, None)?;
    drop((src, dst));

    let t = today();
    let mut s = Store::open(book.clone())?;
    s.refresh(t)?;
    let snap = s.snapshot()?;
    let json = serde_json::to_string_pretty(&figures(&snap.book, t))?;
    std::fs::write(out_dir.join("figures.json"), json + "\n")?;
    println!(
        "Exported {} clients, {} loans, {} payments to {} (figures as of {t})",
        snap.clients.len(),
        snap.loans.len(),
        snap.payments.len(),
        out_dir.display()
    );
    Ok(())
}
