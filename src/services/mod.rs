//! Application services: every write goes through here, validated against
//! the accounting rules and committed atomically in one SQLite transaction.

pub mod export;
pub mod receipt;
pub mod settings;

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use chrono::NaiveDate;
use rusqlite::{Connection, TransactionBehavior};

use crate::database::repositories::{
    capital::{self, CapitalTx},
    clients::{self, Client, ClientInput},
    loans::{self, Loan},
    payments::{self, Payment},
    settings as settings_repo,
};
use crate::database::{self, BackupInfo};
use crate::domain::ledger::{Allocation, ContractStatus, LoanFacts, PaymentError, allocate};
use crate::domain::loans::LoanTerms;
use crate::domain::money::Cents;
use crate::domain::portfolio::{Book, CapitalKind, LoanBook};
use crate::i18n::t;
use settings::Settings;

/// A user-facing validation failure. Its message is shown as-is.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

/// A loan or withdrawal larger than the cash available on (and after) its
/// date.
#[derive(Debug, thiserror::Error)]
#[error("Not enough available capital")]
pub struct InsufficientCash {
    pub available: Cents,
}

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

fn build_book(loans: &[Loan], payments: &[Payment], capital: &[CapitalTx]) -> Book {
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
    }
}

fn load_book(conn: &Connection) -> Result<Book> {
    Ok(build_book(&loans::list(conn)?, &payments::list(conn)?, &capital::list(conn)?))
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
        let clients = clients::list(&self.conn)?;
        let loans = loans::list(&self.conn)?;
        let payments = payments::list(&self.conn)?;
        let capital = capital::list(&self.conn)?;
        let book = build_book(&loans, &payments, &capital);
        let settings = Settings::from_map(&settings_repo::all(&self.conn)?);
        Ok(Snapshot { clients, loans, payments, capital, book, settings })
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
            return invalid(t("Phone numbers may contain digits, spaces and + - ( )", "El teléfono solo puede tener números, espacios y + - ( )"));
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
            clients::get(c, id)?.ok_or_else(|| anyhow!("client not found"))?;
            clients::update(c, id, input)
        })
    }

    pub fn set_client_archived(&mut self, id: i64, archived: bool) -> Result<()> {
        self.write(|c| {
            clients::get(c, id)?.ok_or_else(|| anyhow!("client not found"))?;
            clients::set_archived(c, id, archived)
        })
    }

    // ----- loans ----------------------------------------------------------

    pub fn create_loan(&mut self, client_id: i64, terms: &LoanTerms, notes: &str, today: NaiveDate) -> Result<i64> {
        terms.validate().map_err(|e| Invalid(e.message().into()))?;
        if terms.start_date > today {
            return invalid(t("The loan date can't be in the future", "La fecha del préstamo no puede ser futura"));
        }
        self.write(|c| {
            let client = clients::get(c, client_id)?.ok_or_else(|| Invalid(t("Choose a client", "Elige un cliente").into()))?;
            if client.is_archived() {
                return invalid(t(
                    "This client is archived. Restore the client to lend again.",
                    "Este cliente está archivado. Restáuralo para volver a prestarle.",
                ));
            }
            let cash = load_book(c)?.min_cash_from(terms.start_date);
            if terms.principal > cash {
                return Err(InsufficientCash { available: cash.max(0) }.into());
            }
            let id = loans::insert(c, client_id, terms, notes)?;
            loans::set_status(c, id, ContractStatus::Active, terms.start_date, "Loan disbursed")?;
            Ok(id)
        })
    }

    pub fn mark_defaulted(&mut self, loan_id: i64, on: NaiveDate, note: &str, today: NaiveDate) -> Result<()> {
        if on > today {
            return invalid(t("The date can't be in the future", "La fecha no puede ser futura"));
        }
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            if loan.status != ContractStatus::Active {
                return invalid(t("Only active loans can be marked as defaulted", "Solo los préstamos activos pueden marcarse como incobrables"));
            }
            if on < loan.terms.start_date {
                return invalid(t("The date is before the loan started", "La fecha es anterior al inicio del préstamo"));
            }
            if let Some(last) = payments::for_loan(c, loan_id)?.iter().filter(|p| !p.is_reversed()).map(|p| p.paid_on).max()
                && on < last
            {
                return invalid(t("The date is before the latest payment", "La fecha es anterior al último pago"));
            }
            loans::set_status(c, loan_id, ContractStatus::Defaulted, on, note)
        })
    }

    pub fn reinstate(&mut self, loan_id: i64, today: NaiveDate) -> Result<()> {
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            if loan.status != ContractStatus::Defaulted {
                return invalid(t("Only defaulted loans can be reinstated", "Solo los préstamos incobrables pueden reactivarse"));
            }
            loans::set_status(c, loan_id, ContractStatus::Active, today, "Reinstated")
        })
    }

    // ----- payments -------------------------------------------------------

    fn loan_and_history(c: &Connection, loan_id: i64) -> Result<(Loan, Vec<Payment>)> {
        let loan = loans::get(c, loan_id)?.ok_or_else(|| Invalid(t("Choose a loan", "Elige un préstamo").into()))?;
        let history = payments::for_loan(c, loan_id)?.into_iter().filter(|p| !p.is_reversed()).collect();
        Ok((loan, history))
    }

    pub fn record_payment(&mut self, loan_id: i64, amount: Cents, fee: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        self.write(|c| {
            let (loan, history) = Self::loan_and_history(c, loan_id)?;
            let facts: Vec<_> = history.iter().map(Payment::fact).collect();
            let alloc = allocate(&loan_facts(&loan), &facts, amount, fee, on, today).map_err(|e| Invalid(e.message().into()))?;
            let id = payments::insert(c, loan_id, on, amount, &alloc, notes)?;
            if alloc.pays_off() {
                loans::set_status(c, loan_id, ContractStatus::PaidOff, on, "Paid in full")?;
            }
            Ok(id)
        })
    }

    /// Reverses the latest payment on a loan. The row is kept for audit and
    /// excluded from every calculation. Only the latest payment can be
    /// reversed, because later allocations depend on earlier ones.
    pub fn reverse_payment(&mut self, payment_id: i64, reason: &str, today: NaiveDate) -> Result<()> {
        if reason.trim().is_empty() {
            return invalid(t("Give a reason for the reversal", "Indica el motivo de la anulación"));
        }
        self.write(|c| {
            let payment = payments::get(c, payment_id)?.ok_or_else(|| anyhow!("payment not found"))?;
            if payment.is_reversed() {
                return invalid(t("This payment is already reversed", "Este pago ya está anulado"));
            }
            let (loan, history) = Self::loan_and_history(c, payment.loan_id)?;
            if history.last().map(|p| p.id) != Some(payment_id) {
                return invalid(t(
                    "Only the most recent payment on a loan can be reversed. Reverse later payments first.",
                    "Solo se puede anular el pago más reciente del préstamo. Anula primero los posteriores.",
                ));
            }
            if !matches!(loan.status, ContractStatus::Active | ContractStatus::PaidOff) {
                return invalid(t("Reinstate the loan before reversing its payments", "Reactiva el préstamo antes de anular sus pagos"));
            }
            payments::mark_reversed(c, payment_id, reason)?;
            if load_book(c)?.min_cash_from(payment.paid_on) < 0 {
                bail!(Invalid(
                    t(
                        "Reversing this payment would make available cash negative, because the money was lent or withdrawn later.",
                        "Anular este pago dejaría el efectivo disponible en negativo, porque ese dinero ya se prestó o retiró.",
                    )
                    .into()
                ));
            }
            if loan.status == ContractStatus::PaidOff {
                loans::set_status(c, loan.id, ContractStatus::Active, today, "Payment reversed")?;
            }
            Ok(())
        })
    }

    // ----- capital --------------------------------------------------------

    pub fn add_capital(&mut self, kind: CapitalKind, amount: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        if amount <= 0 {
            return invalid(t("Enter an amount greater than zero", "Escribe un monto mayor que cero"));
        }
        if on > today {
            return invalid(t("The date can't be in the future", "La fecha no puede ser futura"));
        }
        self.write(|c| {
            if kind == CapitalKind::Withdrawal {
                let cash = load_book(c)?.min_cash_from(on);
                if amount > cash {
                    return Err(InsufficientCash { available: cash.max(0) }.into());
                }
            }
            capital::insert(c, kind, amount, on, notes)
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
}

/// Preview of how a payment would be split, for forms.
pub fn preview_allocation(book: &LoanBook, amount: Cents, fee: Cents, on: NaiveDate, today: NaiveDate) -> Result<Allocation, PaymentError> {
    allocate(&book.facts, &book.payments, amount, fee, on, today)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loans::Frequency;
    use crate::domain::money::Rate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn terms(principal: Cents, start: NaiveDate) -> LoanTerms {
        LoanTerms { principal, rate: Rate(100_000), frequency: Frequency::Monthly, start_date: start }
    }

    fn client(store: &mut Store) -> i64 {
        store.create_client(&ClientInput { full_name: "Marta Quiroga".into(), ..Default::default() }).unwrap()
    }

    #[test]
    fn full_lifecycle_through_the_database() {
        let mut s = Store::in_memory();
        let today = d(2026, 6, 1);
        let cid = client(&mut s);
        s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "Opening capital", today).unwrap();
        let lid = s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap();

        // 1,000 at 10%: paying 600 is 100 interest and 500 capital.
        let pid = s.record_payment(lid, 60_000, 0, d(2026, 2, 1), "", today).unwrap();
        let snap = s.snapshot().unwrap();
        let p = snap.payments.iter().find(|p| p.id == pid).unwrap();
        assert_eq!((p.interest, p.principal), (10_000, 50_000));
        assert_eq!(snap.book.summary(today).interest_collected, 10_000);

        // Reverse it: it stays on record but no longer counts.
        s.reverse_payment(pid, "Entered twice", today).unwrap();
        let snap = s.snapshot().unwrap();
        assert!(snap.payments.iter().find(|p| p.id == pid).unwrap().is_reversed());
        assert_eq!(snap.book.summary(today).interest_collected, 0);
        assert!(s.reverse_payment(pid, "again", today).is_err());

        // Pay off in full; the loan closes in the same transaction.
        let book = snap.book_loan(lid).unwrap();
        let payoff = crate::domain::ledger::owed_at_payment(&book.facts, &book.payments, today).payoff();
        s.record_payment(lid, payoff, 0, today, "Payoff", today).unwrap();
        let snap = s.snapshot().unwrap();
        assert_eq!(snap.loan(lid).unwrap().status, ContractStatus::PaidOff);
        assert!(s.record_payment(lid, 100, 0, today, "", today).is_err());
    }

    #[test]
    fn cannot_lend_or_withdraw_money_that_is_not_there() {
        let mut s = Store::in_memory();
        let today = d(2026, 6, 1);
        let cid = client(&mut s);
        let err = s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap_err();
        assert_eq!(err.downcast_ref::<InsufficientCash>().unwrap().available, 0);
        s.add_capital(CapitalKind::Contribution, 100_000, d(2026, 1, 1), "", today).unwrap();
        s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap();
        assert!(s.add_capital(CapitalKind::Withdrawal, 1, d(2026, 1, 2), "", today).is_err());
        s.add_capital(CapitalKind::Contribution, 50_000, d(2026, 3, 1), "", today).unwrap();
        assert!(s.create_loan(cid, &terms(50_000, d(2026, 2, 1)), "", today).is_err());
    }

    #[test]
    fn failed_payment_leaves_no_trace() {
        let mut s = Store::in_memory();
        let today = d(2026, 6, 1);
        let cid = client(&mut s);
        s.add_capital(CapitalKind::Contribution, 1_000_000, d(2026, 1, 1), "", today).unwrap();
        let lid = s.create_loan(cid, &terms(1_000_000, d(2026, 1, 1)), "", today).unwrap();
        assert!(s.record_payment(lid, 99_999_999, 0, d(2026, 2, 1), "", today).is_err());
        assert!(s.snapshot().unwrap().payments.is_empty());
        // A rejected allocation at the database level rolls back the payment row.
        let tx_result: Result<()> = s.write(|c| {
            payments::insert(
                c,
                lid,
                d(2026, 2, 1),
                1_000,
                &Allocation { interest: 1, principal: 1, fee: 0, principal_before: 0, principal_after: 0, interest_after: 0 },
                "",
            )?;
            Ok(())
        });
        assert!(tx_result.is_err());
        let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn only_latest_payment_can_be_reversed() {
        let mut s = Store::in_memory();
        let today = d(2026, 6, 1);
        let cid = client(&mut s);
        s.add_capital(CapitalKind::Contribution, 1_000_000, d(2026, 1, 1), "", today).unwrap();
        let lid = s.create_loan(cid, &terms(500_000, d(2026, 1, 1)), "", today).unwrap();
        let first = s.record_payment(lid, 10_000, 0, d(2026, 2, 1), "", today).unwrap();
        s.record_payment(lid, 10_000, 0, d(2026, 3, 1), "", today).unwrap();
        assert!(s.reverse_payment(first, "wrong", today).is_err());
    }

    #[test]
    fn client_validation_and_archive() {
        let mut s = Store::in_memory();
        assert!(s.create_client(&ClientInput { full_name: "  ".into(), ..Default::default() }).is_err());
        assert!(s.create_client(&ClientInput { full_name: "A".into(), email: "nope".into(), ..Default::default() }).is_err());
        let id = client(&mut s);
        s.set_client_archived(id, true).unwrap();
        s.add_capital(CapitalKind::Contribution, 100_000, d(2026, 1, 1), "", d(2026, 6, 1)).unwrap();
        assert!(s.create_loan(id, &terms(1_000, d(2026, 1, 1)), "", d(2026, 6, 1)).is_err());
        assert_eq!(s.snapshot().unwrap().clients.len(), 1);
    }

    #[test]
    fn settings_round_trip() {
        let mut s = Store::in_memory();
        let mut st = s.snapshot().unwrap().settings;
        assert_eq!(st, Settings::default());
        st.currency_code = "USD".into();
        st.business_name = "Préstamos Valdez".into();
        st.language = crate::i18n::Lang::Es;
        st.default_rate = Rate(150_000);
        st.sidebar_collapsed = true;
        s.save_settings(&st).unwrap();
        assert_eq!(s.snapshot().unwrap().settings, st);
        st.business_name = "   ".into();
        assert!(s.save_settings(&st).is_err());
    }

    #[test]
    fn writes_a_receipt() {
        let mut s = Store::in_memory();
        let dir = tempfile::tempdir().unwrap();
        let today = d(2026, 6, 1);
        let cid = client(&mut s);
        s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
        let lid = s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap();
        let pid = s.record_payment(lid, 60_000, 0, d(2026, 2, 1), "Cash", today).unwrap();
        let snap = s.snapshot().unwrap();
        let data = receipt::data(&snap, pid).unwrap();
        assert_eq!((data.interest, data.principal), (10_000, 50_000));
        assert_eq!((data.principal_before, data.principal_after), (100_000, 50_000));
        assert_eq!(data.next_due, Some((d(2026, 3, 1), 5_000)));
        let path = receipt::write(&snap, pid, dir.path()).unwrap();
        let html = std::fs::read_to_string(path).unwrap();
        assert!(html.contains("Marta Quiroga"));
        assert!(html.contains("50.0%"));
    }
}
