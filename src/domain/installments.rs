//! Installment loans: a fixed number of installments, each repaying part of
//! the capital plus the period's interest, with a late fee charged once on an
//! installment that is still unpaid after its due date.
//!
//! Rules (see `docs/ACCOUNTING.md`):
//! - Installment `k`'s period runs from the previous due date (or the loan
//!   date) to its own due date. Its interest is charged when the period
//!   begins: on the capital still owed then (declining balance), on the
//!   original principal (flat), or as written when the schedule was edited.
//!   No interest is charged once the capital is repaid.
//! - A payment dated on a due date belongs to the period ending that day.
//! - The day after its due date, an installment with capital or interest
//!   still unpaid is charged a late fee of `fee_rate` × that unpaid amount,
//!   once. Later payments do not change it; it can be waived.
//! - Payments are applied by one of four instructions: oldest first
//!   (late fee, interest, capital of each installment in turn), to one
//!   installment, spread with chosen amounts per installment, or a custom
//!   split between capital, interest and late fees, each oldest first.
//!   Nothing is applied beyond what is owed.

use chrono::NaiveDate;

use crate::domain::ledger::{ContractStatus, Delinquency, LoanFacts, Period, Position};
use crate::domain::loans::Method;
use crate::domain::money::{Cents, MoneyFormat, RATE_SCALE, Rate, div_round_half_up};
use crate::i18n::t;
use crate::tr;

/// One scheduled installment: its due date, the capital it repays and its
/// interest as scheduled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installment {
    pub seq: u32,
    pub due: NaiveDate,
    pub principal: Cents,
    pub interest: Cents,
}

impl Installment {
    pub fn total(&self) -> Cents {
        self.principal + self.interest
    }
}

/// The schedule for new terms, assuming every installment is paid on time:
/// equal capital each time (the last takes the rounding remainder), interest
/// on the capital owed when each period begins (declining) or on the
/// original principal (flat).
pub fn proposed_schedule(facts: &LoanFacts, method: Method, count: u32) -> Vec<Installment> {
    let count = count.max(1);
    let base = facts.principal / count as i64;
    let mut balance = facts.principal;
    (1..=count)
        .map(|k| {
            let principal = if k == count { balance } else { base };
            let interest = match method {
                Method::Flat => facts.rate.interest(facts.principal),
                _ => facts.rate.interest(balance),
            };
            balance -= principal;
            Installment { seq: k, due: facts.frequency.due_date(facts.start_date, k), principal, interest }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScheduleError {
    Empty,
    /// The installments' capital doesn't add up to the principal.
    Unallocated {
        allocated: Cents,
        principal: Cents,
    },
    Negative(u32),
    /// A due date is not after the previous one (or the loan date).
    DateOrder(u32),
}

impl ScheduleError {
    pub fn message(&self, money: &MoneyFormat) -> String {
        match self {
            ScheduleError::Empty => t("Add at least one installment", "Agrega al menos una cuota").into(),
            ScheduleError::Unallocated { allocated, principal } => {
                let (a, p) = (money.format(*allocated), money.format(*principal));
                let diff = money.format(principal - allocated);
                tr!(
                    "The installments repay {a} of capital, not {p} ({diff} difference).",
                    "Las cuotas pagan {a} de capital, no {p} (diferencia de {diff})."
                )
            }
            ScheduleError::Negative(k) => tr!("Installment {k} has a negative amount", "La cuota {k} tiene un monto negativo"),
            ScheduleError::DateOrder(k) => tr!(
                "Installment {k} must be due after the one before it (and after the loan date)",
                "La cuota {k} debe vencer después de la anterior (y después de la fecha del préstamo)"
            ),
        }
    }
}

/// Checks an edited schedule: capital adds up to the principal, nothing is
/// negative and due dates move forward.
pub fn check_schedule(principal: Cents, start: NaiveDate, schedule: &[Installment]) -> Result<(), ScheduleError> {
    if schedule.is_empty() {
        return Err(ScheduleError::Empty);
    }
    let mut prev = start;
    for x in schedule {
        if x.principal < 0 || x.interest < 0 {
            return Err(ScheduleError::Negative(x.seq));
        }
        if x.due <= prev {
            return Err(ScheduleError::DateOrder(x.seq));
        }
        prev = x.due;
    }
    let allocated: Cents = schedule.iter().map(|x| x.principal).sum();
    if allocated != principal {
        return Err(ScheduleError::Unallocated { allocated, principal });
    }
    Ok(())
}

/// One part of what is owed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Fee,
    Interest,
    Principal,
}

impl Part {
    pub fn label(self) -> &'static str {
        match self {
            Part::Fee => t("late fees", "cargos por mora"),
            Part::Interest => t("interest", "interés"),
            Part::Principal => t("capital", "capital"),
        }
    }
}

/// What part of a payment went to one installment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub seq: u32,
    pub principal: Cents,
    pub interest: Cents,
    pub fee: Cents,
}

