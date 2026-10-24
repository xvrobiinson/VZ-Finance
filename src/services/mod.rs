//! Application services: every write goes through here, validated against
//! the accounting rules and committed atomically in one SQLite transaction.

pub mod export;
pub mod logo;
pub mod receipt;
mod records;
pub mod settings;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rusqlite::{Connection, TransactionBehavior};

use crate::database::repositories::{
    assets,
    capital::{self, CapitalTx},
    changes::{self, Action, Change, Entity, FieldChange},
    clients::{self, Client, ClientInput},
    entries::{self, CashEntry},
    loans::{self, Loan},
    payments::{self, Payment},
    settings as settings_repo,
};
use crate::database::{self, BackupInfo};
use crate::domain::ledger::{Allocation, ContractStatus, LoanFacts, PaymentError, PaymentFact, allocate};
use crate::domain::money::Cents;
use crate::domain::portfolio::{Account, Book, LoanBook, Shortfall};
use crate::i18n::t;
use settings::Settings;

/// A user-facing validation failure. Its message is shown as-is.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

/// A new loan, withdrawal, expense or transfer larger than what is
/// available on (and after) its date.
#[derive(Debug, thiserror::Error)]
#[error("Not enough available money")]
pub struct InsufficientCash {
    pub available: Cents,
    /// The account it comes from, or `None` for a loan (capital first, then
    /// interest).
    pub account: Option<Account>,
}

/// A change that would leave an account below zero at some point, because
/// the money was already lent, spent or withdrawn later.
#[derive(Debug, thiserror::Error)]
#[error("An account would go below zero")]
pub struct CashShortfall(pub Shortfall);

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Invalid(msg.into()).into())
}

/// Everything the UI shows, loaded in one pass and derived from the ledger.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub clients: Vec<Client>,
    pub loans: Vec<Loan>,
    pub payments: Vec<Payment>,
    pub capital: Vec<CapitalTx>,
    pub entries: Vec<CashEntry>,
    /// Edits and deletions, newest first.
    pub changes: Vec<Change>,
    /// The business logo as PNG.
    pub logo: Option<Arc<[u8]>>,
    pub book: Book,
    pub settings: Settings,
}

impl Snapshot {
    pub fn client(&self, id: i64) -> Option<&Client> {
        self.clients.iter().find(|c| c.id == id)
    }
    pub fn loan(&self, id: i64) -> Option<&Loan> {
        self.loans.iter().find(|l| l.id == id)
    }
    pub fn payment(&self, id: i64) -> Option<&Payment> {
        self.payments.iter().find(|p| p.id == id)
    }
    pub fn capital_tx(&self, id: i64) -> Option<&CapitalTx> {
        self.capital.iter().find(|c| c.id == id)
    }
    pub fn entry(&self, id: i64) -> Option<&CashEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
    pub fn book_loan(&self, id: i64) -> Option<&LoanBook> {
        self.book.loans.iter().find(|l| l.id == id)
    }
    pub fn client_name(&self, id: i64) -> &str {
        self.client(id).map(|c| c.full_name.as_str()).unwrap_or("—")
    }
    pub fn loans_for_client(&self, client_id: i64) -> impl Iterator<Item = &Loan> {
        self.loans.iter().filter(move |l| l.client_id == client_id)
    }
    pub fn payments_for_loan(&self, loan_id: i64) -> impl Iterator<Item = &Payment> {
        self.payments.iter().filter(move |p| p.loan_id == loan_id)
    }
    /// The ledger without one record, so a form editing it can show what
    /// would be available with the record changed.
    pub fn book_without(&self, record: Record) -> Book {
        let mut book = self.book.clone();
        match record {
            Record::Loan(id) => book.loans.retain(|l| l.id != id),
            Record::Capital(id) => book.capital.retain(|c| c.id != id),
            Record::Entry(id) => book.entries.retain(|e| e.id != id),
        }
        book
    }
}

/// A record that moves cash, for [`Snapshot::book_without`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Record {
    Loan(i64),
    Capital(i64),
    Entry(i64),
}

pub fn loan_facts(loan: &Loan) -> LoanFacts {
    LoanFacts {
        principal: loan.terms.principal,
        rate: loan.terms.rate,
        frequency: loan.terms.frequency,
        start_date: loan.terms.start_date,
        status: loan.status,
        closed_on: match loan.status {
            ContractStatus::Defaulted | ContractStatus::Restructured => loan.status_date,
            _ => None,
        },
    }
}

fn build_book(loans: &[Loan], payments: &[Payment], capital: &[CapitalTx], entries: &[CashEntry]) -> Book {
    Book {
        loans: loans
            .iter()
            .map(|l| LoanBook {
                id: l.id,
                client_id: l.client_id,
                facts: loan_facts(l),
                payments: payments.iter().filter(|p| p.loan_id == l.id && !p.is_reversed()).map(Payment::fact).collect(),
            })
            .collect(),
        capital: capital.iter().map(CapitalTx::fact).collect(),
        entries: entries.iter().map(CashEntry::fact).collect(),
    }
}

