//! The loan engine: a loan's position on any date, payment allocation and
//! delinquency, as pure functions over plain data. Rules are documented in
//! `docs/ACCOUNTING.md`.
//!
//! Model: each period's interest is the loan's rate times the capital owed
//! when the period begins. It is charged at the start of the period and is due
//! by the period's end. Payments go to fees (as entered), then to interest
//! owed (oldest first), then to capital. A capital payment lowers the interest
//! of the *next* period.
//!
//! Example: 1,000 at 10% a month. The first period's interest is 100. Paying
//! 600 covers the 100 of interest and repays 500 of capital, so the next
//! period's interest is 50.

use chrono::NaiveDate;

use crate::domain::loans::{Frequency, PeriodRate};
use crate::domain::money::Cents;
use crate::i18n::t;
use crate::utils::dates::days_between;

/// Contractual status as stored. Due soon and overdue are calculated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContractStatus {
    Active,
    PaidOff,
    Defaulted,
    Restructured,
}

impl ContractStatus {
    pub fn key(self) -> &'static str {
        match self {
            ContractStatus::Active => "active",
            ContractStatus::PaidOff => "paid_off",
            ContractStatus::Defaulted => "defaulted",
            ContractStatus::Restructured => "restructured",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "active" => Some(ContractStatus::Active),
            "paid_off" => Some(ContractStatus::PaidOff),
            "defaulted" => Some(ContractStatus::Defaulted),
            "restructured" => Some(ContractStatus::Restructured),
            _ => None,
        }
    }
}

/// What the user sees: contractual status combined with delinquency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DisplayStatus {
    Overdue,
    DueSoon,
    Active,
    Defaulted,
    Restructured,
    PaidOff,
}

impl DisplayStatus {
    pub fn label(self) -> &'static str {
        match self {
            DisplayStatus::Active => t("Current", "Al día"),
            DisplayStatus::DueSoon => t("Due soon", "Vence pronto"),
            DisplayStatus::Overdue => t("Overdue", "Atrasado"),
            DisplayStatus::PaidOff => t("Paid off", "Saldado"),
            DisplayStatus::Defaulted => t("Defaulted", "Incobrable"),
            DisplayStatus::Restructured => t("Restructured", "Reestructurado"),
        }
    }
}

/// Days before a due date at which a payment is flagged "Due soon".
pub const DUE_SOON_DAYS: i64 = 5;

#[derive(Clone, Debug)]
pub struct LoanFacts {
    pub principal: Cents,
    /// The quoted rate and its conversion to the repayment period.
    pub rate: PeriodRate,
    /// How often payments fall due (each period's length).
    pub frequency: Frequency,
    pub start_date: NaiveDate,
    pub status: ContractStatus,
    /// Date a loan was defaulted or restructured: no new interest after it.
    pub closed_on: Option<NaiveDate>,
}

/// A recorded, non-reversed payment with its stored allocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentFact {
    pub paid_on: NaiveDate,
    pub principal: Cents,
    pub interest: Cents,
    pub fee: Cents,
}

impl PaymentFact {
    pub fn total(&self) -> Cents {
        self.principal + self.interest + self.fee
    }
}

/// One charged period.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Period {
    pub seq: u32,
    pub starts: NaiveDate,
    pub due: NaiveDate,
    /// Capital owed when the period began.
    pub balance: Cents,
    pub interest: Cents,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub principal_outstanding: Cents,
    /// Interest charged and not yet paid.
    pub interest_outstanding: Cents,
    pub interest_charged_total: Cents,
    pub interest_paid: Cents,
    pub principal_paid: Cents,
    pub fees_paid: Cents,
    /// Late fees assessed and not yet paid (installment loans).
    pub fees_outstanding: Cents,
    pub paid_total: Cents,
    pub last_event: Option<NaiveDate>,
    pub periods: Vec<Period>,
}

impl Position {
    pub fn payoff(&self) -> Cents {
        self.principal_outstanding + self.interest_outstanding + self.fees_outstanding
    }
}

/// Which periods count as charged at the cut-off.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cutoff {
    /// End of the day: a period starting on this date is charged, after that
    /// day's payments.
    EndOfDay,
    /// The moment a payment is received: a period starting on this date is
    /// charged only after the payment, so it is not included.
    BeforePayment,
}

