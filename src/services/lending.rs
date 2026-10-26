//! Loans and payments: creating, editing and deleting them, working out every
//! payment's split, and keeping late fees on overdue installments recorded.
//! Every change is checked against the whole ledger before it is committed.

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rusqlite::Connection;

use super::records::{ensure_available, not_future};
use super::{Diff, Invalid, Store, check_cash, invalid, loan_facts, plan_for};
use crate::database::repositories::{
    changes::{self, Action, Entity, FieldChange},
    clients,
    late_fees::{self, LateFee},
    loans::{self, Loan},
    payments::{self, Payment},
    settings as settings_repo,
};
use crate::domain::installments::{self, Applied, Installment, Instruction, PlanError, PlanPayment, PlanState};
use crate::domain::ledger::{Allocation, ContractStatus, PaymentError, PaymentInput, reallocate};
use crate::domain::loans::LoanTerms;
use crate::domain::money::{Cents, MoneyFormat, RATE_SCALE, div_round_half_up};
use crate::i18n::t;
use crate::services::settings::Settings;
use crate::tr;
use crate::utils::dates::to_iso;

fn settings(c: &Connection) -> Settings {
    Settings::from_map(&settings_repo::all(c).unwrap_or_default())
}

/// Why a recorded payment no longer fits after a change, in words.
fn payment_problem(c: &Connection, p: &Payment, e: &PaymentError) -> anyhow::Error {
    let s = settings(c);
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

/// The same for an installment loan.
fn plan_problem(c: &Connection, p: &Payment, e: &PlanError) -> anyhow::Error {
    let s = settings(c);
    let (money, ds) = (s.money(), s.date_style);
    let (amount, date, why) = (money.format(p.amount), ds.format(p.paid_on), e.message(&money));
    Invalid(tr!("The payment of {amount} on {date} no longer fits: {why}", "El pago de {amount} del {date} ya no cuadra: {why}")).into()
}

/// The schedule to store for `terms`: the one given (checked), or the
/// proposed one. Returns it and whether it differs from the proposed one.
fn settle_schedule(terms: &LoanTerms, given: Option<Vec<Installment>>, money: &MoneyFormat) -> Result<(Vec<Installment>, bool)> {
    if !terms.method.has_installments() {
        return Ok((Vec::new(), false));
    }
    let facts = crate::domain::ledger::LoanFacts {
        principal: terms.principal,
        rate: terms.period_rate(),
        frequency: terms.frequency,
        start_date: terms.start_date,
        status: ContractStatus::Active,
        closed_on: None,
    };
    let proposed = installments::proposed_schedule(&facts, terms.method, terms.installments.unwrap_or(1));
    match given {
        Some(schedule) => {
            installments::check_schedule(terms.principal, terms.start_date, &schedule).map_err(|e| Invalid(e.message(money)))?;
            let edited = schedule != proposed;
            Ok((schedule, edited))
        }
        None => Ok((proposed, false)),
    }
}

/// "5|1300000": a schedule's size and total, for the change history.
fn schedule_summary(schedule: &[Installment]) -> String {
    if schedule.is_empty() {
        return String::new();
    }
    format!("{}|{}", schedule.len(), schedule.iter().map(Installment::total).sum::<Cents>())
}

/// Late fee records that differ from what the loan's payments say.
pub(super) enum FeeChange {
    Add { seq: u32, on: NaiveDate, base: Cents, amount: Cents },
    Update { old: LateFee, on: NaiveDate, base: Cents, amount: Cents },
    Remove { old: LateFee },
}

pub(super) fn fee_changes_for(stored: &[LateFee], loan: &Loan, state: &PlanState) -> Vec<FeeChange> {
    let rate = loan.terms.late_fee_rate;
    let mut out = Vec::new();
    for x in &state.installments {
        let Some(on) = x.fee_assessed_on else { continue };
        // The fee as assessed, before any waiver.
        let amount = div_round_half_up(x.fee_base as i128 * rate.0 as i128, RATE_SCALE as i128) as Cents;
        match stored.iter().find(|f| f.loan_id == loan.id && f.seq == x.seq) {
            None => out.push(FeeChange::Add { seq: x.seq, on, base: x.fee_base, amount }),
            Some(f) if (f.assessed_on, f.base_amount, f.rate, f.amount) != (on, x.fee_base, rate, amount) => {
                out.push(FeeChange::Update { old: f.clone(), on, base: x.fee_base, amount })
            }
            Some(_) => {}
        }
    }
    for f in stored.iter().filter(|f| f.loan_id == loan.id) {
        if !state.installments.iter().any(|x| x.seq == f.seq && x.fee_assessed_on.is_some()) {
            out.push(FeeChange::Remove { old: f.clone() });
        }
    }
    out
}

/// Fee changes for a loan in a snapshot (read-only check before writing).
pub(super) fn fee_changes(snap: &super::Snapshot, loan: &Loan, state: &PlanState) -> Vec<FeeChange> {
    fee_changes_for(&snap.late_fees, loan, state)
}

fn apply_fee_changes(c: &Connection, loan: &Loan, changes_: Vec<FeeChange>) -> Result<usize> {
    let n = changes_.len();
    let rate = loan.terms.late_fee_rate;
    for ch in changes_ {
        match ch {
            FeeChange::Add { seq, on, base, amount } => late_fees::upsert(c, loan.id, seq, on, base, rate, amount)?,
            FeeChange::Update { old, on, base, amount } => {
                late_fees::upsert(c, loan.id, old.seq, on, base, rate, amount)?;
                let fields = vec![
                    FieldChange { field: "amount".into(), old: old.amount.to_string(), new: amount.to_string() },
                    FieldChange { field: "fee_base".into(), old: old.base_amount.to_string(), new: base.to_string() },
                ];
                changes::insert(c, Entity::LateFee, old.seq as i64, Some(loan.id), Action::Update, &fields, "Recalculated after a change")?;
            }
            FeeChange::Remove { old } => {
                late_fees::remove(c, loan.id, old.seq)?;
                let fields = vec![
                    FieldChange { field: "amount".into(), old: old.amount.to_string(), new: String::new() },
                    FieldChange { field: "fee_base".into(), old: old.base_amount.to_string(), new: String::new() },
                ];
                changes::insert(
                    c,
                    Entity::LateFee,
                    old.seq as i64,
                    Some(loan.id),
                    Action::Delete,
                    &fields,
                    "No longer overdue after a change",
                )?;
            }
        }
    }
    Ok(n)
}

/// Brings a loan's late fee records in line with its payments as of `today`.
pub(super) fn sync_late_fees(c: &Connection, loan_id: i64, today: NaiveDate) -> Result<usize> {
    let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
    let stored = late_fees::for_loan(c, loan_id)?;
    let Some(plan) = plan_for(&loan, &stored, Vec::new()) else { return Ok(0) };
    let pays: Vec<PlanPayment> = payments::for_loan(c, loan_id)?
        .into_iter()
        .filter(|p| !p.is_reversed())
        .map(|p| PlanPayment { paid_on: p.paid_on, amount: p.amount, instruction: Instruction::Fixed(p.applied) })
        .collect();
    let Ok(state) = installments::run(&loan_facts(&loan), &plan, &pays, today, today) else { return Ok(0) };
    apply_fee_changes(c, &loan, fee_changes_for(&stored, &loan, &state))
}

fn update_paid_off_status(c: &Connection, loan: &Loan, paid_off_on: Option<NaiveDate>, today: NaiveDate) -> Result<()> {
    match (loan.status, paid_off_on) {
        (ContractStatus::Active, Some(on)) => loans::set_status(c, loan.id, ContractStatus::PaidOff, on, "Paid in full"),
        (ContractStatus::PaidOff, Some(on)) if loan.status_date != Some(on) => {
            loans::set_status(c, loan.id, ContractStatus::PaidOff, on, "Paid in full")
        }
        (ContractStatus::PaidOff, None) => loans::set_status(c, loan.id, ContractStatus::Active, today, "Reopened after a change"),
        _ => Ok(()),
    }
}

/// Works out every live payment on a loan again from the loan as it is now,
/// stores the splits, keeps late fees and the paid-off status in step.
pub(super) fn reallocate_loan(c: &Connection, loan_id: i64, today: NaiveDate) -> Result<()> {
    let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
    let live: Vec<Payment> = payments::for_loan(c, loan_id)?.into_iter().filter(|p| !p.is_reversed()).collect();
    if !loan.terms.method.has_installments() {
        let inputs: Vec<PaymentInput> = live.iter().map(|p| PaymentInput { paid_on: p.paid_on, amount: p.amount, fee: p.fee }).collect();
        let allocs = reallocate(&loan_facts(&loan), &inputs, today).map_err(|(i, e)| payment_problem(c, &live[i], &e))?;
        for (p, a) in live.iter().zip(&allocs) {
            if (p.principal, p.interest, p.fee) != (a.principal, a.interest, a.fee) {
                payments::set_allocation(c, p.id, a)?;
            }
            if !p.applied.is_empty() {
                payments::clear_split(c, p.id)?;
            }
        }
        for f in late_fees::for_loan(c, loan_id)? {
            late_fees::remove(c, loan_id, f.seq)?;
        }
        let paid_off_on = live.last().filter(|_| allocs.last().is_some_and(Allocation::pays_off)).map(|p| p.paid_on);
        return update_paid_off_status(c, &loan, paid_off_on, today);
    }
    let stored = late_fees::for_loan(c, loan_id)?;
    let plan = plan_for(&loan, &stored, Vec::new()).expect("installment loan has a plan");
    let pays: Vec<PlanPayment> =
        live.iter().map(|p| PlanPayment { paid_on: p.paid_on, amount: p.amount, instruction: p.instruction() }).collect();
    let as_of = today.max(live.last().map_or(today, |p| p.paid_on));
    let state = installments::run(&loan_facts(&loan), &plan, &pays, as_of, as_of).map_err(|(i, e)| plan_problem(c, &live[i], &e))?;
    for (p, applied) in live.iter().zip(&state.payments) {
        let totals = |f: fn(&Applied) -> Cents| applied.iter().map(f).sum::<Cents>();
        if p.applied != *applied || (p.principal, p.interest, p.fee) != (totals(|a| a.principal), totals(|a| a.interest), totals(|a| a.fee))
        {
            payments::set_split(c, p.id, applied)?;
        }
    }
    apply_fee_changes(c, &loan, fee_changes_for(&stored, &loan, &state))?;
    let paid_off_on = state.paid_off().then(|| live.last().map(|p| p.paid_on)).flatten();
    update_paid_off_status(c, &loan, paid_off_on, today)
}

impl Store {
    // ----- loans ----------------------------------------------------------

    /// Creates a loan with its proposed schedule (installment loans).
    pub fn create_loan(&mut self, client_id: i64, terms: &LoanTerms, notes: &str, today: NaiveDate) -> Result<i64> {
        self.create_loan_with_schedule(client_id, terms, None, notes, today)
    }

    /// Creates a loan. For installment loans `schedule` is the reviewed
    /// schedule, possibly edited by hand; `None` uses the proposed one.
    pub fn create_loan_with_schedule(
        &mut self,
        client_id: i64,
        terms: &LoanTerms,
        schedule: Option<Vec<Installment>>,
        notes: &str,
        today: NaiveDate,
    ) -> Result<i64> {
        terms.validate().map_err(|e| Invalid(e.message().into()))?;
        if terms.start_date > today {
            return invalid(t("The loan date can't be in the future", "La fecha del préstamo no puede ser futura"));
        }
        let money = settings(&self.conn).money();
        let (schedule, edited) = settle_schedule(terms, schedule, &money)?;
        self.write(|c| {
            let client = clients::get(c, client_id)?.ok_or_else(|| Invalid(t("Choose a client", "Elige un cliente").into()))?;
            if client.is_archived() {
                return invalid(t(
                    "This client is archived. Restore the client to lend again.",
                    "Este cliente está archivado. Restáuralo para volver a prestarle.",
                ));
            }
            ensure_available(c, terms.start_date, None, terms.principal)?;
            let id = loans::insert(c, client_id, terms, &schedule, edited, notes)?;
            loans::set_status(c, id, ContractStatus::Active, terms.start_date, "Loan disbursed")?;
            check_cash(c)?;
            Ok(id)
        })
    }

    /// Changes a loan's client, terms, schedule or notes. Every payment on it
    /// is worked out again from the new terms. With `schedule` `None`, an
    /// installment loan keeps its stored schedule when the terms that shape it
    /// are unchanged, and gets the proposed one otherwise.
    #[allow(clippy::too_many_arguments)]
    pub fn update_loan(
        &mut self,
        loan_id: i64,
        client_id: i64,
        terms: &LoanTerms,
        schedule: Option<Vec<Installment>>,
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
            let shaping_unchanged = LoanTerms { late_fee_rate: old.terms.late_fee_rate, ..terms.clone() } == old.terms;
            let (schedule, edited) = match schedule {
                None if shaping_unchanged && terms.method.has_installments() => (old.schedule.clone(), old.schedule_edited),
                given => settle_schedule(terms, given, &settings(c).money())?,
            };
            let diff = Diff::default()
                .field("client", old.client_id, client_id)
                .field("principal", old.terms.principal, terms.principal)
                .field("rate", old.terms.rate.0, terms.rate.0)
                .field("interest_period", old.terms.interest_period.key(), terms.interest_period.key())
                .field("frequency", old.terms.frequency.key(), terms.frequency.key())
                .field("method", old.terms.method.key(), terms.method.key())
                .field(
                    "installments",
                    old.terms.installments.map(|n| n.to_string()).unwrap_or_default(),
                    terms.installments.map(|n| n.to_string()).unwrap_or_default(),
                )
                .field("late_fee_rate", old.terms.late_fee_rate.0, terms.late_fee_rate.0)
                .field("start_date", to_iso(old.terms.start_date), to_iso(terms.start_date))
                .field("schedule", schedule_summary(&old.schedule), schedule_summary(&schedule))
                .field("schedule_edited", old.schedule_edited, edited)
                .field("notes", &old.notes, notes.trim());
            if diff.is_empty() && schedule == old.schedule {
                return Ok(());
            }
            loans::update(c, loan_id, client_id, terms, &schedule, edited, notes)?;
            reallocate_loan(c, loan_id, today)?;
            check_cash(c)?;
            diff.log(c, Entity::Loan, loan_id, Some(loan_id), Action::Update, reason)
        })
    }

    /// Deletes a loan together with its payments, schedule and late fees.
    pub fn delete_loan(&mut self, loan_id: i64, reason: &str) -> Result<()> {
        self.write(|c| {
            let old = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            let paid: Vec<Payment> = payments::for_loan(c, loan_id)?.into_iter().filter(|p| !p.is_reversed()).collect();
            Diff::default()
                .removed("client", old.client_id)
                .removed("principal", old.terms.principal)
                .removed("rate", old.terms.rate.0)
                .removed("interest_period", old.terms.interest_period.key())
                .removed("frequency", old.terms.frequency.key())
                .removed("method", old.terms.method.key())
                .removed("schedule", schedule_summary(&old.schedule))
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
            loans::set_status(c, loan_id, ContractStatus::Defaulted, on, note)?;
            sync_late_fees(c, loan_id, today)?;
            Ok(())
        })
    }

    pub fn reinstate(&mut self, loan_id: i64, today: NaiveDate) -> Result<()> {
        self.write(|c| {
            let loan = loans::get(c, loan_id)?.ok_or_else(|| anyhow!("loan not found"))?;
            if loan.status != ContractStatus::Defaulted {
                return invalid(t("Only defaulted loans can be reinstated", "Solo los préstamos incobrables pueden reactivarse"));
            }
            loans::set_status(c, loan_id, ContractStatus::Active, today, "Reinstated")?;
            sync_late_fees(c, loan_id, today)?;
            Ok(())
        })
    }

    /// Waives (or restores) the late fee on one installment. Payments on the
    /// loan are worked out again.
    pub fn set_late_fee_waived(&mut self, loan_id: i64, seq: u32, waived: bool, reason: &str, today: NaiveDate) -> Result<()> {
        self.write(|c| {
            let fee = late_fees::for_loan(c, loan_id)?
                .into_iter()
                .find(|f| f.seq == seq)
                .ok_or_else(|| Invalid(t("There is no late fee on that installment", "Esa cuota no tiene cargo por mora").into()))?;
            if fee.is_waived() == waived {
                return Ok(());
            }
            late_fees::set_waived(c, loan_id, seq, waived, reason)?;
            let fields = vec![FieldChange { field: "waived".into(), old: (!waived).to_string(), new: waived.to_string() }];
            changes::insert(c, Entity::LateFee, seq as i64, Some(loan_id), Action::Update, &fields, reason)?;
            reallocate_loan(c, loan_id, today)?;
            check_cash(c)
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

    fn active_loan(c: &Connection, loan_id: i64) -> Result<Loan> {
        let loan = loans::get(c, loan_id)?.ok_or_else(|| Invalid(t("Choose a loan", "Elige un préstamo").into()))?;
        if loan.status != ContractStatus::Active {
            return invalid(PaymentError::LoanClosed.message());
        }
        Ok(loan)
    }

    /// Records a payment on an active loan, applied oldest first. It may be
    /// dated before payments already recorded; every split on the loan is
    /// then worked out again. `fee` is an extra fee (interest-only loans;
    /// installment loans charge late fees by themselves).
    pub fn record_payment(&mut self, loan_id: i64, amount: Cents, fee: Cents, on: NaiveDate, notes: &str, today: NaiveDate) -> Result<i64> {
        Self::check_payment_amounts(amount, fee)?;
        not_future(on, today)?;
        self.write(|c| {
            let loan = Self::active_loan(c, loan_id)?;
            if loan.terms.method.has_installments() && fee > 0 {
                return invalid(t(
                    "Late fees on installment loans are charged automatically",
                    "En préstamos por cuotas los cargos por mora se calculan solos",
                ));
            }
            let placeholder =
                Allocation { fee, interest: 0, principal: amount - fee, principal_before: 0, principal_after: 0, interest_after: 0 };
            let id = payments::insert(c, loan_id, on, amount, &placeholder, notes)?;
            reallocate_loan(c, loan_id, today)?;
            check_cash(c)?;
            Ok(id)
        })
    }

    /// Records a payment on an installment loan, applied as `instruction` says.
    pub fn record_installment_payment(
        &mut self,
        loan_id: i64,
        amount: Cents,
        on: NaiveDate,
        notes: &str,
        instruction: &Instruction,
        today: NaiveDate,
    ) -> Result<i64> {
        Self::check_payment_amounts(amount, 0)?;
        not_future(on, today)?;
        self.write(|c| {
            let loan = Self::active_loan(c, loan_id)?;
            if !loan.terms.method.has_installments() {
                return invalid(t("This loan has no installments", "Este préstamo no tiene cuotas"));
            }
            let placeholder =
                Allocation { fee: 0, interest: 0, principal: amount, principal_before: 0, principal_after: 0, interest_after: 0 };
            let id = payments::insert(c, loan_id, on, amount, &placeholder, notes)?;
            payments::set_instruction(c, id, instruction)?;
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
            let instruction = old.instruction();
            payments::update(c, payment_id, on, amount, fee, notes)?;
            if old.mode != payments::AllocationMode::Auto {
                payments::set_instruction(c, payment_id, &instruction)?;
            }
            reallocate_loan(c, old.loan_id, today)?;
            check_cash(c)?;
            diff.log(c, Entity::Payment, payment_id, Some(old.loan_id), Action::Update, reason)
        })
    }

    /// Changes a payment on an installment loan, including how it is applied.
    #[allow(clippy::too_many_arguments)]
    pub fn update_installment_payment(
        &mut self,
        payment_id: i64,
        amount: Cents,
        on: NaiveDate,
        notes: &str,
        instruction: &Instruction,
        reason: &str,
        today: NaiveDate,
    ) -> Result<()> {
        Self::check_payment_amounts(amount, 0)?;
        not_future(on, today)?;
        self.write(|c| {
            let old = payments::get(c, payment_id)?.ok_or_else(|| anyhow!("payment not found"))?;
            if old.is_reversed() {
                return invalid(t("A reversed payment can only be deleted", "Un pago anulado solo se puede eliminar"));
            }
            let describe = |i: &Instruction| match i {
                Instruction::Auto | Instruction::Fixed(_) => "auto".to_string(),
                Instruction::Installment(seq) => format!("installment {seq}"),
                Instruction::Spread(parts) => {
                    format!("spread {}", parts.iter().map(|(s, a)| format!("{s}:{a}")).collect::<Vec<_>>().join(","))
                }
                Instruction::Custom { principal, interest, fee } => format!("custom {principal}/{interest}/{fee}"),
            };
            let diff = Diff::default()
                .field("amount", old.amount, amount)
                .field("paid_on", to_iso(old.paid_on), to_iso(on))
                .field("allocation", describe(&old.instruction()), describe(instruction))
                .field("notes", &old.notes, notes.trim());
            if diff.is_empty() {
                return Ok(());
            }
            payments::update(c, payment_id, on, amount, 0, notes)?;
            payments::set_instruction(c, payment_id, instruction)?;
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
}
