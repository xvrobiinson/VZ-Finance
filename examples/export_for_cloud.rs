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

use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};
use vz_finance::services::Store;
use vz_finance::utils::dates::today;

/// `"a": 1, "b": 2` for the named fields of `$s`, keys in camelCase.
macro_rules! fields {
    ($out:expr, $s:expr; $($f:ident),+) => {{
        let mut parts = Vec::new();
        $( parts.push(format!("\"{}\": {}", camel(stringify!($f)), $s.$f)); )+
        $out.push_str(&parts.join(", "));
    }};
}

fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

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
    let sum = snap.book.summary(t);

    let mut json = String::new();
    write!(json, "{{\n  \"asOf\": \"{t}\",\n  \"summary\": {{")?;
    fields!(json, sum;
        contributions, withdrawals, principal_disbursed, principal_repaid, interest_collected, fees_collected,
        capital_expenses, interest_expenses, capital_income, other_income, capital_adjustments,
        interest_adjustments, available_capital, available_interest, available_cash, outstanding_principal,
        interest_receivable, fees_receivable, defaulted_principal, defaulted_interest, portfolio_value,
        active_loans, overdue_loans, due_soon_loans, overdue_amount, expected_monthly_interest);
    json.push_str("},\n  \"loans\": [");
    for (i, loan) in snap.book.loans.iter().enumerate() {
        let pos = loan.position(t);
        let late = loan.delinquency(t);
        if i > 0 {
            json.push(',');
        }
        write!(json, "\n    {{\"no\": {}, ", loan.id)?;
        fields!(json, pos;
            principal_outstanding, interest_outstanding, interest_charged_total, interest_paid, principal_paid,
            fees_paid, fees_outstanding, paid_total);
        json.push_str(", ");
        fields!(json, late; overdue_amount, days_overdue);
        json.push('}');
    }
    json.push_str("\n  ]\n}\n");
    std::fs::write(out_dir.join("figures.json"), json)?;

    println!(
        "Exported {} clients, {} loans, {} payments to {} (figures as of {t})",
        snap.clients.len(),
        snap.loans.len(),
        snap.payments.len(),
        out_dir.display()
    );
    Ok(())
}
