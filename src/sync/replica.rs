//! The linked database as a copy of the cloud's books.
//!
//! A linked business gets its own database file (`path_for`), so the
//! books kept on this PC before linking are never touched. Each snapshot
//! from the cloud replaces that file's records in one transaction: a record's
//! id is its record number in the cloud (unique per table, like the local
//! ids), so screens, receipts and record numbers work as they do today.
//! `remote_ids` maps each row back to its cloud id and version.
//!
//! Kept across snapshots: client photos (until the client has none in the
//! cloud), this PC's own preferences (language, theme, charts, sidebar,
//! backups), the change history and the outbox.

use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::wire::{Role, Snapshot};
use crate::database::now_timestamp;

/// Kinds of records with a cloud id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entity {
    Client,
    Loan,
    Payment,
    LateFee,
    Capital,
    Entry,
}

impl Entity {
    fn key(self) -> &'static str {
        match self {
            Entity::Client => "client",
            Entity::Loan => "loan",
            Entity::Payment => "payment",
            Entity::LateFee => "late_fee",
            Entity::Capital => "capital",
            Entity::Entry => "entry",
        }
    }
}

fn role_key(r: Role) -> &'static str {
    match r {
        Role::Admin => "admin",
        Role::Collector => "collector",
        Role::Viewer => "viewer",
    }
}

/// The linked business this database follows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub business_id: String,
    pub business_name: String,
    pub role: Role,
    /// "Today" in the business's time zone at the last snapshot.
    pub today: chrono::NaiveDate,
}

/// `%LOCALAPPDATA%\VZFinance\cloud\<business id>.db`, next to the local
/// books' `data` folder.
pub fn path_for(business_id: &str) -> Result<PathBuf> {
    if business_id.is_empty() || business_id.len() > 64 || !business_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("not a business id: {business_id:?}");
    }
    let data = crate::database::default_db_path()?;
    let app = data.parent().and_then(|p| p.parent()).context("no app folder")?;
    Ok(app.join("cloud").join(format!("{business_id}.db")))
}

/// The business this database is linked to, if any.
pub fn link(conn: &Connection) -> Result<Option<Link>> {
    conn.query_row("SELECT business_id, business_name, role, today FROM sync_state WHERE id = 1", [], |r| {
        let role = match r.get_ref(2)?.as_str()? {
            "admin" => Role::Admin,
            "collector" => Role::Collector,
            _ => Role::Viewer,
        };
        let today: crate::database::repositories::Day = r.get(3)?;
        Ok(Link { business_id: r.get(0)?, business_name: r.get(1)?, role, today: today.0 })
    })
    .optional()
    .map_err(Into::into)
}