struct Replay<'a> {
    loan: &'a LoanFacts,
    pos: Position,
    next_seq: u32,
    charge_until: NaiveDate,
}

impl Replay<'_> {
    /// Charge every period that has started at `at` (strictly before it when
    /// `strict`), on the capital owed at that moment.
    fn charge_through(&mut self, at: NaiveDate, strict: bool) {
        loop {
            let k = self.next_seq;
            let starts = self.loan.frequency.period_start(self.loan.start_date, k);
            let started = if k == 1 {
                starts <= at
            } else if strict {
                starts < at
            } else {
                starts <= at
            };
            if !started || starts > self.charge_until || self.pos.principal_outstanding <= 0 {
                break;
            }
            let interest = self.loan.rate.interest(self.pos.principal_outstanding);
            self.pos.periods.push(Period {
                seq: k,
                starts,
                due: self.loan.frequency.due_date(self.loan.start_date, k),
                balance: self.pos.principal_outstanding,
                interest,
            });
            self.pos.interest_outstanding += interest;
            self.pos.interest_charged_total += interest;
            self.next_seq += 1;
        }
    }
}

fn replay(loan: &LoanFacts, payments: &[PaymentFact], at: NaiveDate, cutoff: Cutoff) -> Position {
    if at < loan.start_date {
        return Position::default();
    }
    let charge_until = match (loan.status, loan.closed_on) {
        (ContractStatus::Defaulted | ContractStatus::Restructured, Some(d)) => d,
        _ => NaiveDate::MAX,
    };
    let mut r = Replay { loan, pos: Position { principal_outstanding: loan.principal, ..Default::default() }, next_seq: 1, charge_until };
    for p in payments.iter().filter(|p| p.paid_on <= at) {
        // Payments dated on a due date belong to the period ending then.
        r.charge_through(p.paid_on, true);
        r.pos.interest_outstanding -= p.interest;
        r.pos.principal_outstanding -= p.principal;
        r.pos.interest_paid += p.interest;
        r.pos.principal_paid += p.principal;
        r.pos.fees_paid += p.fee;
        r.pos.paid_total += p.total();
        r.pos.last_event = Some(p.paid_on);
    }
    r.charge_through(at, cutoff == Cutoff::BeforePayment);
    r.pos
}

/// The loan's position at the end of `as_of`. Payments must be sorted by
/// date, then by id.
pub fn position(loan: &LoanFacts, payments: &[PaymentFact], as_of: NaiveDate) -> Position {
    replay(loan, payments, as_of, Cutoff::EndOfDay)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub interest: Cents,
    pub principal: Cents,
    pub fee: Cents,
    /// Capital owed before this payment.
    pub principal_before: Cents,
    pub principal_after: Cents,
    pub interest_after: Cents,
}