fn load_book(conn: &Connection) -> Result<Book> {
    Ok(build_book(&loans::list(conn)?, &payments::list(conn)?, &capital::list(conn)?, &entries::list(conn)?))
}

/// Refuses a write that leaves an account below zero at any point.
fn check_cash(conn: &Connection) -> Result<()> {
    match load_book(conn)?.shortfall() {
        Some(s) => Err(CashShortfall(s).into()),
        None => Ok(()),
    }
}

/// Field-by-field record of an edit, for the change history.
#[derive(Default)]
struct Diff(Vec<FieldChange>);

impl Diff {
    fn field(mut self, name: &str, old: impl ToString, new: impl ToString) -> Self {
        let (old, new) = (old.to_string(), new.to_string());
        if old != new {
            self.0.push(FieldChange { field: name.into(), old, new });
        }
        self
    }
    /// A deleted record's values.
    fn removed(mut self, name: &str, old: impl ToString) -> Self {
        self.0.push(FieldChange { field: name.into(), old: old.to_string(), new: String::new() });
        self
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn log(self, c: &Connection, entity: Entity, id: i64, loan_id: Option<i64>, action: Action, reason: &str) -> Result<()> {
        changes::insert(c, entity, id, loan_id, action, &self.0, reason)?;
        Ok(())
    }
}

fn opt(s: &Option<String>) -> &str {
    s.as_deref().unwrap_or("")
}

pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    pub fn open(path: PathBuf) -> Result<Self> {
        let conn = database::open(&path)?;
        Ok(Self { conn, path })
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self { conn: database::open_in_memory().unwrap(), path: PathBuf::from(":memory:") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn app_dir(&self) -> PathBuf {
        self.path.parent().and_then(Path::parent).unwrap_or(Path::new(".")).to_path_buf()
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.app_dir().join("backups")
    }

    pub fn receipts_dir(&self) -> PathBuf {
        self.app_dir().join("receipts")
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let c = &self.conn;
        let clients = clients::list(c)?;
        let loans = loans::list(c)?;
        let payments = payments::list(c)?;
        let capital = capital::list(c)?;
        let entries = entries::list(c)?;
        let book = build_book(&loans, &payments, &capital, &entries);
        let settings = Settings::from_map(&settings_repo::all(c)?);
        let changes = changes::list(c)?;
        let logo = assets::get(c, assets::LOGO)?.map(Arc::from);
        Ok(Snapshot { clients, loans, payments, capital, entries, changes, logo, book, settings })
    }

    fn write<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    // ----- settings -------------------------------------------------------

    pub fn save_settings(&mut self, s: &Settings) -> Result<()> {
        if s.business_name.trim().is_empty() {
            return invalid(t("Enter a business name", "Escribe el nombre del negocio"));
        }
        if s.business_name.trim().chars().count() > 60 {
            return invalid(t("The name is too long (60 characters at most)", "El nombre es muy largo (máximo 60 caracteres)"));
        }
        self.write(|c| {
            for (k, v) in s.to_pairs() {
                settings_repo::set(c, k, &v)?;
            }
            Ok(())
        })
    }

    /// Saves an uploaded picture as the business logo, shrunk to a size
    /// that fits receipts and the sidebar.
    pub fn set_logo(&mut self, bytes: &[u8]) -> Result<()> {
        let png = logo::prepare(bytes).map_err(|e| {
            Invalid(match e {
                logo::LogoError::TooLarge => {
                    t("The picture is too large (20 MB at most)", "La imagen es demasiado grande (máximo 20 MB)").into()
                }
                logo::LogoError::Unreadable => t(
                    "This file isn't a picture the app can read. Use a PNG, JPG, GIF, BMP or WebP image.",
                    "La app no puede leer este archivo. Usa una imagen PNG, JPG, GIF, BMP o WebP.",
                )
                .into(),
            })
        })?;
        self.write(|c| assets::set(c, assets::LOGO, "image/png", &png))
    }

    pub fn remove_logo(&mut self) -> Result<()> {
        self.write(|c| assets::remove(c, assets::LOGO))
    }

    // ----- clients --------------------------------------------------------

    fn validate_client(input: &ClientInput) -> Result<()> {
        let name = input.full_name.trim();
        if name.is_empty() {
            return invalid(t("Enter the client's full name", "Escribe el nombre completo del cliente"));
        }
        if name.chars().count() > 200 {
            return invalid(t("The name is too long", "El nombre es muy largo"));
        }
        let email = input.email.trim();
        if !email.is_empty() && (!email.contains('@') || email.contains(' ') || email.len() > 200) {
            return invalid(t("Enter a valid email address, or leave it empty", "Escribe un correo válido o déjalo vacío"));
        }
        let phone = input.phone.trim();
        if phone.len() > 60 || !phone.chars().all(|c| c.is_ascii_digit() || " +-().ext".contains(c)) {
            return invalid(t(
                "Phone numbers may contain digits, spaces and + - ( )",
                "El teléfono solo puede tener números, espacios y + - ( )",
            ));
        }
        Ok(())
    }

    pub fn create_client(&mut self, input: &ClientInput) -> Result<i64> {
        Self::validate_client(input)?;
        self.write(|c| clients::insert(c, input))
    }

    pub fn update_client(&mut self, id: i64, input: &ClientInput) -> Result<()> {
        Self::validate_client(input)?;
        self.write(|c| {
            let old = clients::get(c, id)?.ok_or_else(|| anyhow!("client not found"))?;
            clients::update(c, id, input)?;
            let diff = Diff::default()
                .field("full_name", &old.full_name, input.full_name.trim())
                .field("phone", opt(&old.phone), input.phone.trim())
                .field("email", opt(&old.email), input.email.trim())
                .field("notes", &old.notes, input.notes.trim());
            if !diff.is_empty() {
                diff.log(c, Entity::Client, id, None, Action::Update, "")?;
            }
            Ok(())
        })
    }

    pub fn set_client_archived(&mut self, id: i64, archived: bool) -> Result<()> {
        self.write(|c| {
            clients::get(c, id)?.ok_or_else(|| anyhow!("client not found"))?;
            clients::set_archived(c, id, archived)
        })
    }

    /// Deletes a client who has no loans.
    pub fn delete_client(&mut self, id: i64, reason: &str) -> Result<()> {
        self.write(|c| {
            let old = clients::get(c, id)?.ok_or_else(|| anyhow!("client not found"))?;
            if loans::list(c)?.iter().any(|l| l.client_id == id) {
                return invalid(t(
                    "This client has loans. Delete the loans, or move them to another client by editing them, first.",
                    "Este cliente tiene préstamos. Primero elimínalos o pásalos a otro cliente editándolos.",
                ));
            }
            Diff::default()
                .removed("full_name", &old.full_name)
                .removed("phone", opt(&old.phone))
                .removed("email", opt(&old.email))
                .removed("notes", &old.notes)
                .log(c, Entity::Client, id, None, Action::Delete, reason)?;
            clients::delete(c, id)
        })
    }

    // ----- files ----------------------------------------------------------

    pub fn record_backup_time(&mut self) -> Result<()> {
        let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        self.write(|c| settings_repo::set(c, "last_backup_at", &stamp))
    }

    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        database::backup_to(&self.conn, dest)
    }

    pub fn inspect_backup(path: &Path) -> Result<BackupInfo> {
        database::validate_backup(path)
    }

    pub fn restore_from(&mut self, source: &Path) -> Result<PathBuf> {
        let dir = self.backups_dir();
        database::restore_from(&mut self.conn, source, &dir)
    }

    pub fn export_csv(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        export::export_all(&self.snapshot()?, dir)
    }

    /// Writes a printable receipt for a payment and returns its path.
    pub fn write_receipt(&self, payment_id: i64) -> Result<PathBuf> {
        let snap = self.snapshot()?;
        receipt::write(&snap, payment_id, &self.receipts_dir())
    }

    /// Writes the printable receipt for a loan and returns its path.
    pub fn write_loan_receipt(&self, loan_id: i64) -> Result<PathBuf> {
        let snap = self.snapshot()?;
        receipt::write_loan(&snap, loan_id, &self.receipts_dir())
    }
}

/// The live payments that come before a new payment dated `on` (or before
/// payment `editing` moved to `on`), in ledger order.
pub fn payments_before(snap: &Snapshot, loan_id: i64, editing: Option<i64>, on: NaiveDate) -> Vec<PaymentFact> {
    let key = (on, editing.unwrap_or(i64::MAX));
    snap.payments
        .iter()
        .filter(|p| p.loan_id == loan_id && !p.is_reversed() && Some(p.id) != editing && (p.paid_on, p.id) < key)
        .map(Payment::fact)
        .collect()
}

/// How a payment would be split if recorded now, or, when `editing`, if
/// that payment were changed. Later payments are split again on save.
pub fn preview_payment(
    snap: &Snapshot,
    loan_id: i64,
    editing: Option<i64>,
    amount: Cents,
    fee: Cents,
    on: NaiveDate,
    today: NaiveDate,
) -> Option<Result<Allocation, PaymentError>> {
    let loan = snap.loan(loan_id)?;
    let mut facts = loan_facts(loan);
    if editing.is_some() {
        facts.status = ContractStatus::Active;
        facts.closed_on = None;
    }
    Some(allocate(&facts, &payments_before(snap, loan_id, editing, on), amount, fee, on, today))
}

#[cfg(test)]
mod tests;
