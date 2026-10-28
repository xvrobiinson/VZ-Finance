//! Capital entries, expenses, transfers and adjustments: creating, editing
//! and deleting them. Every change is checked against the whole ledger before
//! it is committed: no account may go below zero at any point. Loans and
//! payments are in `lending`.

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rusqlite::Connection;

use super::{Diff, InsufficientCash, Store, check_cash, invalid, load_book};
use crate::database::repositories::{
    capital,
    changes::{Action, Entity},
    entries,
};
use crate::domain::money::Cents;
use crate::domain::portfolio::{Account, CapitalKind, EntryKind};
use crate::i18n::t;
use crate::utils::dates::to_iso;

pub(super) fn not_future(on: NaiveDate, today: NaiveDate) -> Result<()> {
    if on > today {
        return invalid(t("The date can't be in the future", "La fecha no puede ser futura"));
    }
    Ok(())
}

pub(super) fn positive(amount: Cents) -> Result<()> {
    if amount <= 0 {
        return invalid(t("Enter an amount greater than zero", "Escribe un monto mayor que cero"));
    }
    Ok(())
}

/// Expenses say what they were for and income where it came from; spaces
/// alone don't count.
fn required_reason(kind: EntryKind, text: &str) -> Result<()> {
    let text = text.trim();
    match kind {
        EntryKind::Expense if text.is_empty() => invalid(t("Write what the expense was for", "Escribe en qué fue el gasto")),
        EntryKind::Income if text.is_empty() => invalid(t("Write where the income came from", "Escribe de dónde vino el ingreso")),
        _ if text.chars().count() > 2000 => {
            invalid(t("The description is too long (2,000 characters at most)", "La descripción es muy larga (máximo 2.000 caracteres)"))
        }
        _ => Ok(()),
    }
}

/// Refuses a new outflow larger than what `account` (both accounts for a
/// loan) can spare on `on` and after.
pub(super) fn ensure_available(c: &Connection, on: NaiveDate, account: Option<Account>, amount: Cents) -> Result<()> {
    let available = load_book(c)?.available_from(on, account);
    if amount > available {
        return Err(InsufficientCash { available: available.max(0), account }.into());
    }
    Ok(())
}

impl Store {
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
        self.add_record(EntryKind::Expense, account, amount, on, description, today)
    }

    /// Records money received from outside the business into `account`:
    /// into available interest it is income, into available capital it
    /// counts as capital invested. It needs no balance to exist first.
    pub fn add_income(&mut self, account: Account, amount: Cents, on: NaiveDate, source: &str, today: NaiveDate) -> Result<i64> {
        self.add_record(EntryKind::Income, account, amount, on, source, today)
    }

    /// An expense or income from New record. The same record submitted
    /// twice within a few seconds is saved once.
    fn add_record(
        &mut self,
        kind: EntryKind,
        account: Account,
        amount: Cents,
        on: NaiveDate,
        description: &str,
        today: NaiveDate,
    ) -> Result<i64> {
        positive(amount)?;
        not_future(on, today)?;
        required_reason(kind, description)?;
        self.write(|c| {
            if let Some(id) = entries::recent_duplicate(c, kind, account, amount, on, description, 10)? {
                log::warn!("{} submitted twice; kept the first", kind.key());
                return Ok(id);
            }
            if kind == EntryKind::Expense {
                ensure_available(c, on, Some(account), amount)?;
            }
            let id = entries::insert(c, kind, account, amount, on, description)?;
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
            required_reason(old.kind, description)?;
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
