//! Loans, payments, capital and cash entries: creating, editing and deleting
//! them. Every change is checked against the whole ledger before it is
//! committed: payment splits are recalculated and no account may go below
//! zero at any point.

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rusqlite::Connection;

use super::{Diff, InsufficientCash, Invalid, Store, check_cash, invalid, load_book, loan_facts};
use crate::database::repositories::{
    capital,
    changes::{Action, Entity},
    clients, entries, loans,
    payments::{self, Payment},
    settings as settings_repo,
};
use crate::domain::ledger::{Allocation, ContractStatus, PaymentError, PaymentInput, reallocate};
use crate::domain::loans::LoanTerms;
use crate::domain::money::Cents;
use crate::domain::portfolio::{Account, CapitalKind, EntryKind};
use crate::i18n::t;
use crate::services::settings::Settings;
use crate::tr;
use crate::utils::dates::to_iso;

fn not_future(on: NaiveDate, today: NaiveDate) -> Result<()> {
    if on > today {
        return invalid(t("The date can't be in the future", "La fecha no puede ser futura"));
    }
    Ok(())
}

fn positive(amount: Cents) -> Result<()> {
    if amount <= 0 {
        return invalid(t("Enter an amount greater than zero", "Escribe un monto mayor que cero"));
    }
    Ok(())
}

/// Refuses a new outflow larger than what `account` (both accounts for a
/// loan) can spare on `on` and after.
fn ensure_available(c: &Connection, on: NaiveDate, account: Option<Account>, amount: Cents) -> Result<()> {
    let available = load_book(c)?.available_from(on, account);
    if amount > available {
        return Err(InsufficientCash { available: available.max(0), account }.into());
    }
    Ok(())
}

/// Why a recorded payment no longer fits after a change, in words.
fn payment_problem(c: &Connection, p: &Payment, e: &PaymentError) -> anyhow::Error {
    let s = Settings::from_map(&settings_repo::all(c).unwrap_or_default());
    let (money, ds) = (s.money(), s.date_style);
    let (amount, date) = (money.format(p.amount), ds.format(p.paid_on));
    let msg = match e {
        PaymentError::Overpayment { payoff } => {
            let owed = money.format(*payoff);
            tr!(
                "The payment of {amount} on {date} would be more than the client owed then ({owed}).",
                "El pago de {amount} del {date} sería más de lo que el cliente debía entonces ({owed})."
            )
        }
        PaymentError::BeforeStart => tr!(
            "The payment of {amount} on {date} would be before the loan date.",
            "El pago de {amount} del {date} quedaría antes de la fecha del préstamo."
        ),
        PaymentError::LoanClosed => tr!(
            "The payment of {amount} on {date} is after the loan was marked as defaulted.",
            "El pago de {amount} del {date} es posterior a cuando el préstamo se marcó incobrable."
        ),
        other => other.message().to_string(),
    };
    Invalid(msg).into()
}

/// Splits every live payment on a loan again from the loan as it is now,
/// stores the new splits and keeps the paid-off status in step.
fn reallocate_loan(c: &Connection, loan_id: i64, today: NaiveDate) -> Result<()> {
    let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
    let live: Vec<Payment> = payments::for_loan(c, loan_id)?.into_iter().filter(|p| !p.is_reversed()).collect();
    let inputs: Vec<PaymentInput> = live.iter().map(|p| PaymentInput { paid_on: p.paid_on, amount: p.amount, fee: p.fee }).collect();
    let allocs = reallocate(&loan_facts(&loan), &inputs, today).map_err(|(i, e)| payment_problem(c, &live[i], &e))?;
    for (p, a) in live.iter().zip(&allocs) {
        if (p.principal, p.interest, p.fee) != (a.principal, a.interest, a.fee) {
            payments::set_allocation(c, p.id, a)?;
        }
    }
    let paid_off_on = live.last().filter(|_| allocs.last().is_some_and(Allocation::pays_off)).map(|p| p.paid_on);
    match (loan.status, paid_off_on) {
        (ContractStatus::Active, Some(on)) => loans::set_status(c, loan_id, ContractStatus::PaidOff, on, "Paid in full")?,
        (ContractStatus::PaidOff, Some(on)) if loan.status_date != Some(on) => {
            loans::set_status(c, loan_id, ContractStatus::PaidOff, on, "Paid in full")?
        }
        (ContractStatus::PaidOff, None) => loans::set_status(c, loan_id, ContractStatus::Active, today, "Reopened after a change")?,
        _ => {}
    }
    Ok(())
}