/// The cloud id and version of a local record.
pub fn remote(conn: &Connection, entity: Entity, local_id: i64, seq: i64) -> Result<Option<(String, i64)>> {
    conn.query_row(
        "SELECT remote_id, version FROM remote_ids WHERE entity = ?1 AND local_id = ?2 AND seq = ?3",
        params![entity.key(), local_id, seq],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .map_err(Into::into)
}

/// Milliseconds since 1970 as the local timestamp text.
fn stamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn opt(s: &str) -> Option<&str> {
    let t = s.trim();
    (!t.is_empty()).then_some(t)
}

/// Receipt wording keys, which the cloud keeps as written here.
const WORDING: &str =
    "key GLOB 'receipt_rules_*' OR key IN ('receipt_client_sign', 'receipt_lender_sign', 'receipt_payment_footer', 'receipt_loan_footer')";

/// Replaces the books in `conn` with `snap`. All or nothing: if any record
/// doesn't fit, the previous copy stays as it was. A database linked to a
/// different business is refused.
pub fn apply(conn: &mut Connection, snap: &Snapshot) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(l) = link(&tx)?
        && l.business_id != snap.business.id
    {
        bail!("this database follows another business ({})", l.business_name);
    }
    let linked: i64 = tx.query_row("SELECT COUNT(*) FROM sync_state", [], |r| r.get(0))?;
    if linked == 0 {
        let records: i64 = tx.query_row(
            "SELECT (SELECT COUNT(*) FROM clients) + (SELECT COUNT(*) FROM capital_transactions) + (SELECT COUNT(*) FROM cash_entries)",
            [],
            |r| r.get(0),
        )?;
        if records > 0 {
            bail!("this database has its own books; a linked business needs a database of its own");
        }
    }

    // Payments go before loans (they restrict deleting a loan); loans take
    // their schedules, late fees and status events with them. Clients stay,
    // so their photos do, unless the cloud no longer has them.
    tx.execute_batch(
        "DELETE FROM payments;
         DELETE FROM loans;
         DELETE FROM capital_transactions;
         DELETE FROM cash_entries;
         DELETE FROM remote_ids;",
    )?;
    let keep: Vec<i64> = snap.clients.iter().map(|c| c.record_no).collect();
    let keep = serde_json::to_string(&keep)?;
    tx.execute("DELETE FROM clients WHERE id NOT IN (SELECT value FROM json_each(?1))", [&keep])?;
    let photoless: Vec<i64> = snap.clients.iter().filter(|c| !c.has_photo).map(|c| c.record_no).collect();
    tx.execute("DELETE FROM client_photos WHERE client_id IN (SELECT value FROM json_each(?1))", [serde_json::to_string(&photoless)?])?;

    let mut remote = tx.prepare("INSERT INTO remote_ids (entity, local_id, seq, remote_id, version) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    let mut local = std::collections::HashMap::new();

    for c in &snap.clients {
        tx.execute(
            "INSERT INTO clients (id, full_name, phone, email, notes, created_at, archived_at, id_number, address)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET full_name = excluded.full_name, phone = excluded.phone, email = excluded.email,
                 notes = excluded.notes, created_at = excluded.created_at, archived_at = excluded.archived_at,
                 id_number = excluded.id_number, address = excluded.address",
            params![
                c.record_no,
                c.full_name,
                opt(&c.phone),
                opt(&c.email),
                c.notes,
                stamp(c.created_at),
                c.archived_at.map(stamp),
                opt(&c.id_number),
                opt(&c.address)
            ],
        )
        .with_context(|| format!("client {}", c.record_no))?;
        remote.execute(params![Entity::Client.key(), c.record_no, 0, c.id, c.version])?;
        local.insert(c.id.as_str(), c.record_no);
    }
    let client_no = |id: &str| local.get(id).copied().with_context(|| format!("unknown client {id}"));

    let mut loan_nos = std::collections::HashMap::new();
    for l in &snap.loans {
        tx.execute(
            "INSERT INTO loans (id, client_id, principal, period_rate, interest_period, frequency, method, installments,
                                schedule_edited, late_fee_rate, start_date, status, status_date, notes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                l.record_no,
                client_no(&l.client_id)?,
                l.principal,
                l.rate,
                l.interest_period,
                l.frequency,
                l.method,
                l.installments,
                l.schedule_edited,
                l.late_fee_rate,
                l.start_date.0.to_string(),
                l.status,
                l.status_date.map(|d| d.0.to_string()),
                l.notes,
                stamp(l.created_at)
            ],
        )
        .with_context(|| format!("loan {}", l.record_no))?;
        for x in &l.schedule {
            tx.execute(
                "INSERT INTO loan_installments (loan_id, seq, due_date, principal, interest) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![l.record_no, x.seq, x.due.0.to_string(), x.principal, x.interest],
            )?;
        }
        remote.execute(params![Entity::Loan.key(), l.record_no, 0, l.id, l.version])?;
        loan_nos.insert(l.id.as_str(), l.record_no);
    }
    let loan_no = |id: &str| loan_nos.get(id).copied().with_context(|| format!("unknown loan {id}"));

    for e in &snap.status_events {
        tx.execute(
            "INSERT INTO loan_status_events (loan_id, status, occurred_on, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![loan_no(&e.loan_id)?, e.status, e.occurred_on.0.to_string(), e.note, stamp(e.created_at)],
        )?;
    }

    for p in &snap.payments {
        tx.execute(
            "INSERT INTO payments (id, loan_id, paid_on, amount, notes, created_at, allocation_mode, target_seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                p.record_no,
                loan_no(&p.loan_id)?,
                p.paid_on.0.to_string(),
                p.amount,
                p.notes,
                stamp(p.created_at),
                p.mode,
                p.target_seq
            ],
        )
        .with_context(|| format!("payment {}", p.record_no))?;
        tx.execute(
            "INSERT INTO payment_allocations (payment_id, principal, interest, fee) VALUES (?1, ?2, ?3, ?4)",
            params![p.record_no, p.principal, p.interest, p.fee],
        )
        .with_context(|| format!("payment {}", p.record_no))?;
        for t in p.targets.iter().flatten() {
            tx.execute("INSERT INTO payment_targets (payment_id, seq, amount) VALUES (?1, ?2, ?3)", params![p.record_no, t.seq, t.amount])?;
        }
        for a in &p.applied {
            tx.execute(
                "INSERT INTO installment_allocations (payment_id, seq, principal, interest, late_fee) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![p.record_no, a.seq, a.principal, a.interest, a.fee],
            )?;
        }
        remote.execute(params![Entity::Payment.key(), p.record_no, 0, p.id, p.version])?;
    }

    for f in &snap.late_fees {
        let no = loan_no(&f.loan_id)?;
        tx.execute(
            "INSERT INTO late_fees (loan_id, seq, assessed_on, base_amount, rate, amount, waived_at, waive_reason, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                no,
                f.seq,
                f.assessed_on.0.to_string(),
                f.base_amount,
                f.rate,
                f.amount,
                f.waived_at.map(stamp),
                f.waive_reason,
                stamp(f.created_at),
                stamp(f.updated_at)
            ],
        )
        .with_context(|| format!("late fee {no}/{}", f.seq))?;
        remote.execute(params![Entity::LateFee.key(), no, f.seq, f.id, 0])?;
    }

    for c in &snap.capital {
        tx.execute(
            "INSERT INTO capital_transactions (id, kind, amount, occurred_on, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![c.record_no, c.kind, c.amount, c.occurred_on.0.to_string(), c.notes, stamp(c.created_at)],
        )
        .with_context(|| format!("capital {}", c.record_no))?;
        remote.execute(params![Entity::Capital.key(), c.record_no, 0, c.id, c.version])?;
    }

    for e in &snap.entries {
        tx.execute(
            "INSERT INTO cash_entries (id, kind, account, amount, occurred_on, description, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![e.record_no, e.kind, e.account, e.amount, e.occurred_on.0.to_string(), e.description, stamp(e.created_at)],
        )
        .with_context(|| format!("entry {}", e.record_no))?;
        remote.execute(params![Entity::Entry.key(), e.record_no, 0, e.id, e.version])?;
    }
    drop(remote);

    if let Some(s) = &snap.settings {
        let b = |v: bool| if v { "1" } else { "0" };
        let pairs = [
            ("business_name", s.receipt_name.as_str()),
            ("company_address", &s.company_address),
            ("company_phone", &s.company_phone),
            ("receipt_company_name", b(s.receipt_company.name)),
            ("receipt_company_address", b(s.receipt_company.address)),
            ("receipt_company_phone", b(s.receipt_company.phone)),
            ("receipt_client_name", b(s.receipt_client.name)),
            ("receipt_client_id_number", b(s.receipt_client.id_number)),
            ("receipt_client_phone", b(s.receipt_client.phone)),
            ("receipt_client_address", b(s.receipt_client.address)),
            ("currency", &s.currency_code),
            ("number_style", &s.number_style),
            ("date_style", &s.date_style),
            ("default_rate", &s.default_rate.to_string()),
            ("default_frequency", &s.default_frequency),
            ("late_fee_rate", &s.late_fee_rate.to_string()),
            ("receipt_after_payment", b(s.receipt_after_payment)),
            ("receipt_after_loan", b(s.receipt_after_loan)),
        ];
        for (k, v) in pairs {
            crate::database::repositories::settings::set(&tx, k, v)?;
        }
        tx.execute(&format!("DELETE FROM app_settings WHERE {WORDING}"), [])?;
        for (k, v) in &s.receipt_text {
            let wording: bool = tx.query_row(&format!("SELECT {WORDING} FROM (SELECT ?1 AS key)"), [k], |r| r.get(0))?;
            if wording {
                crate::database::repositories::settings::set(&tx, k, v)?;
            }
        }
    }

    tx.execute(
        "INSERT INTO sync_state (id, business_id, business_name, timezone, user_id, role, today, settings_version, applied_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT (id) DO UPDATE SET business_name = excluded.business_name, timezone = excluded.timezone,
             user_id = excluded.user_id, role = excluded.role, today = excluded.today,
             settings_version = excluded.settings_version, applied_at = excluded.applied_at",
        params![
            snap.business.id,
            snap.business.name,
            snap.business.timezone,
            snap.me.user_id,
            role_key(snap.me.role),
            snap.today,
            snap.settings.as_ref().map_or(0, |s| s.version),
            now_timestamp()
        ],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::repositories::{clients, settings};
    use crate::services::Store;
    use serde_json::{Value, json};

    /// A small book as `data.snapshot` sends it (numbers as floats).
    fn book() -> Value {
        json!({
            "business": { "_id": "biz1", "name": "Préstamos Uno", "timezone": "America/Santo_Domingo" },
            "me": { "userId": "u1", "role": "admin", "name": "Ana" },
            "today": "2026-03-10",
            "settings": {
                "receiptName": "Préstamos Uno", "companyAddress": "Calle 1", "companyPhone": "809",
                "receiptCompany": { "name": true, "address": true, "phone": false },
                "receiptClient": { "name": true, "idNumber": false, "phone": true, "address": true },
                "receiptText": { "receipt_payment_footer": "Gracias", "not_wording": "x" },
                "currencyCode": "DOP", "numberStyle": "comma_dot", "dateStyle": "medium", "language": "es",
                "defaultRate": 100000.0, "defaultFrequency": "monthly", "lateFeeRate": 50000.0,
                "receiptAfterPayment": true, "receiptAfterLoan": false, "logoUrl": null, "version": 4.0
            },
            "clients": [
                { "_id": "c7", "recordNo": 7.0, "fullName": "Juan Pérez", "phone": "809-555", "email": "", "idNumber": "001",
                  "address": "", "notes": "", "hasPhoto": true, "createdAt": 1767225600000.0, "version": 2.0 },
                { "_id": "c9", "recordNo": 9.0, "fullName": "Rosa", "phone": "", "email": "", "idNumber": "",
                  "address": "", "notes": "", "archivedAt": 1767312000000.0, "hasPhoto": false, "createdAt": 1767225600000.0, "version": 1.0 }
            ],
            "loans": [{
                "_id": "l3", "recordNo": 3.0, "clientId": "c7", "principal": 100000.0, "rate": 100000.0,
                "interestPeriod": "monthly", "frequency": "monthly", "method": "declining", "installments": 2.0,
                "scheduleEdited": false, "lateFeeRate": 50000.0, "startDate": "2026-01-01", "status": "active",
                "statusDate": null, "notes": "", "createdAt": 1767225600000.0, "version": 5.0,
                "schedule": [
                    { "seq": 1.0, "due": "2026-02-01", "principal": 50000.0, "interest": 10000.0 },
                    { "seq": 2.0, "due": "2026-03-01", "principal": 50000.0, "interest": 5000.0 }
                ]
            }],
            "payments": [{
                "_id": "p12", "loanId": "l3", "recordNo": 12.0, "paidOn": "2026-02-01", "amount": 60000.0, "notes": "",
                "mode": "auto", "principal": 50000.0, "interest": 10000.0, "fee": 0.0,
                "applied": [{ "seq": 1.0, "principal": 50000.0, "interest": 10000.0, "fee": 0.0 }],
                "createdAt": 1769904000000.0, "version": 1.0
            }],
            "lateFees": [{
                "_id": "f1", "loanId": "l3", "seq": 2.0, "assessedOn": "2026-03-02", "baseAmount": 55000.0,
                "rate": 50000.0, "amount": 2750.0, "waiveReason": "", "createdAt": 1772409600000.0, "updatedAt": 1772409600000.0
            }],
            "capital": [{ "_id": "k2", "recordNo": 2.0, "kind": "contribution", "amount": 500000.0, "occurredOn": "2026-01-01",
                          "notes": "", "createdAt": 1767225600000.0, "version": 1.0 }],
            "entries": [{ "_id": "e4", "recordNo": 4.0, "kind": "expense", "account": "interest", "amount": 1500.0,
                          "occurredOn": "2026-02-15", "description": "Gasolina", "createdAt": 1771113600000.0, "version": 1.0 }],
            "statusEvents": [{ "loanId": "l3", "status": "active", "occurredOn": "2026-01-01", "note": "", "createdAt": 1767225600000.0 }],
            "people": []
        })
    }

    fn snap(v: Value) -> Snapshot {
        serde_json::from_value(super::super::wire::normalize(v)).unwrap()
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn books_arrive_with_record_numbers_as_ids() {
        let mut conn = crate::database::open_in_memory().unwrap();
        apply(&mut conn, &snap(book())).unwrap();
        // Applying the same copy again changes nothing.
        apply(&mut conn, &snap(book())).unwrap();

        let ids =
            |sql: &str| conn.prepare(sql).unwrap().query_map([], |r| r.get::<_, i64>(0)).unwrap().map(Result::unwrap).collect::<Vec<_>>();
        assert_eq!(ids("SELECT id FROM clients ORDER BY id"), [7, 9]);
        assert_eq!(ids("SELECT id FROM loans"), [3]);
        assert_eq!(ids("SELECT id FROM payments"), [12]);
        assert_eq!(ids("SELECT id FROM capital_transactions"), [2]);
        assert_eq!(ids("SELECT id FROM cash_entries"), [4]);
        for (table, n) in
            [("loan_installments", 2), ("loan_status_events", 1), ("late_fees", 1), ("installment_allocations", 1), ("remote_ids", 7)]
        {
            assert_eq!(count(&conn, table), n, "{table}");
        }
        assert_eq!(remote(&conn, Entity::Loan, 3, 0).unwrap(), Some(("l3".into(), 5)));
        assert_eq!(remote(&conn, Entity::LateFee, 3, 2).unwrap(), Some(("f1".into(), 0)));

        let c = clients::get(&conn, 9).unwrap().unwrap();
        assert!(c.is_archived());
        assert_eq!(c.phone, None);
        let l = link(&conn).unwrap().unwrap();
        assert_eq!((l.business_id.as_str(), l.role, l.today.to_string()), ("biz1", Role::Admin, "2026-03-10".into()));
        let s = settings::all(&conn).unwrap();
        assert_eq!((s["business_name"].as_str(), s["receipt_company_phone"].as_str()), ("Préstamos Uno", "0"));
        assert_eq!(s.get("receipt_payment_footer").map(String::as_str), Some("Gracias"));
        assert!(!s.contains_key("not_wording"));
    }

    #[test]
    fn the_engine_reads_the_copy() {
        let mut store = Store::in_memory();
        store.apply_cloud(&snap(book())).unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
        let s = store.snapshot().unwrap();
        let sum = s.book.summary(today);
        assert_eq!(sum.contributions, 500000);
        assert_eq!(sum.principal_repaid, 50000);
        assert_eq!(sum.interest_collected, 10000);
        assert_eq!(sum.interest_expenses, 1500);
        assert_eq!(sum.outstanding_principal, 50000);
        assert_eq!(sum.overdue_loans, 1);
        // The cloud already assessed the late fee; checking again adds none.
        assert_eq!(store.refresh(today).unwrap(), 0);
        assert_eq!(store.cloud_link().unwrap().map(|l| l.business_name), Some("Préstamos Uno".into()));
    }

    #[test]
    fn photos_and_this_pcs_preferences_stay() {
        let mut conn = crate::database::open_in_memory().unwrap();
        apply(&mut conn, &snap(book())).unwrap();
        clients::set_photo(&conn, 7, b"jpeg7").unwrap();
        clients::set_photo(&conn, 9, b"jpeg9").unwrap();
        settings::set(&conn, "theme", "dark").unwrap();
        settings::set(&conn, "receipt_loan_footer", "old wording").unwrap();

        let mut next = book();
        next["clients"][0]["fullName"] = json!("Juan P. Pérez");
        next["settings"]["receiptText"] = json!({});
        apply(&mut conn, &snap(next)).unwrap();
        assert_eq!(clients::get(&conn, 7).unwrap().unwrap().full_name, "Juan P. Pérez");
        assert_eq!(clients::photo(&conn, 7).unwrap().as_deref(), Some(&b"jpeg7"[..]));
        assert_eq!(clients::photo(&conn, 9).unwrap(), None, "the cloud has no photo for client 9");
        let s = settings::all(&conn).unwrap();
        assert_eq!(s.get("theme").map(String::as_str), Some("dark"));
        assert!(!s.contains_key("receipt_loan_footer") && !s.contains_key("receipt_payment_footer"));

        // A client deleted in the cloud goes, with its photo.
        let mut next = book();
        next["clients"].as_array_mut().unwrap().remove(1);
        apply(&mut conn, &snap(next)).unwrap();
        assert_eq!(count(&conn, "clients"), 1);
    }

    #[test]
    fn a_copy_that_doesnt_fit_changes_nothing() {
        let mut conn = crate::database::open_in_memory().unwrap();
        apply(&mut conn, &snap(book())).unwrap();
        let mut bad = book();
        bad["capital"][0]["amount"] = json!(700000.0);
        bad["payments"][0]["interest"] = json!(9000.0); // no longer adds up to the payment
        let err = apply(&mut conn, &snap(bad)).unwrap_err();
        assert!(format!("{err:#}").contains("payment 12"), "{err:#}");
        let amount: i64 = conn.query_row("SELECT amount FROM capital_transactions", [], |r| r.get(0)).unwrap();
        assert_eq!((amount, count(&conn, "payments")), (500000, 1));
    }

    #[test]
    fn only_its_own_business_and_never_local_books() {
        let mut conn = crate::database::open_in_memory().unwrap();
        apply(&mut conn, &snap(book())).unwrap();
        let mut other = book();
        other["business"]["_id"] = json!("biz2");
        assert!(apply(&mut conn, &snap(other)).is_err());

        let mut local = crate::database::open_in_memory().unwrap();
        local.execute("INSERT INTO clients (full_name, notes, created_at) VALUES ('Mine', '', 'x')", []).unwrap();
        assert!(apply(&mut local, &snap(book())).is_err());
        assert_eq!(count(&local, "loans"), 0);
    }

    #[test]
    fn linked_files_live_in_the_cloud_folder() {
        let p = path_for("k57abc123").unwrap();
        assert!(p.ends_with("cloud/k57abc123.db"), "{}", p.display());
        assert!(path_for("../x").is_err() && path_for("").is_err());
    }
}
