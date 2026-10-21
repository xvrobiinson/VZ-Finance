//! CSV export of every table, in plain units (2-decimal amounts, ISO dates).

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::Snapshot;
use crate::domain::money::{Cents, MoneyFormat};
use crate::utils::dates::to_iso;

fn amount(c: Cents) -> String {
    MoneyFormat::input_value(c)
}

pub fn export_all(snap: &Snapshot, dir: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d");
    let mut written = Vec::new();
    let file = |name: &str| -> Result<(csv::Writer<std::fs::File>, PathBuf)> {
        let path = dir.join(format!("vz-finance-{name}-{stamp}.csv"));
        Ok((csv::Writer::from_path(&path)?, path))
    };

    let (mut w, path) = file("clients")?;
    w.write_record(["id", "full_name", "phone", "email", "notes", "created_at", "archived_at"])?;
    for c in &snap.clients {
        w.write_record([
            c.id.to_string(),
            c.full_name.clone(),
            c.phone.clone().unwrap_or_default(),
            c.email.clone().unwrap_or_default(),
            c.notes.clone(),
            c.created_at.clone(),
            c.archived_at.clone().unwrap_or_default(),
        ])?;
    }
    w.flush()?;
    written.push(path);

    let (mut w, path) = file("loans")?;
    w.write_record([
        "id", "client_id", "client", "principal", "annual_rate_percent", "method", "frequency", "term_count", "start_date", "maturity_date", "status", "notes",
    ])?;
    for l in &snap.loans {
        w.write_record([
            l.id.to_string(),
            l.client_id.to_string(),
            snap.client_name(l.client_id).to_string(),
            amount(l.terms.principal),
            l.terms.rate.percent_label().trim_end_matches('%').to_string(),
            l.terms.method.key().to_string(),
            l.terms.frequency.key().to_string(),
            l.terms.term_count.to_string(),
            to_iso(l.terms.start_date),
            to_iso(l.maturity_date),
            l.status.key().to_string(),
            l.notes.clone(),
        ])?;
    }
    w.flush()?;
    written.push(path);

    let (mut w, path) = file("payments")?;
    w.write_record(["id", "loan_id", "client", "paid_on", "amount", "principal", "interest", "fee", "notes", "reversed_at", "reversal_reason"])?;
    for p in &snap.payments {
        let client = snap.loan(p.loan_id).map(|l| snap.client_name(l.client_id)).unwrap_or_default();
        w.write_record([
            p.id.to_string(),
            p.loan_id.to_string(),
            client.to_string(),
            to_iso(p.paid_on),
            amount(p.amount),
            amount(p.principal),
            amount(p.interest),
            amount(p.fee),
            p.notes.clone(),
            p.reversed_at.clone().unwrap_or_default(),
            p.reversal_reason.clone().unwrap_or_default(),
        ])?;
    }
    w.flush()?;
    written.push(path);

    let (mut w, path) = file("capital")?;
    w.write_record(["id", "kind", "amount", "occurred_on", "notes"])?;
    for c in &snap.capital {
        w.write_record([c.id.to_string(), c.kind.key().to_string(), amount(c.amount), to_iso(c.occurred_on), c.notes.clone()])?;
    }
    w.flush()?;
    written.push(path);

    Ok(written)
}