impl Allocation {
    pub fn pays_off(&self) -> bool {
        self.principal_after == 0 && self.interest_after == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PaymentError {
    #[error("not positive")]
    NotPositive,
    #[error("overpayment")]
    Overpayment { payoff: Cents },
    #[error("before start")]
    BeforeStart,
    #[error("before last payment")]
    BeforeLastPayment { last: NaiveDate },
    #[error("in future")]
    InFuture,
    #[error("loan closed")]
    LoanClosed,
    #[error("fee too large")]
    FeeTooLarge,
}

impl PaymentError {
    pub fn message(&self) -> &'static str {
        match self {
            PaymentError::NotPositive => t("Enter an amount greater than zero", "Escribe un monto mayor que cero"),
            PaymentError::Overpayment { .. } => t("This is more than the client owes", "Es más de lo que debe el cliente"),
            PaymentError::BeforeStart => t("The date is before the loan started", "La fecha es anterior al inicio del préstamo"),
            PaymentError::BeforeLastPayment { .. } => {
                t("A later payment is already recorded", "Ya hay un pago registrado con fecha posterior")
            }
            PaymentError::InFuture => t("The date is in the future", "La fecha está en el futuro"),
            PaymentError::LoanClosed => t("This loan is closed", "Este préstamo está cerrado"),
            PaymentError::FeeTooLarge => t("The fee is larger than the payment", "El cargo es mayor que el pago"),
        }
    }
}

/// Allocates a payment received on `paid_on`: fee as entered, then interest
/// owed, then capital. Amounts above what is owed are refused.
pub fn allocate(
    loan: &LoanFacts,
    payments: &[PaymentFact],
    amount: Cents,
    fee: Cents,
    paid_on: NaiveDate,
    today: NaiveDate,
) -> Result<Allocation, PaymentError> {
    if amount <= 0 {
        return Err(PaymentError::NotPositive);
    }
    if fee < 0 || fee > amount {
        return Err(PaymentError::FeeTooLarge);
    }
    if loan.status != ContractStatus::Active {
        return Err(PaymentError::LoanClosed);
    }
    if paid_on < loan.start_date {
        return Err(PaymentError::BeforeStart);
    }
    if paid_on > today {
        return Err(PaymentError::InFuture);
    }
    if let Some(last) = payments.iter().map(|p| p.paid_on).max()
        && paid_on < last
    {
        return Err(PaymentError::BeforeLastPayment { last });
    }
    let pos = replay(loan, payments, paid_on, Cutoff::BeforePayment);
    let available = amount - fee;
    if available > pos.payoff() {
        return Err(PaymentError::Overpayment { payoff: pos.payoff() + fee });
    }
    let interest = available.min(pos.interest_outstanding.max(0));
    let principal = (available - interest).min(pos.principal_outstanding);
    Ok(Allocation {
        interest,
        principal,
        fee,
        principal_before: pos.principal_outstanding,
        principal_after: pos.principal_outstanding - principal,
        interest_after: pos.interest_outstanding - interest,
    })
}

/// A recorded payment before its split is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaymentInput {
    pub paid_on: NaiveDate,
    pub amount: Cents,
    pub fee: Cents,
}

/// Splits every payment on a loan again, oldest first, from the loan's
/// current terms. Used after a loan or any of its payments is edited, added
/// or deleted, so every split follows the same rules as a new payment.
/// Payments must be sorted by date, then by id. On failure returns the index
/// of the first payment that no longer fits and why.
pub fn reallocate(loan: &LoanFacts, payments: &[PaymentInput], today: NaiveDate) -> Result<Vec<Allocation>, (usize, PaymentError)> {
    // Payments are allowed up to a default date; the split itself is the one
    // an active loan would have.
    let open = LoanFacts { status: ContractStatus::Active, closed_on: None, ..loan.clone() };
    let limit = match (loan.status, loan.closed_on) {
        (ContractStatus::Defaulted | ContractStatus::Restructured, Some(d)) => Some(d),
        _ => None,
    };
    let mut facts: Vec<PaymentFact> = Vec::with_capacity(payments.len());
    let mut out = Vec::with_capacity(payments.len());
    for (i, p) in payments.iter().enumerate() {
        if limit.is_some_and(|d| p.paid_on > d) {
            return Err((i, PaymentError::LoanClosed));
        }
        // After a payoff nothing is owed, so a later payment is refused as
        // an overpayment.
        let a = allocate(&open, &facts, p.amount, p.fee, p.paid_on, today).map_err(|e| (i, e))?;
        facts.push(PaymentFact { paid_on: p.paid_on, principal: a.principal, interest: a.interest, fee: a.fee });
        out.push(a);
    }
    Ok(out)
}