impl Applied {
    pub fn total(&self) -> Cents {
        self.principal + self.interest + self.fee
    }
}

/// How a payment is applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction {
    /// Oldest first: late fee, interest, then capital of each installment.
    Auto,
    /// To one installment only, which may be left partly paid.
    Installment(u32),
    /// Chosen amounts per installment.
    Spread(Vec<(u32, Cents)>),
    /// Chosen amounts to capital, interest and late fees, each oldest first.
    Custom { principal: Cents, interest: Cents, fee: Cents },
    /// Splits already worked out and stored, replayed as recorded.
    Fixed(Vec<Applied>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanPayment {
    pub paid_on: NaiveDate,
    pub amount: Cents,
    pub instruction: Instruction,
}

/// An installment loan's schedule and recorded payments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub method: Method,
    pub schedule: Vec<Installment>,
    /// Edited by hand: the scheduled interest is used as written.
    pub edited: bool,
    pub fee_rate: Rate,
    /// Installments whose late fee was waived.
    pub waived: Vec<u32>,
    /// Recorded payments in ledger order, with their stored splits.
    pub payments: Vec<PlanPayment>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallmentState {
    pub seq: u32,
    pub starts: NaiveDate,
    pub due: NaiveDate,
    /// Capital this installment repays.
    pub principal: Cents,
    /// Interest charged when its period began, or projected before that.
    pub interest: Cents,
    pub charged: bool,
    /// Capital owed when its period began (once charged).
    pub balance_at_start: Cents,
    pub fee: Cents,
    /// What was unpaid when the late fee was assessed.
    pub fee_base: Cents,
    pub fee_assessed_on: Option<NaiveDate>,
    pub fee_waived: bool,
    pub paid_principal: Cents,
    pub paid_interest: Cents,
    pub paid_fee: Cents,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstallmentStatus {
    Overdue,
    PartlyPaid,
    Due,
    Upcoming,
    Paid,
}

impl InstallmentStatus {
    pub fn label(self) -> &'static str {
        match self {
            InstallmentStatus::Paid => t("Paid", "Pagada"),
            InstallmentStatus::PartlyPaid => t("Partly paid", "Pago parcial"),
            InstallmentStatus::Overdue => t("Overdue", "Atrasada"),
            InstallmentStatus::Due => t("Current", "En curso"),
            InstallmentStatus::Upcoming => t("Upcoming", "Próxima"),
        }
    }
}

impl InstallmentState {
    pub fn unpaid_principal(&self) -> Cents {
        self.principal - self.paid_principal
    }
    /// Interest owed now (none before the period begins).
    pub fn unpaid_interest(&self) -> Cents {
        if self.charged { self.interest - self.paid_interest } else { 0 }
    }
    pub fn unpaid_fee(&self) -> Cents {
        self.fee - self.paid_fee
    }
    /// Everything owed on it right now.
    pub fn owed(&self) -> Cents {
        self.unpaid_principal() + self.unpaid_interest() + self.unpaid_fee()
    }
    /// Its full amount: capital, interest (charged or projected) and late fee.
    pub fn total_due(&self) -> Cents {
        self.principal + self.interest + self.fee
    }
    pub fn paid(&self) -> Cents {
        self.paid_principal + self.paid_interest + self.paid_fee
    }
    /// What remains of its full amount.
    pub fn remaining(&self) -> Cents {
        self.total_due() - self.paid()
    }
    fn unpaid(&self, part: Part) -> Cents {
        match part {
            Part::Fee => self.unpaid_fee(),
            Part::Interest => self.unpaid_interest(),
            Part::Principal => self.unpaid_principal(),
        }
    }
    fn pay(&mut self, part: Part, amount: Cents) {
        match part {
            Part::Fee => self.paid_fee += amount,
            Part::Interest => self.paid_interest += amount,
            Part::Principal => self.paid_principal += amount,
        }
    }
    pub fn status(&self, today: NaiveDate) -> InstallmentStatus {
        if self.remaining() <= 0 {
            InstallmentStatus::Paid
        } else if self.due < today {
            InstallmentStatus::Overdue
        } else if self.paid() > 0 {
            InstallmentStatus::PartlyPaid
        } else if self.starts <= today {
            InstallmentStatus::Due
        } else {
            InstallmentStatus::Upcoming
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    NotPositive,
    InFuture,
    BeforeStart,
    LoanClosed,
    /// More than everything owed now.
    Overpayment {
        payoff: Cents,
    },
    /// More than one installment owes now.
    ExceedsInstallment {
        seq: u32,
        owed: Cents,
    },
    NoSuchInstallment(u32),
    /// Chosen amounts don't add up to the payment.
    SplitMismatch {
        allocated: Cents,
        amount: Cents,
    },
    /// More of one part than is owed.
    ExceedsPart {
        part: Part,
        owed: Cents,
    },
}

impl PlanError {
    pub fn message(&self, money: &MoneyFormat) -> String {
        match self {
            PlanError::NotPositive => t("Enter an amount greater than zero", "Escribe un monto mayor que cero").into(),
            PlanError::InFuture => t("The date is in the future", "La fecha está en el futuro").into(),
            PlanError::BeforeStart => t("The date is before the loan started", "La fecha es anterior al inicio del préstamo").into(),
            PlanError::LoanClosed => {
                t("The date is after the loan was marked as defaulted", "La fecha es posterior a cuando se marcó incobrable").into()
            }
            PlanError::Overpayment { payoff } => {
                let p = money.format(*payoff);
                tr!("This is more than the client owes ({p} on this date)", "Es más de lo que debe el cliente ({p} en esta fecha)")
            }
            PlanError::ExceedsInstallment { seq, owed } => {
                let o = money.format(*owed);
                tr!(
                    "Installment {seq} only owes {o} on this date. Spread the payment to cover more.",
                    "La cuota {seq} solo debe {o} en esta fecha. Reparte el pago para cubrir más."
                )
            }
            PlanError::NoSuchInstallment(seq) => tr!("There is no installment {seq}", "No existe la cuota {seq}"),
            PlanError::SplitMismatch { allocated, amount } => {
                let (a, b) = (money.format(*allocated), money.format(*amount));
                tr!("The amounts add up to {a}, not {b}", "Los montos suman {a}, no {b}")
            }
            PlanError::ExceedsPart { part, owed } => {
                let (what, o) = (part.label(), money.format(*owed));
                tr!("Only {o} of {what} is owed on this date", "Solo se deben {o} de {what} en esta fecha")
            }
        }
    }
}

/// The loan after replaying its payments: every installment's state and how
/// each payment was applied.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanState {
    pub principal: Cents,
    pub installments: Vec<InstallmentState>,
    pub payments: Vec<Vec<Applied>>,
    pub paid_total: Cents,
    pub last_payment: Option<NaiveDate>,
}

impl PlanState {
    pub fn principal_paid(&self) -> Cents {
        self.installments.iter().map(|x| x.paid_principal).sum()
    }
    pub fn principal_outstanding(&self) -> Cents {
        self.principal - self.principal_paid()
    }
    pub fn interest_paid(&self) -> Cents {
        self.installments.iter().map(|x| x.paid_interest).sum()
    }
    pub fn fees_paid(&self) -> Cents {
        self.installments.iter().map(|x| x.paid_fee).sum()
    }
    pub fn fees_assessed(&self) -> Cents {
        self.installments.iter().map(|x| x.fee).sum()
    }
    /// Scheduled interest: charged so far plus projected.
    pub fn interest_total(&self) -> Cents {
        self.installments.iter().map(|x| x.interest).sum()
    }
    /// Everything owed now to close the loan: capital, interest charged and
    /// late fees.
    pub fn payoff(&self) -> Cents {
        self.installments.iter().map(InstallmentState::owed).sum()
    }
    pub fn paid_off(&self) -> bool {
        self.principal_outstanding() <= 0 && self.payoff() <= 0
    }

    pub fn position(&self) -> Position {
        let charged: Vec<&InstallmentState> = self.installments.iter().filter(|x| x.charged).collect();
        Position {
            principal_outstanding: self.principal_outstanding(),
            interest_outstanding: charged.iter().map(|x| x.interest - x.paid_interest).sum(),
            interest_charged_total: charged.iter().map(|x| x.interest).sum(),
            interest_paid: self.interest_paid(),
            principal_paid: self.principal_paid(),
            fees_paid: self.fees_paid(),
            fees_outstanding: self.installments.iter().map(InstallmentState::unpaid_fee).sum(),
            paid_total: self.paid_total,
            last_event: self.last_payment,
            periods: charged
                .iter()
                .map(|x| Period { seq: x.seq, starts: x.starts, due: x.due, balance: x.balance_at_start, interest: x.interest })
                .collect(),
        }
    }

    /// Delinquency as of `today` (the state must be run to `today`).
    pub fn delinquency(&self, status: ContractStatus, today: NaiveDate) -> Delinquency {
        if status != ContractStatus::Active || self.paid_off() {
            return Delinquency::default();
        }
        let mut q = Delinquency::default();
        for x in &self.installments {
            if x.due < today && x.owed() > 0 {
                q.overdue_amount += x.owed();
                if q.overdue_since.is_none() {
                    q.overdue_since = Some(x.due);
                    q.days_overdue = (today - x.due).num_days();
                }
            }
        }
        if let Some(next) = self.installments.iter().find(|x| x.due >= today && x.remaining() > 0) {
            q.next_due = Some(next.due);
            q.next_due_amount = q.overdue_amount + next.remaining();
        } else if q.overdue_amount > 0 {
            // Past the last due date: everything left is overdue now.
            q.next_due = Some(today);
            q.next_due_amount = q.overdue_amount;
        }
        q
    }
}

struct Engine<'a> {
    facts: &'a LoanFacts,
    plan: &'a Plan,
    st: Vec<InstallmentState>,
    /// No interest or fees after a default date.
    limit: Option<NaiveDate>,
}

impl Engine<'_> {
    fn outstanding(&self) -> Cents {
        self.st.iter().map(InstallmentState::unpaid_principal).sum()
    }

    fn interest_for(&self, k: usize, balance: Cents) -> Cents {
        if balance <= 0 {
            return 0;
        }
        match self.plan.method {
            Method::Declining if !self.plan.edited => self.facts.rate.interest(balance),
            _ => self.plan.schedule[k].interest,
        }
    }

    /// Charges every period that has begun at `at` (strictly before it when
    /// `strict`) and assesses late fees on installments due before `at`.
    fn advance(&mut self, at: NaiveDate, strict: bool) {
        for k in 0..self.st.len() {
            if !self.st[k].charged {
                let starts = self.st[k].starts;
                let started = if k == 0 || !strict { starts <= at } else { starts < at };
                if started && self.limit.is_none_or(|d| starts <= d) {
                    let balance = self.outstanding();
                    let interest = self.interest_for(k, balance);
                    let s = &mut self.st[k];
                    s.interest = interest;
                    s.charged = true;
                    s.balance_at_start = balance;
                }
            }
            let s = &self.st[k];
            if s.fee_assessed_on.is_none() && s.charged && s.due < at && self.limit.is_none_or(|d| s.due < d) {
                let base = s.unpaid_principal() + s.unpaid_interest();
                if base > 0 && self.plan.fee_rate.0 > 0 {
                    let fee = div_round_half_up(base as i128 * self.plan.fee_rate.0 as i128, RATE_SCALE as i128) as Cents;
                    let s = &mut self.st[k];
                    s.fee_base = base;
                    s.fee = if s.fee_waived { 0 } else { fee };
                    s.fee_assessed_on = Some(s.due + chrono::Duration::days(1));
                }
            }
        }
    }

    /// Projected interest of periods not yet begun, assuming the remaining
    /// capital is repaid as scheduled.
    fn project(&mut self) {
        for k in 0..self.st.len() {
            if !self.st[k].charged {
                let balance: Cents = self.st[k..].iter().map(InstallmentState::unpaid_principal).sum();
                let interest = self.interest_for(k, balance);
                self.st[k].interest = interest;
            }
        }
    }

    fn index(&self, seq: u32) -> Result<usize, PlanError> {
        self.st.iter().position(|x| x.seq == seq).ok_or(PlanError::NoSuchInstallment(seq))
    }

    /// Takes up to `amount` of `part` from installment `k`; returns what was taken.
    fn take(&mut self, k: usize, part: Part, amount: Cents, out: &mut Vec<Applied>) -> Cents {
        let a = amount.min(self.st[k].unpaid(part)).max(0);
        if a > 0 {
            self.st[k].pay(part, a);
            let seq = self.st[k].seq;
            let entry = match out.iter_mut().find(|x| x.seq == seq) {
                Some(e) => e,
                None => {
                    out.push(Applied { seq, ..Default::default() });
                    out.last_mut().unwrap()
                }
            };
            match part {
                Part::Fee => entry.fee += a,
                Part::Interest => entry.interest += a,
                Part::Principal => entry.principal += a,
            }
        }
        a
    }

    /// Late fee, interest, then capital of one installment.
    fn fill(&mut self, k: usize, mut amount: Cents, out: &mut Vec<Applied>) -> Cents {
        for part in [Part::Fee, Part::Interest, Part::Principal] {
            amount -= self.take(k, part, amount, out);
        }
        amount
    }

    fn apply(&mut self, p: &PlanPayment) -> Result<Vec<Applied>, PlanError> {
        let mut out = Vec::new();
        match &p.instruction {
            Instruction::Auto => {
                let mut left = p.amount;
                for k in 0..self.st.len() {
                    left = self.fill(k, left, &mut out);
                }
                if left > 0 {
                    return Err(PlanError::Overpayment { payoff: p.amount - left });
                }
            }
            Instruction::Installment(seq) => {
                let k = self.index(*seq)?;
                let left = self.fill(k, p.amount, &mut out);
                if left > 0 {
                    return Err(PlanError::ExceedsInstallment { seq: *seq, owed: p.amount - left });
                }
            }
            Instruction::Spread(parts) => {
                let allocated: Cents = parts.iter().map(|(_, a)| *a).sum();
                if allocated != p.amount {
                    return Err(PlanError::SplitMismatch { allocated, amount: p.amount });
                }
                for (seq, amount) in parts {
                    let k = self.index(*seq)?;
                    let left = self.fill(k, *amount, &mut out);
                    if left > 0 {
                        return Err(PlanError::ExceedsInstallment { seq: *seq, owed: amount - left });
                    }
                }
            }
            Instruction::Custom { principal, interest, fee } => {
                let allocated = principal + interest + fee;
                if allocated != p.amount {
                    return Err(PlanError::SplitMismatch { allocated, amount: p.amount });
                }
                for (part, amount) in [(Part::Fee, *fee), (Part::Interest, *interest), (Part::Principal, *principal)] {
                    if amount < 0 {
                        return Err(PlanError::NotPositive);
                    }
                    let mut left = amount;
                    for k in 0..self.st.len() {
                        left -= self.take(k, part, left, &mut out);
                    }
                    if left > 0 {
                        return Err(PlanError::ExceedsPart { part, owed: amount - left });
                    }
                }
            }
            Instruction::Fixed(applied) => {
                let total: Cents = applied.iter().map(Applied::total).sum();
                if total != p.amount {
                    return Err(PlanError::SplitMismatch { allocated: total, amount: p.amount });
                }
                for a in applied {
                    let k = self.index(a.seq)?;
                    for (part, amount) in [(Part::Fee, a.fee), (Part::Interest, a.interest), (Part::Principal, a.principal)] {
                        if self.take(k, part, amount, &mut out) != amount {
                            return Err(PlanError::ExceedsPart { part, owed: self.st[k].unpaid(part) });
                        }
                    }
                }
            }
        }
        out.sort_by_key(|a| a.seq);
        Ok(out)
    }
}