impl Store {
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
            ensure_available(c, terms.start_date, None, terms.principal)?;
            let id = loans::insert(c, client_id, terms, notes)?;
            loans::set_status(c, id, ContractStatus::Active, terms.start_date, "Loan disbursed")?;
            check_cash(c)?;
            Ok(id)
        })
    }

    /// Changes a loan's client, amount, rate, frequency, date or notes. Every
    /// payment on it is split again from the new terms.
    pub fn update_loan(
        &mut self,
        loan_id: i64,
        client_id: i64,
        terms: &LoanTerms,
        notes: &str,
        reason: &str,
        today: NaiveDate,
    ) -> Result<()> {
        terms.validate().map_err(|e| Invalid(e.message().into()))?;
        if terms.start_date > today {
            return invalid(t("The loan date can't be in the future", "La fecha del préstamo no puede ser futura"));
        }
        self.write(|c| {
            let old = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            let client = clients::get(c, client_id)?.ok_or_else(|| Invalid(t("Choose a client", "Elige un cliente").into()))?;
            if client_id != old.client_id && client.is_archived() {
                return invalid(t(
                    "This client is archived. Restore the client first.",
                    "Este cliente está archivado. Restáuralo primero.",
                ));
            }
            let diff = Diff::default()
                .field("client", old.client_id, client_id)
                .field("principal", old.terms.principal, terms.principal)
                .field("rate", old.terms.rate.0, terms.rate.0)
                .field("frequency", old.terms.frequency.key(), terms.frequency.key())
                .field("start_date", to_iso(old.terms.start_date), to_iso(terms.start_date))
                .field("notes", &old.notes, notes.trim());
            if diff.is_empty() {
                return Ok(());
            }
            loans::update(c, loan_id, client_id, terms, notes)?;
            reallocate_loan(c, loan_id, today)?;
            check_cash(c)?;
            diff.log(c, Entity::Loan, loan_id, Some(loan_id), Action::Update, reason)
        })
    }

    /// Deletes a loan together with its payments.
    pub fn delete_loan(&mut self, loan_id: i64, reason: &str) -> Result<()> {
        self.write(|c| {
            let old = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            let paid: Vec<Payment> = payments::for_loan(c, loan_id)?.into_iter().filter(|p| !p.is_reversed()).collect();
            Diff::default()
                .removed("client", old.client_id)
                .removed("principal", old.terms.principal)
                .removed("rate", old.terms.rate.0)
                .removed("frequency", old.terms.frequency.key())
                .removed("start_date", to_iso(old.terms.start_date))
                .removed("payments", paid.len())
                .removed("received", paid.iter().map(|p| p.amount).sum::<Cents>())
                .removed("notes", &old.notes)
                .log(c, Entity::Loan, loan_id, Some(loan_id), Action::Delete, reason)?;
            payments::delete_for_loan(c, loan_id)?;
            loans::delete(c, loan_id)?;
            check_cash(c)
        })
    }

    pub fn mark_defaulted(&mut self, loan_id: i64, on: NaiveDate, note: &str, today: NaiveDate) -> Result<()> {
        not_future(on, today)?;
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            if loan.status != ContractStatus::Active {
                return invalid(t(
                    "Only active loans can be marked as defaulted",
                    "Solo los préstamos activos pueden marcarse como incobrables",
                ));
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

    fn check_payment_amounts(amount: Cents, fee: Cents) -> Result<()> {
        if amount <= 0 {
            return invalid(PaymentError::NotPositive.message());
        }
        if fee < 0 || fee > amount {
            return invalid(PaymentError::FeeTooLarge.message());
        }
        Ok(())
    }

    /// Records a payment on an active loan. It may be dated before payments
    /// already recorded; every split on the loan is then worked out again.
    pub fn record_payment(&mut self, loan_id: i64, amount: Cents, fee: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        Self::check_payment_amounts(amount, fee)?;
        not_future(on, today)?;
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| Invalid(t("Choose a loan", "Elige un préstamo").into()))?;
            if loan.status != ContractStatus::Active {
                return invalid(PaymentError::LoanClosed.message());
            }
            let placeholder =
                Allocation { fee, interest: 0, principal: amount - fee, principal_before: 0, principal_after: 0, interest_after: 0 };
            let id = payments::insert(c, loan_id, on, amount, &placeholder, notes)?;
            reallocate_loan(c, loan_id, today)?;
            check_cash(c)?;
            Ok(id)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_payment(
        &mut self,
        payment_id: i64,
        amount: Cents,
        fee: Cents,
        on: NaiveDate,
        notes: &str,
        reason: &str,
        today: NaiveDate,
    ) -> Result<()> {
        Self::check_payment_amounts(amount, fee)?;
        not_future(on, today)?;
        self.write(|c| {
            let old = payments::get(c, payment_id)?.ok_or_else(|| anyhow!("payment not found"))?;
            if old.is_reversed() {
                return invalid(t("A reversed payment can only be deleted", "Un pago anulado solo se puede eliminar"));
            }
            let diff = Diff::default()
                .field("amount", old.amount, amount)
                .field("fee", old.fee, fee)
                .field("paid_on", to_iso(old.paid_on), to_iso(on))
                .field("notes", &old.notes, notes.trim());
            if diff.is_empty() {
                return Ok(());
            }
            payments::update(c, payment_id, on, amount, fee, notes)?;
            reallocate_loan(c, old.loan_id, today)?;
            check_cash(c)?;
            diff.log(c, Entity::Payment, payment_id, Some(old.loan_id), Action::Update, reason)
        })
    }

    pub fn delete_payment(&mut self, payment_id: i64, reason: &str, today: NaiveDate) -> Result<()> {
        self.write(|c| {
            let old = payments::get(c, payment_id)?.ok_or_else(|| anyhow!("payment not found"))?;
            Diff::default()
                .removed("amount", old.amount)
                .removed("interest", old.interest)
                .removed("principal", old.principal)
                .removed("fee", old.fee)
                .removed("paid_on", to_iso(old.paid_on))
                .removed("notes", &old.notes)
                .log(c, Entity::Payment, payment_id, Some(old.loan_id), Action::Delete, reason)?;
            payments::delete(c, payment_id)?;
            reallocate_loan(c, old.loan_id, today)?;
            check_cash(c)
        })
    }

    // ----- capital --------------------------------------------------------

    pub fn add_capital(&mut self, kind: CapitalKind, amount: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        positive(amount)?;
        not_future(on, today)?;
        self.write(|c| {
            if kind == CapitalKind::Withdrawal {
                ensure_available(c, on, Some(Account::Capital), amount)?;
            }
            let id = capital::insert(c, kind, amount, on, notes)?;
            check_cash(c)?;
            Ok(id)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_capital(
        &mut self,
        id: i64,
        kind: CapitalKind,
        amount: Cents,
        on: NaiveDate,
        notes: &str,
        reason: &str,
        today: NaiveDate,
    ) -> Result<()> {
        positive(amount)?;
        not_future(on, today)?;
        self.write(|c| {
            let old = capital::get(c, id)?.ok_or_else(|| anyhow!("capital entry not found"))?;
            let diff = Diff::default()
                .field("kind", old.kind.key(), kind.key())
                .field("amount", old.amount, amount)
                .field("occurred_on", to_iso(old.occurred_on), to_iso(on))
                .field("notes", &old.notes, notes.trim());
            if diff.is_empty() {
                return Ok(());
            }
            capital::update(c, id, kind, amount, on, notes)?;
            check_cash(c)?;
            diff.log(c, Entity::Capital, id, None, Action::Update, reason)
        })
    }

    pub fn delete_capital(&mut self, id: i64, reason: &str) -> Result<()> {
        self.write(|c| {
            let old = capital::get(c, id)?.ok_or_else(|| anyhow!("capital entry not found"))?;
            Diff::default()
                .removed("kind", old.kind.key())
                .removed("amount", old.amount)
                .removed("occurred_on", to_iso(old.occurred_on))
                .removed("notes", &old.notes)
                .log(c, Entity::Capital, id, None, Action::Delete, reason)?;
            capital::delete(c, id)?;
            check_cash(c)
        })
    }

    // ----- expenses, transfers and adjustments ----------------------------

    /// Records money spent outside the business, paid from `account`.
    pub fn add_expense(&mut self, account: Account, amount: Cents, on: NaiveDate, description: &str, today: NaiveDate) -> Result<i64> {
        positive(amount)?;
        not_future(on, today)?;
        if description.trim().is_empty() {
            return invalid(t("Write what the expense was for", "Escribe en qué fue el gasto"));
        }
        self.write(|c| {
            ensure_available(c, on, Some(account), amount)?;
            let id = entries::insert(c, EntryKind::Expense, account, amount, on, description)?;
            check_cash(c)?;
            Ok(id)
        })
    }

    /// Moves money from `from` to the other account.
    pub fn add_transfer(&mut self, from: Account, amount: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        positive(amount)?;
        not_future(on, today)?;
        self.write(|c| {
            ensure_available(c, on, Some(from), amount)?;
            let id = entries::insert(c, EntryKind::Transfer, from, amount, on, notes)?;
            check_cash(c)?;
            Ok(id)
        })
    }

    /// Sets the two accounts to the amounts really on hand at the end of
    /// `on`, recording an adjustment for each difference. Returns how many
    /// adjustments were recorded.
    pub fn adjust_balances(&mut self, capital: Cents, interest: Cents, on: NaiveDate, reason: &str, today: NaiveDate) -> Result<usize> {
        not_future(on, today)?;
        if capital < 0 || interest < 0 {
            return invalid(t("Balances can't be below zero", "Los saldos no pueden ser negativos"));
        }
        if reason.trim().is_empty() {
            return invalid(t("Give a reason for the adjustment", "Indica el motivo del ajuste"));
        }
        self.write(|c| {
            let now = load_book(c)?.balances_on(on);
            let mut made = 0;
            for (account, target) in [(Account::Capital, capital), (Account::Interest, interest)] {
                let diff = target - now.get(account);
                if diff != 0 {
                    entries::insert(c, EntryKind::Adjustment, account, diff, on, reason)?;
                    made += 1;
                }
            }
            if made == 0 {
                return invalid(t("Nothing to adjust: the amounts already match.", "No hay nada que ajustar: los montos ya coinciden."));
            }
            check_cash(c)?;
            Ok(made)
        })
    }

    /// Edits an expense, transfer or adjustment. Its kind stays the same; an
    /// adjustment's amount is signed.
    #[allow(clippy::too_many_arguments)]
    pub fn update_entry(
        &mut self,
        id: i64,
        account: Account,
        amount: Cents,
        on: NaiveDate,
        description: &str,
        reason: &str,
        today: NaiveDate,
    ) -> Result<()> {
        not_future(on, today)?;
        self.write(|c| {
            let old = entries::get(c, id)?.ok_or_else(|| anyhow!("entry not found"))?;
            match old.kind {
                EntryKind::Adjustment if amount == 0 => {
                    return invalid(t("Enter an amount other than zero", "Escribe un monto distinto de cero"));
                }
                EntryKind::Adjustment => {}
                _ => positive(amount)?,
            }
            if old.kind == EntryKind::Expense && description.trim().is_empty() {
                return invalid(t("Write what the expense was for", "Escribe en qué fue el gasto"));
            }
            let diff = Diff::default()
                .field("account", old.account.key(), account.key())
                .field("amount", old.amount, amount)
                .field("occurred_on", to_iso(old.occurred_on), to_iso(on))
                .field("description", &old.description, description.trim());
            if diff.is_empty() {
                return Ok(());
            }
            entries::update(c, id, account, amount, on, description)?;
            check_cash(c)?;
            diff.log(c, Entity::Entry, id, None, Action::Update, reason)
        })
    }

    pub fn delete_entry(&mut self, id: i64, reason: &str) -> Result<()> {
        self.write(|c| {
            let old = entries::get(c, id)?.ok_or_else(|| anyhow!("entry not found"))?;
            Diff::default()
                .removed("kind", old.kind.key())
                .removed("account", old.account.key())
                .removed("amount", old.amount)
                .removed("occurred_on", to_iso(old.occurred_on))
                .removed("description", &old.description)
                .log(c, Entity::Entry, id, None, Action::Delete, reason)?;
            entries::delete(c, id)?;
            check_cash(c)
        })
    }
}
