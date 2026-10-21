//! Application services: every write goes through here, validated against
//! the accounting rules and committed atomically in one SQLite transaction.

pub mod export;
pub mod settings;

use std::collections::HashMap;
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
use crate::domain::loans::{Installment, LoanTerms};
use crate::domain::money::Cents;
use crate::domain::portfolio::{Book, CapitalKind, LoanBook};
use settings::Settings;

/// A user-facing validation failure. Its message is shown as-is.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

/// A disbursement or withdrawal larger than the cash available on (and after)
/// its date.
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
        self.client(id).map(|c| c.full_name.as_str()).unwrap_or("Unknown client")
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
        method: loan.terms.method,
        start_date: loan.terms.start_date,
        maturity_date: loan.maturity_date,
        status: loan.status,
        closed_on: match loan.status {
            ContractStatus::Defaulted | ContractStatus::Restructured => loan.status_date,
            _ => None,
        },
    }
}

fn build_book(loans: &[Loan], schedules: Vec<(i64, Installment)>, payments: &[Payment], capital: &[CapitalTx]) -> Book {
    let mut by_loan: HashMap<i64, Vec<Installment>> = HashMap::new();
    for (id, row) in schedules {
        by_loan.entry(id).or_default().push(row);
    }
    Book {
        loans: loans
            .iter()
            .map(|l| LoanBook {
                id: l.id,
                client_id: l.client_id,
                facts: loan_facts(l),
                schedule: by_loan.remove(&l.id).unwrap_or_default(),
                payments: payments.iter().filter(|p| p.loan_id == l.id && !p.is_reversed()).map(Payment::fact).collect(),
            })
            .collect(),
        capital: capital.iter().map(CapitalTx::fact).collect(),
    }
}