/// What a client owes on a given day (at the moment of a payment), for
/// quick-fill amounts: interest owed now and the full payoff.
pub fn owed_at_payment(loan: &LoanFacts, payments: &[PaymentFact], on: NaiveDate) -> Position {
    replay(loan, payments, on, Cutoff::BeforePayment)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Delinquency {
    /// Interest past its due date and not yet paid.
    pub overdue_amount: Cents,
    pub days_overdue: i64,
    /// Due date of the oldest unpaid period that is past due.
    pub overdue_since: Option<NaiveDate>,
    /// The next due date, today or later.
    pub next_due: Option<NaiveDate>,
    /// Everything that must be paid by that date to be current, including
    /// any overdue interest.
    pub next_due_amount: Cents,
}

/// Delinquency as of `today`. Interest paid covers periods oldest first; a
/// period is overdue the day after its due date if its interest is not
/// covered.
pub fn delinquency(loan: &LoanFacts, payments: &[PaymentFact], today: NaiveDate) -> Delinquency {
    if loan.status != ContractStatus::Active {
        return Delinquency::default();
    }
    let pos = position(loan, payments, today);
    if pos.payoff() <= 0 {
        return Delinquency::default();
    }
    let mut result = Delinquency::default();
    let mut cumulative = 0;
    let mut past_due = 0;
    for p in &pos.periods {
        cumulative += p.interest;
        if p.due < today {
            past_due = cumulative;
            if result.overdue_since.is_none() && pos.interest_paid < cumulative {
                result.overdue_since = Some(p.due);
                result.days_overdue = days_between(p.due, today);
            }
        } else if result.next_due.is_none() && pos.interest_paid < cumulative {
            result.next_due = Some(p.due);
            result.next_due_amount = cumulative - pos.interest_paid;
        }
    }
    result.overdue_amount = (past_due - pos.interest_paid).max(0);
    if result.next_due.is_none() {
        // Everything charged by now is paid (or only overdue interest is left):
        // the next payment is the next period's interest plus anything overdue.
        let k = pos.periods.last().map(|p| p.seq + 1).unwrap_or(1);
        result.next_due = Some(loan.frequency.due_date(loan.start_date, k));
        result.next_due_amount = (cumulative - pos.interest_paid).max(0) + loan.rate.interest(pos.principal_outstanding);
    }
    result
}

pub fn display_status(loan: &LoanFacts, delinquency: &Delinquency, today: NaiveDate) -> DisplayStatus {
    match loan.status {
        ContractStatus::PaidOff => DisplayStatus::PaidOff,
        ContractStatus::Defaulted => DisplayStatus::Defaulted,
        ContractStatus::Restructured => DisplayStatus::Restructured,
        ContractStatus::Active => {
            if delinquency.overdue_amount > 0 {
                DisplayStatus::Overdue
            } else if delinquency.next_due_amount > 0 && delinquency.next_due.is_some_and(|d| days_between(today, d) <= DUE_SOON_DAYS) {
                DisplayStatus::DueSoon
            } else {
                DisplayStatus::Active
            }
        }
    }
}

/// Coverage of a charged period by interest paid so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodState {
    Paid,
    PartlyPaid,
    Overdue,
    Open,
}