/// Replays `payments` (sorted by date, then id) on an installment loan up to
/// the end of `as_of`. Payments after `as_of` are ignored. On failure returns
/// the index of the first payment that doesn't fit and why.
pub fn run(
    facts: &LoanFacts,
    plan: &Plan,
    payments: &[PlanPayment],
    as_of: NaiveDate,
    today: NaiveDate,
) -> Result<PlanState, (usize, PlanError)> {
    let limit = match (facts.status, facts.closed_on) {
        (ContractStatus::Defaulted | ContractStatus::Restructured, Some(d)) => Some(d),
        _ => None,
    };
    let st = plan
        .schedule
        .iter()
        .enumerate()
        .map(|(i, x)| InstallmentState {
            seq: x.seq,
            starts: if i == 0 { facts.start_date } else { plan.schedule[i - 1].due },
            due: x.due,
            principal: x.principal,
            interest: x.interest,
            fee_waived: plan.waived.contains(&x.seq),
            ..Default::default()
        })
        .collect();
    let mut e = Engine { facts, plan, st, limit };
    let mut out = PlanState { principal: facts.principal, ..Default::default() };
    if as_of < facts.start_date {
        e.project();
        out.installments = e.st;
        return Ok(out);
    }
    for (i, p) in payments.iter().enumerate() {
        if p.paid_on > as_of {
            break;
        }
        let fail = |err| Err((i, err));
        if p.amount <= 0 {
            return fail(PlanError::NotPositive);
        }
        if p.paid_on < facts.start_date {
            return fail(PlanError::BeforeStart);
        }
        if p.paid_on > today {
            return fail(PlanError::InFuture);
        }
        if limit.is_some_and(|d| p.paid_on > d) {
            return fail(PlanError::LoanClosed);
        }
        e.advance(p.paid_on, true);
        let applied = e.apply(p).map_err(|err| (i, err))?;
        out.payments.push(applied);
        out.paid_total += p.amount;
        out.last_payment = Some(p.paid_on);
    }
    e.advance(as_of, false);
    e.project();
    out.installments = e.st;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loans::{Frequency, PeriodRate};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn facts(principal: Cents, pct: i64) -> LoanFacts {
        LoanFacts {
            principal,
            rate: PeriodRate::new(Rate(pct * 10_000), Frequency::Monthly, Frequency::Monthly),
            frequency: Frequency::Monthly,
            start_date: d(2026, 1, 1),
            status: ContractStatus::Active,
            closed_on: None,
        }
    }

    fn plan(f: &LoanFacts, method: Method, n: u32) -> Plan {
        Plan {
            method,
            schedule: proposed_schedule(f, method, n),
            edited: false,
            fee_rate: Rate(50_000),
            waived: Vec::new(),
            payments: Vec::new(),
        }
    }

    fn pay(on: NaiveDate, amount: Cents, instruction: Instruction) -> PlanPayment {
        PlanPayment { paid_on: on, amount, instruction }
    }

    const FAR: NaiveDate = NaiveDate::MAX;

    #[test]
    fn the_declining_example() {
        // 10,000 at 10% a month in 5 installments.
        let f = facts(1_000_000, 10);
        let s = proposed_schedule(&f, Method::Declining, 5);
        let totals: Vec<_> = s.iter().map(|x| (x.principal, x.interest, x.total())).collect();
        assert_eq!(
            totals,
            vec![
                (200_000, 100_000, 300_000),
                (200_000, 80_000, 280_000),
                (200_000, 60_000, 260_000),
                (200_000, 40_000, 240_000),
                (200_000, 20_000, 220_000),
            ]
        );
        assert_eq!(s.iter().map(Installment::total).sum::<Cents>(), 1_300_000);
        assert_eq!(s[0].due, d(2026, 2, 1));
        assert!(check_schedule(f.principal, f.start_date, &s).is_ok());

        // Paid on time, it closes with exactly the scheduled amounts.
        let p = plan(&f, Method::Declining, 5);
        let pays: Vec<_> = s.iter().map(|x| pay(x.due, x.total(), Instruction::Auto)).collect();
        let st = run(&f, &p, &pays, FAR, FAR).unwrap();
        assert!(st.paid_off());
        assert_eq!(st.interest_paid(), 300_000);
        assert_eq!(st.fees_assessed(), 0);
    }

    #[test]
    fn flat_interest_is_on_the_original_amount() {
        let f = facts(1_000_000, 10);
        let s = proposed_schedule(&f, Method::Flat, 5);
        assert!(s.iter().all(|x| x.interest == 100_000 && x.principal == 200_000));
        assert_eq!(s.iter().map(|x| x.interest).sum::<Cents>(), 500_000);
    }

    #[test]
    fn rounding_goes_to_the_last_installment() {
        let f = facts(1_000_000, 10);
        let s = proposed_schedule(&f, Method::Declining, 3);
        assert_eq!(s.iter().map(|x| x.principal).collect::<Vec<_>>(), vec![333_333, 333_333, 333_334]);
        assert_eq!(s[1].interest, 66_667, "10% of 6,666.67, half-up");
    }

    #[test]
    fn partial_payment_leaves_the_rest_outstanding_and_a_fee_once() {
        let f = facts(1_000_000, 10);
        let p = plan(&f, Method::Declining, 5);
        // Pays 1,000 of the first 3,000 on time.
        let pays = [pay(d(2026, 2, 1), 100_000, Instruction::Installment(1))];
        let st = run(&f, &p, &pays, d(2026, 2, 1), FAR).unwrap();
        let first = &st.installments[0];
        assert_eq!((first.paid_interest, first.paid_principal), (100_000, 0));
        assert_eq!(first.status(d(2026, 2, 1)), InstallmentStatus::PartlyPaid);
        assert_eq!(first.fee, 0, "not overdue on its due date");

        // The day after: 5% of the 2,000 still unpaid.
        let st = run(&f, &p, &pays, d(2026, 2, 2), FAR).unwrap();
        let first = &st.installments[0];
        assert_eq!((first.fee_base, first.fee, first.fee_assessed_on), (200_000, 10_000, Some(d(2026, 2, 2))));
        assert_eq!(first.status(d(2026, 2, 2)), InstallmentStatus::Overdue);
        // Running again (a refresh) gives the same single fee.
        assert_eq!(run(&f, &p, &pays, d(2026, 2, 20), FAR).unwrap().installments[0].fee, 10_000);

        // Paying part of it later doesn't change the fee.
        let mut later = pays.to_vec();
        later.push(pay(d(2026, 2, 10), 50_000, Instruction::Installment(1)));
        let st = run(&f, &p, &later, d(2026, 2, 10), FAR).unwrap();
        let first = &st.installments[0];
        assert_eq!(first.fee, 10_000);
        assert_eq!((first.paid_fee, first.paid_principal), (10_000, 40_000), "fee first, then capital");
    }

    #[test]
    fn late_capital_keeps_the_next_interest_higher() {
        let f = facts(1_000_000, 10);
        let p = plan(&f, Method::Declining, 5);
        // Only the interest is paid on Feb 1; the capital is still owed when
        // period 2 begins.
        let pays = [pay(d(2026, 2, 1), 100_000, Instruction::Auto)];
        let st = run(&f, &p, &pays, d(2026, 2, 2), FAR).unwrap();
        assert_eq!(st.installments[1].interest, 100_000);
        assert_eq!(st.installments[0].fee, 10_000, "5% of the 2,000 of capital unpaid");
    }

    #[test]
    fn auto_payment_fills_oldest_first_and_refuses_too_much() {
        let f = facts(1_000_000, 10);
        let p = plan(&f, Method::Declining, 5);
        let st = run(&f, &p, &[pay(d(2026, 2, 1), 400_000, Instruction::Auto)], FAR, FAR).unwrap();
        // 3,000 for the first installment, then 1,000 of the second one's capital.
        assert_eq!(
            st.payments[0],
            vec![
                Applied { seq: 1, principal: 200_000, interest: 100_000, fee: 0 },
                Applied { seq: 2, principal: 100_000, interest: 0, fee: 0 }
            ]
        );
        // Second period's interest is on the 7,000 still owed.
        assert_eq!(st.installments[1].interest, 70_000);
        let err = run(&f, &p, &[pay(d(2026, 2, 1), 2_000_000, Instruction::Auto)], FAR, FAR).unwrap_err();
        assert_eq!(err, (0, PlanError::Overpayment { payoff: 1_100_000 }));
    }

    #[test]
    fn spread_and_custom_splits() {
        let f = facts(1_000_000, 10);
        let p = plan(&f, Method::Flat, 5);
        // Day after Mar 1: installments 1 and 2 overdue (3,000 each), fees 150 each.
        let spread = Instruction::Spread(vec![(1, 315_000), (2, 100_000)]);
        let st = run(&f, &p, &[pay(d(2026, 3, 5), 415_000, spread)], FAR, FAR).unwrap();
        assert_eq!(st.installments[0].remaining(), 0);
        assert_eq!((st.installments[1].paid_fee, st.installments[1].paid_interest), (15_000, 85_000));
        let bad = Instruction::Spread(vec![(1, 100_000)]);
        assert_eq!(
            run(&f, &p, &[pay(d(2026, 3, 5), 200_000, bad)], FAR, FAR).unwrap_err().1,
            PlanError::SplitMismatch { allocated: 100_000, amount: 200_000 }
        );

        // Custom: all to capital.
        let custom = Instruction::Custom { principal: 500_000, interest: 0, fee: 0 };
        let st = run(&f, &p, &[pay(d(2026, 1, 10), 500_000, custom)], FAR, FAR).unwrap();
        assert_eq!(st.principal_outstanding(), 500_000);
        assert_eq!(st.installments[0].paid_principal, 200_000);
        assert_eq!(st.installments[2].paid_principal, 100_000);
        let too_much = Instruction::Custom { principal: 0, interest: 200_000, fee: 0 };
        assert_eq!(
            run(&f, &p, &[pay(d(2026, 1, 10), 200_000, too_much)], FAR, FAR).unwrap_err().1,
            PlanError::ExceedsPart { part: Part::Interest, owed: 100_000 }
        );
    }

    #[test]
    fn stored_splits_replay_exactly() {
        let f = facts(1_000_000, 10);
        let mut p = plan(&f, Method::Declining, 5);
        let pays = [pay(d(2026, 2, 1), 300_000, Instruction::Auto), pay(d(2026, 3, 9), 100_000, Instruction::Installment(2))];
        let live = run(&f, &p, &pays, FAR, FAR).unwrap();
        p.payments =
            pays.iter().zip(&live.payments).map(|(x, a)| PlanPayment { instruction: Instruction::Fixed(a.clone()), ..x.clone() }).collect();
        let replay = run(&f, &p, &p.payments, FAR, FAR).unwrap();
        assert_eq!(replay, live);
    }

    #[test]
    fn waived_fee_and_delinquency() {
        let f = facts(1_000_000, 10);
        let mut p = plan(&f, Method::Declining, 5);
        let today = d(2026, 2, 10);
        let st = run(&f, &p, &[], today, today).unwrap();
        let q = st.delinquency(ContractStatus::Active, today);
        // 3,000 overdue plus its 150 fee; next is installment 2 (interest 1,000 on 10,000 still owed).
        assert_eq!((q.overdue_amount, q.overdue_since, q.days_overdue), (315_000, Some(d(2026, 2, 1)), 9));
        assert_eq!(q.next_due, Some(d(2026, 3, 1)));
        assert_eq!(q.next_due_amount, 315_000 + 300_000);
        p.waived = vec![1];
        let st = run(&f, &p, &[], today, today).unwrap();
        assert_eq!((st.installments[0].fee, st.installments[0].fee_base), (0, 300_000));
        assert_eq!(st.position().fees_outstanding, 0);
    }

    #[test]
    fn edited_schedule_keeps_its_interest() {
        let f = facts(1_000_000, 10);
        let mut p = plan(&f, Method::Declining, 2);
        p.schedule[0].interest = 50_000;
        p.schedule[1].interest = 50_000;
        p.edited = true;
        let st = run(&f, &p, &[pay(d(2026, 2, 1), 100_000, Instruction::Auto)], FAR, FAR).unwrap();
        assert_eq!(st.installments[1].interest, 50_000);
        let mut bad = p.schedule.clone();
        bad[1].principal = 400_000;
        assert_eq!(
            check_schedule(f.principal, f.start_date, &bad),
            Err(ScheduleError::Unallocated { allocated: 900_000, principal: 1_000_000 })
        );
        bad[1].principal = 500_000;
        bad[1].due = bad[0].due;
        assert_eq!(check_schedule(f.principal, f.start_date, &bad), Err(ScheduleError::DateOrder(2)));
    }

    #[test]
    fn weekly_installments_on_a_monthly_rate() {
        let mut f = facts(1_000_000, 10);
        f.frequency = Frequency::Weekly;
        f.rate = PeriodRate::new(Rate(100_000), Frequency::Monthly, Frequency::Weekly);
        let s = proposed_schedule(&f, Method::Declining, 4);
        assert_eq!(s[0].interest, 23_333);
        assert_eq!(s[1].due, d(2026, 1, 15));
        let mut f = facts(1_000_000, 1);
        f.frequency = Frequency::Daily;
        f.rate = PeriodRate::new(Rate(10_000), Frequency::Daily, Frequency::Daily);
        let s = proposed_schedule(&f, Method::Flat, 10);
        assert_eq!((s[0].interest, s[9].due), (10_000, d(2026, 1, 11)));
    }
}