fn load_book(conn: &Connection) -> Result<Book> {
    let loans = loans::list(conn)?;
    let payments = payments::list(conn)?;
    let capital = capital::list(conn)?;
    Ok(build_book(&loans, loans::all_schedules(conn)?, &payments, &capital))
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

    pub fn backups_dir(&self) -> PathBuf {
        self.path.parent().and_then(Path::parent).unwrap_or(Path::new(".")).join("backups")
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let clients = clients::list(&self.conn)?;
        let loans = loans::list(&self.conn)?;
        let payments = payments::list(&self.conn)?;
        let capital = capital::list(&self.conn)?;
        let book = build_book(&loans, loans::all_schedules(&self.conn)?, &payments, &capital);
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
            return invalid("Enter the client's full name");
        }
        if name.chars().count() > 200 {
            return invalid("The name is too long");
        }
        let email = input.email.trim();
        if !email.is_empty() && (!email.contains('@') || email.contains(' ') || email.len() > 200) {
            return invalid("Enter a valid email address, or leave it empty");
        }
        let phone = input.phone.trim();
        if phone.len() > 60 || !phone.chars().all(|c| c.is_ascii_digit() || " +-().ext".contains(c)) {
            return invalid("Phone numbers may contain digits, spaces and + - ( )");
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
            clients::get(c, id)?.ok_or_else(|| anyhow!("Client not found"))?;
            clients::update(c, id, input)
        })
    }

    pub fn set_client_archived(&mut self, id: i64, archived: bool) -> Result<()> {
        self.write(|c| {
            clients::get(c, id)?.ok_or_else(|| anyhow!("Client not found"))?;
            clients::set_archived(c, id, archived)
        })
    }

    // ----- loans ----------------------------------------------------------

    pub fn create_loan(&mut self, client_id: i64, terms: &LoanTerms, notes: &str, today: NaiveDate) -> Result<i64> {
        let schedule = terms.schedule().map_err(|e| Invalid(e.to_string()))?;
        if terms.start_date > today {
            return invalid("The start date can't be in the future");
        }
        self.write(|c| {
            let client = clients::get(c, client_id)?.ok_or_else(|| Invalid("Choose a client".into()))?;
            if client.is_archived() {
                return invalid("This client is archived. Restore the client to lend again.");
            }
            let book = load_book(c)?;
            let cash = book.min_cash_from(terms.start_date);
            if terms.principal > cash {
                return Err(InsufficientCash { available: cash.max(0) }.into());
            }
            let id = loans::insert(c, client_id, terms, &schedule, notes)?;
            loans::set_status(c, id, ContractStatus::Active, terms.start_date, "Loan disbursed")?;
            Ok(id)
        })
    }

    pub fn set_loan_notes(&mut self, loan_id: i64, notes: &str) -> Result<()> {
        self.write(|c| loans::update_notes(c, loan_id, notes))
    }

    pub fn mark_defaulted(&mut self, loan_id: i64, on: NaiveDate, note: &str, today: NaiveDate) -> Result<()> {
        if on > today {
            return invalid("The default date can't be in the future");
        }
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("Loan not found"))?;
            if loan.status != ContractStatus::Active {
                return invalid("Only active loans can be marked as defaulted");
            }
            if on < loan.terms.start_date {
                return invalid("The default date is before the loan started");
            }
            if let Some(last) = payments::for_loan(c, loan_id)?.iter().filter(|p| !p.is_reversed()).map(|p| p.paid_on).max()
                && on < last
            {
                return invalid("The default date is before the latest payment");
            }
            loans::set_status(c, loan_id, ContractStatus::Defaulted, on, note)
        })
    }

    pub fn reinstate(&mut self, loan_id: i64, today: NaiveDate) -> Result<()> {
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("Loan not found"))?;
            if loan.status != ContractStatus::Defaulted {
                return invalid("Only defaulted loans can be reinstated");
            }
            loans::set_status(c, loan_id, ContractStatus::Active, today, "Reinstated")
        })
    }

    // ----- payments -------------------------------------------------------

    fn loan_and_history(c: &Connection, loan_id: i64) -> Result<(Loan, Vec<Payment>)> {
        let loan = loans::get(c, loan_id)?.ok_or_else(|| Invalid("Choose a loan".into()))?;
        let history = payments::for_loan(c, loan_id)?.into_iter().filter(|p| !p.is_reversed()).collect();
        Ok((loan, history))
    }

    pub fn preview_payment(&self, loan_id: i64, amount: Cents, fee: Cents, on: NaiveDate, today: NaiveDate) -> Result<Allocation, PaymentError> {
        let Ok((loan, history)) = Self::loan_and_history(&self.conn, loan_id) else {
            return Err(PaymentError::LoanClosed);
        };
        let facts: Vec<_> = history.iter().map(Payment::fact).collect();
        allocate(&loan_facts(&loan), &facts, amount, fee, on, today)
    }

    pub fn record_payment(&mut self, loan_id: i64, amount: Cents, fee: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        self.write(|c| {
            let (loan, history) = Self::loan_and_history(c, loan_id)?;
            let facts: Vec<_> = history.iter().map(Payment::fact).collect();
            let alloc = allocate(&loan_facts(&loan), &facts, amount, fee, on, today).map_err(|e| Invalid(payment_error_message(&e)))?;
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
            return invalid("Give a reason for the reversal");
        }
        self.write(|c| {
            let payment = payments::get(c, payment_id)?.ok_or_else(|| anyhow!("Payment not found"))?;
            if payment.is_reversed() {
                return invalid("This payment is already reversed");
            }
            let (loan, history) = Self::loan_and_history(c, payment.loan_id)?;
            if history.last().map(|p| p.id) != Some(payment_id) {
                return invalid("Only the most recent payment on a loan can be reversed. Reverse later payments first.");
            }
            if !matches!(loan.status, ContractStatus::Active | ContractStatus::PaidOff) {
                return invalid("Reinstate the loan before reversing its payments");
            }
            payments::mark_reversed(c, payment_id, reason)?;
            let book = load_book(c)?;
            if book.min_cash_from(payment.paid_on) < 0 {
                bail!(Invalid(
                    "Reversing this payment would make available cash negative, because the money was lent or withdrawn later."
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
            return invalid("Enter an amount greater than zero");
        }
        if on > today {
            return invalid("The date can't be in the future");
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
}

pub fn payment_error_message(e: &PaymentError) -> String {
    match e {
        PaymentError::Overpayment { .. } => "This is more than the payoff amount".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loans::{Frequency, InterestMethod};
    use crate::domain::money::Rate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn terms(principal: Cents, start: NaiveDate) -> LoanTerms {
        LoanTerms {
            principal,
            rate: Rate(120_000),
            method: InterestMethod::Amortized,
            frequency: Frequency::Monthly,
            term_count: 12,
            start_date: start,
        }
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
        let lid = s.create_loan(cid, &terms(1_200_000, d(2026, 1, 1)), "", today).unwrap();
        let schedule = s.snapshot().unwrap().book_loan(lid).unwrap().schedule.clone();
        assert_eq!(schedule.len(), 12);

        let pid = s.record_payment(lid, schedule[0].total(), 0, d(2026, 2, 1), "", today).unwrap();
        let snap = s.snapshot().unwrap();
        let p = snap.payments.iter().find(|p| p.id == pid).unwrap();
        assert_eq!((p.interest, p.principal), (schedule[0].interest, schedule[0].principal));
        let sum = snap.book.summary(today);
        assert_eq!(sum.interest_collected, schedule[0].interest);
        assert_eq!(sum.available_cash, 2_000_000 - 1_200_000 + schedule[0].total());

        // Reverse it: it stays on record but no longer counts.
        s.reverse_payment(pid, "Entered twice", today).unwrap();
        let snap = s.snapshot().unwrap();
        assert!(snap.payments.iter().find(|p| p.id == pid).unwrap().is_reversed());
        assert_eq!(snap.book.summary(today).interest_collected, 0);
        assert!(s.reverse_payment(pid, "again", today).is_err());

        // Pay off in full; the loan closes in the same transaction.
        let payoff = snap.book_loan(lid).unwrap().position(today).payoff();
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
        assert!(s.add_capital(CapitalKind::Withdrawal, 1, d(2026, 2, 1), "", today).is_err());
        // Backdating a loan before the capital arrived is rejected too.
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
                &Allocation { interest: 1, principal: 1, fee: 0, principal_after: 0, interest_after: 0 },
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
        // Archiving never deletes history.
        assert_eq!(s.snapshot().unwrap().clients.len(), 1);
    }

    #[test]
    fn settings_round_trip() {
        let mut s = Store::in_memory();
        let mut st = s.snapshot().unwrap().settings;
        assert_eq!(st, Settings::default());
        st.currency_code = "EUR".into();
        st.sidebar_collapsed = true;
        s.save_settings(&st).unwrap();
        assert_eq!(s.snapshot().unwrap().settings, st);
    }
}