/// The state of each charged period, oldest first.
pub fn period_states(pos: &Position, today: NaiveDate) -> Vec<(Period, PeriodState, Cents)> {
    let mut cumulative = 0;
    pos.periods
        .iter()
        .map(|p| {
            let before = cumulative;
            cumulative += p.interest;
            let paid = (pos.interest_paid - before).clamp(0, p.interest);
            let state = if paid >= p.interest {
                PeriodState::Paid
            } else if p.due < today {
                PeriodState::Overdue
            } else if paid > 0 {
                PeriodState::PartlyPaid
            } else {
                PeriodState::Open
            };
            (p.clone(), state, paid)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn loan(principal: Cents, pct: i64, frequency: Frequency, start: NaiveDate) -> LoanFacts {
        LoanFacts {
            principal,
            rate: PeriodRate::new(crate::domain::money::Rate(pct * 10_000), frequency, frequency),
            frequency,
            start_date: start,
            status: ContractStatus::Active,
            closed_on: None,
        }
    }

    #[test]
    fn weekly_payments_on_a_monthly_rate() {
        // 10,000 at 10% a month, paid weekly: 7/30 of 1,000 each week.
        let mut l = loan(1_000_000, 10, Frequency::Weekly, d(2026, 1, 1));
        l.rate = PeriodRate::new(crate::domain::money::Rate(100_000), Frequency::Monthly, Frequency::Weekly);
        let pos = position(&l, &[], d(2026, 1, 8));
        assert_eq!(pos.periods.len(), 2);
        assert_eq!(pos.periods[0].interest, 23_333);
        assert_eq!(pos.periods[1].due, d(2026, 1, 15));
    }

    fn pay(l: &LoanFacts, history: &mut Vec<PaymentFact>, amount: Cents, on: NaiveDate) -> Allocation {
        let a = allocate(l, history, amount, 0, on, d(2030, 1, 1)).expect("allocates");
        history.push(PaymentFact { paid_on: on, principal: a.principal, interest: a.interest, fee: 0 });
        a
    }

    #[test]
    fn the_owners_example() {
        // 1,000 at 10% a month: 100 of interest per month.
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let mut h = Vec::new();
        assert_eq!(position(&l, &h, d(2026, 1, 15)).interest_outstanding, 10_000);
        // The client pays 600 on the due date: 100 interest, 500 capital.
        let a = pay(&l, &mut h, 60_000, d(2026, 2, 1));
        assert_eq!((a.interest, a.principal), (10_000, 50_000));
        assert_eq!(a.principal_before, 100_000);
        assert_eq!(a.principal_after, 50_000);
        // Next month's interest is only 50.
        let q = delinquency(&l, &h, d(2026, 2, 2));
        assert_eq!(q.next_due, Some(d(2026, 3, 1)));
        assert_eq!(q.next_due_amount, 5_000);
        // Paying just the 50 keeps the capital at 500.
        let a = pay(&l, &mut h, 5_000, d(2026, 3, 1));
        assert_eq!((a.interest, a.principal), (5_000, 0));
        assert_eq!(position(&l, &h, d(2026, 3, 2)).interest_outstanding, 5_000);
    }

    #[test]
    fn early_payment_in_the_period_still_covers_its_interest_first() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let mut h = Vec::new();
        let a = pay(&l, &mut h, 60_000, d(2026, 1, 20));
        assert_eq!((a.interest, a.principal), (10_000, 50_000));
        // Nothing more is owed until the next period begins on Feb 1.
        let q = delinquency(&l, &h, d(2026, 1, 25));
        assert_eq!(q.overdue_amount, 0);
        assert_eq!(q.next_due, Some(d(2026, 3, 1)));
        assert_eq!(q.next_due_amount, 5_000);
    }

    #[test]
    fn biweekly_periods() {
        let l = loan(200_000, 5, Frequency::Biweekly, d(2026, 1, 5));
        let mut h = Vec::new();
        // 5% of 2,000 = 100 every 15 days.
        let a = pay(&l, &mut h, 10_000, d(2026, 1, 20));
        assert_eq!(a.interest, 10_000);
        let pos = position(&l, &h, d(2026, 1, 20));
        assert_eq!(pos.periods.len(), 2);
        assert_eq!(pos.periods[1].due, d(2026, 2, 5));
    }

    #[test]
    fn partial_interest_carries_over_without_compounding() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let mut h = Vec::new();
        pay(&l, &mut h, 4_000, d(2026, 2, 1));
        let pos = position(&l, &h, d(2026, 2, 1));
        // 60 left from January plus February's 100, still on 1,000 of capital.
        assert_eq!(pos.interest_outstanding, 6_000 + 10_000);
        assert_eq!(pos.principal_outstanding, 100_000);
        let q = delinquency(&l, &h, d(2026, 2, 2));
        assert_eq!(q.overdue_amount, 6_000);
        assert_eq!(q.days_overdue, 1);
        assert_eq!(q.overdue_since, Some(d(2026, 2, 1)));
        // To be current by March 1: the 60 overdue plus February's 100.
        assert_eq!((q.next_due, q.next_due_amount), (Some(d(2026, 3, 1)), 16_000));
        assert_eq!(display_status(&l, &q, d(2026, 2, 2)), DisplayStatus::Overdue);
        // The next payment pays the oldest interest first.
        let a = pay(&l, &mut h, 20_000, d(2026, 2, 10));
        assert_eq!((a.interest, a.principal), (16_000, 4_000));
    }

    #[test]
    fn due_date_is_not_overdue_the_day_after_is() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let h = Vec::new();
        let q = delinquency(&l, &h, d(2026, 2, 1));
        assert_eq!(q.overdue_amount, 0);
        assert_eq!(display_status(&l, &q, d(2026, 2, 1)), DisplayStatus::DueSoon);
        let q = delinquency(&l, &h, d(2026, 2, 2));
        assert_eq!(q.overdue_amount, 10_000);
    }

    #[test]
    fn payoff_closes_and_overpayment_is_refused() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let mut h = Vec::new();
        assert_eq!(allocate(&l, &h, 200_000, 0, d(2026, 1, 10), d(2030, 1, 1)), Err(PaymentError::Overpayment { payoff: 110_000 }));
        let a = pay(&l, &mut h, 110_000, d(2026, 1, 10));
        assert!(a.pays_off());
        // No interest is charged once the capital is repaid.
        let pos = position(&l, &h, d(2026, 6, 1));
        assert_eq!(pos.payoff(), 0);
        assert_eq!(pos.periods.len(), 1);
    }

    #[test]
    fn validation() {
        let mut l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let mut h = Vec::new();
        pay(&l, &mut h, 1_000, d(2026, 3, 1));
        let today = d(2026, 4, 1);
        assert_eq!(allocate(&l, &h, 100, 0, d(2026, 2, 1), today), Err(PaymentError::BeforeLastPayment { last: d(2026, 3, 1) }));
        assert_eq!(allocate(&l, &h, 100, 0, d(2026, 4, 2), today), Err(PaymentError::InFuture));
        assert_eq!(allocate(&l, &h, 100, 0, d(2025, 12, 31), today), Err(PaymentError::BeforeStart));
        assert_eq!(allocate(&l, &h, 0, 0, today, today), Err(PaymentError::NotPositive));
        assert_eq!(allocate(&l, &h, 100, 200, today, today), Err(PaymentError::FeeTooLarge));
        l.status = ContractStatus::PaidOff;
        assert_eq!(allocate(&l, &h, 100, 0, today, today), Err(PaymentError::LoanClosed));
    }

    #[test]
    fn fee_comes_first() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let a = allocate(&l, &[], 15_000, 2_500, d(2026, 1, 10), d(2030, 1, 1)).unwrap();
        assert_eq!((a.fee, a.interest, a.principal), (2_500, 10_000, 2_500));
    }

    #[test]
    fn default_stops_new_interest() {
        let mut l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        l.status = ContractStatus::Defaulted;
        l.closed_on = Some(d(2026, 2, 15));
        let pos = position(&l, &[], d(2026, 12, 1));
        // January and February charged; nothing after the default date.
        assert_eq!(pos.interest_outstanding, 20_000);
        assert_eq!(delinquency(&l, &[], d(2026, 12, 1)), Delinquency::default());
    }

    #[test]
    fn reallocation_follows_edited_terms() {
        let input = |amount: Cents, on: NaiveDate| PaymentInput { paid_on: on, amount, fee: 0 };
        let payments = [input(60_000, d(2026, 2, 1)), input(5_000, d(2026, 3, 1))];
        let today = d(2026, 6, 1);
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let a = reallocate(&l, &payments, today).unwrap();
        assert_eq!((a[0].interest, a[0].principal, a[1].interest, a[1].principal), (10_000, 50_000, 5_000, 0));

        // The loan was really 2,000: both payments are split again.
        let l = loan(200_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let a = reallocate(&l, &payments, today).unwrap();
        assert_eq!((a[0].interest, a[0].principal), (20_000, 40_000));
        assert_eq!((a[1].interest, a[1].principal), (5_000, 0), "only part of February's 160 of interest");
        assert_eq!(a[1].interest_after, 11_000);

        // A smaller loan makes the first payment more than was owed.
        let l = loan(40_000, 10, Frequency::Monthly, d(2026, 1, 1));
        assert_eq!(reallocate(&l, &payments, today), Err((0, PaymentError::Overpayment { payoff: 44_000 })));

        // A later loan date puts a payment before the loan.
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 2, 15));
        assert_eq!(reallocate(&l, &payments, today), Err((0, PaymentError::BeforeStart)));
    }

    #[test]
    fn nothing_is_owed_after_a_payoff() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let payments = [
            PaymentInput { paid_on: d(2026, 1, 10), amount: 110_000, fee: 0 },
            PaymentInput { paid_on: d(2026, 2, 1), amount: 1_000, fee: 0 },
        ];
        assert_eq!(reallocate(&l, &payments, d(2026, 6, 1)), Err((1, PaymentError::Overpayment { payoff: 0 })));
    }

    #[test]
    fn period_states_cover_oldest_first() {
        let l = loan(100_000, 10, Frequency::Monthly, d(2026, 1, 1));
        let mut h = Vec::new();
        pay(&l, &mut h, 15_000, d(2026, 2, 1));
        let pos = position(&l, &h, d(2026, 3, 5));
        let s = period_states(&pos, d(2026, 3, 5));
        assert_eq!(s.len(), 3);
        // 150 paid of 100 (Jan) + 95 (Feb, on 950 of capital after 50 repaid).
        assert_eq!(pos.periods[1].balance, 95_000);
        assert_eq!(s[0].1, PeriodState::Paid);
        assert_eq!(s[1].1, PeriodState::Overdue);
        assert_eq!(s[2].1, PeriodState::Open);
    }
}
