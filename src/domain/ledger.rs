//! The accrual engine: a loan's actual position on any date, payment
//! allocation, and delinquency. Pure functions over plain data so every rule
//! is unit-tested without a database or UI. Rules are documented in
//! `docs/ACCOUNTING.md`.

use chrono::NaiveDate;

use crate::domain::loans::{Installment, InterestMethod};
use crate::domain::money::{Cents, Rate, interest_for_days};
use crate::utils::dates::days_between;

/// Contractual status as stored. Calculated indicators (due soon, overdue)
/// are derived separately and never written back.
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

/// What the user sees: contractual status combined with calculated delinquency.
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
            DisplayStatus::Active => "Active",
            DisplayStatus::DueSoon => "Due soon",
            DisplayStatus::Overdue => "Overdue",
            DisplayStatus::PaidOff => "Paid off",
            DisplayStatus::Defaulted => "Defaulted",
            DisplayStatus::Restructured => "Restructured",
        }
    }
}

/// Days before a due date at which an installment is flagged "Due soon".
pub const DUE_SOON_DAYS: i64 = 7;

/// The facts about a loan the engine needs.
#[derive(Clone, Debug)]
pub struct LoanFacts {
    pub principal: Cents,
    pub rate: Rate,
    pub method: InterestMethod,
    pub start_date: NaiveDate,
    pub maturity_date: NaiveDate,
    pub status: ContractStatus,
    /// Date the contractual status last changed to Defaulted/Restructured;
    /// interest stops accruing from that date.
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

/// A loan's actual position at the end of a given day.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub principal_outstanding: Cents,
    /// Interest accrued and not yet paid (receivable).
    pub interest_outstanding: Cents,
    /// All interest accrued since the start, paid or not.
    pub interest_accrued_total: Cents,
    pub interest_paid: Cents,
    pub principal_paid: Cents,
    pub fees_paid: Cents,
    pub paid_total: Cents,
    pub last_event: Option<NaiveDate>,
}

impl Position {
    pub fn payoff(&self) -> Cents {
        self.principal_outstanding + self.interest_outstanding
    }
}

/// Replays a loan's payments to `as_of` (inclusive). Payments must be sorted
/// by date then by id. Interest accrues daily, Actual/365, on the outstanding
/// principal only (no compounding); each accrual segment between events is
/// rounded half-up to the cent. Accrual stops on the date a loan is defaulted
/// or restructured.
pub fn position(loan: &LoanFacts, payments: &[PaymentFact], as_of: NaiveDate) -> Position {
    let mut pos = Position { principal_outstanding: loan.principal, ..Default::default() };
    if as_of < loan.start_date {
        return Position::default();
    }
    let accrual_end = match (loan.status, loan.closed_on) {
        (ContractStatus::Defaulted | ContractStatus::Restructured, Some(d)) => d.min(as_of),
        _ => as_of,
    };
    let mut last = loan.start_date;
    for p in payments.iter().filter(|p| p.paid_on <= as_of) {
        let until = p.paid_on.min(accrual_end);
        if until > last {
            let accrued = interest_for_days(pos.principal_outstanding, loan.rate, days_between(last, until));
            pos.interest_outstanding += accrued;
            pos.interest_accrued_total += accrued;
            last = until;
        }
        pos.interest_outstanding -= p.interest;
        pos.principal_outstanding -= p.principal;
        pos.interest_paid += p.interest;
        pos.principal_paid += p.principal;
        pos.fees_paid += p.fee;
        pos.paid_total += p.total();
        pos.last_event = Some(p.paid_on);
    }
    if accrual_end > last {
        let accrued = interest_for_days(pos.principal_outstanding, loan.rate, days_between(last, accrual_end));
        pos.interest_outstanding += accrued;
        pos.interest_accrued_total += accrued;
    }
    pos
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub interest: Cents,
    pub principal: Cents,
    pub fee: Cents,
    /// Remaining after this payment.
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
    #[error("Enter an amount greater than zero")]
    NotPositive,
    #[error("This is more than the payoff amount")]
    Overpayment { payoff: Cents },
    #[error("The payment date is before the loan started")]
    BeforeStart,
    #[error("The payment date is before the latest recorded payment")]
    BeforeLastPayment { last: NaiveDate },
    #[error("The payment date is in the future")]
    InFuture,
    #[error("This loan is closed")]
    LoanClosed,
    #[error("The fee is larger than the payment")]
    FeeTooLarge,
}

/// Allocates a payment received on `paid_on`. Order: fee (as entered), then
/// accrued interest, then principal. A payment larger than the payoff is
/// rejected so no unexplained credit balance is ever created.
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
    let pos = position(loan, payments, paid_on);
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
        principal_after: pos.principal_outstanding - principal,
        interest_after: pos.interest_outstanding - interest,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Delinquency {
    /// Scheduled amounts past their due date and not yet covered.
    pub overdue_amount: Cents,
    pub days_overdue: i64,
    /// The oldest installment not fully covered by payments.
    pub next_due: Option<NaiveDate>,
    /// Amount still owed on that installment.
    pub next_due_amount: Cents,
}

/// Delinquency as of `today`. Payments are applied to installments oldest
/// first (by cumulative total). An installment is overdue the day after its
/// due date. After maturity, everything still owed is overdue.
pub fn delinquency(loan: &LoanFacts, schedule: &[Installment], payments: &[PaymentFact], today: NaiveDate) -> Delinquency {
    if loan.status != ContractStatus::Active {
        return Delinquency::default();
    }
    let pos = position(loan, payments, today);
    if pos.payoff() <= 0 {
        return Delinquency::default();
    }
    let paid = pos.paid_total - pos.fees_paid;
    let mut cumulative = 0;
    let mut result = Delinquency::default();
    let mut past_due_scheduled = 0;
    for row in schedule {
        let before = cumulative;
        cumulative += row.total();
        if row.due_date < today {
            past_due_scheduled = cumulative;
        }
        if result.next_due.is_none() && paid < cumulative {
            result.next_due = Some(row.due_date);
            result.next_due_amount = (cumulative - paid.max(before)).min(pos.payoff());
            if row.due_date < today {
                result.days_overdue = days_between(row.due_date, today);
            }
        }
    }
    result.overdue_amount = (past_due_scheduled - paid).max(0).min(pos.payoff());
    if today > loan.maturity_date {
        // Late interest can leave a balance after every installment is covered.
        result.overdue_amount = pos.payoff();
        if result.next_due.is_none() {
            result.next_due = Some(loan.maturity_date);
            result.next_due_amount = pos.payoff();
            result.days_overdue = days_between(loan.maturity_date, today);
        }
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
            } else if delinquency
                .next_due
                .is_some_and(|d| days_between(today, d) <= DUE_SOON_DAYS)
            {
                DisplayStatus::DueSoon
            } else {
                DisplayStatus::Active
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::loans::{Frequency, LoanTerms};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn facts(terms: &LoanTerms) -> LoanFacts {
        LoanFacts {
            principal: terms.principal,
            rate: terms.rate,
            method: terms.method,
            start_date: terms.start_date,
            maturity_date: terms.maturity_date(),
            status: ContractStatus::Active,
            closed_on: None,
        }
    }

    fn pay(loan: &LoanFacts, history: &mut Vec<PaymentFact>, amount: Cents, on: NaiveDate) -> Allocation {
        let a = allocate(loan, history, amount, 0, on, d(2030, 1, 1)).expect("allocates");
        history.push(PaymentFact { paid_on: on, principal: a.principal, interest: a.interest, fee: 0 });
        a
    }

    fn amortized() -> LoanTerms {
        LoanTerms {
            principal: 1_200_000,
            rate: Rate(120_000),
            method: InterestMethod::Amortized,
            frequency: Frequency::Monthly,
            term_count: 12,
            start_date: d(2026, 1, 1),
        }
    }

    #[test]
    fn on_time_payments_match_schedule_exactly() {
        let terms = amortized();
        let loan = facts(&terms);
        let schedule = terms.schedule().unwrap();
        let mut history = Vec::new();
        for row in &schedule {
            let a = pay(&loan, &mut history, row.total(), row.due_date);
            assert_eq!((a.interest, a.principal), (row.interest, row.principal), "row {}", row.seq);
        }
        let pos = position(&loan, &history, d(2027, 6, 1));
        assert_eq!(pos.principal_outstanding, 0);
        assert_eq!(pos.interest_outstanding, 0);
        assert_eq!(pos.principal_paid, 1_200_000);
    }

    #[test]
    fn partial_payment_goes_to_interest_first() {
        let terms = amortized();
        let loan = facts(&terms);
        let mut history = Vec::new();
        // 31 days on 12,000 at 12% = 122.30
        let a = pay(&loan, &mut history, 5_000, d(2026, 2, 1));
        assert_eq!(a.interest, 5_000);
        assert_eq!(a.principal, 0);
        assert_eq!(a.interest_after, 12_230 - 5_000);
        // The unpaid interest stays receivable and does not compound.
        let pos = position(&loan, &history, d(2026, 3, 1));
        // 28 more days on 12,000: 110.47
        assert_eq!(pos.interest_outstanding, 7_230 + 11_047);
        assert_eq!(pos.principal_outstanding, 1_200_000);
    }

    #[test]
    fn early_payment_reduces_principal_and_future_interest() {
        let terms = amortized();
        let loan = facts(&terms);
        let mut history = Vec::new();
        // Pay 6,000 after 10 days: interest 10 days = 39.45, rest to principal.
        let a = pay(&loan, &mut history, 600_000, d(2026, 1, 11));
        assert_eq!(a.interest, 3_945);
        assert_eq!(a.principal, 596_055);
        let pos = position(&loan, &history, d(2026, 2, 1));
        assert_eq!(pos.principal_outstanding, 603_945);
        // 21 days on 6,039.45 = 41.70
        assert_eq!(pos.interest_outstanding, 4_170);
    }

    #[test]
    fn overpayment_is_rejected_with_payoff() {
        let terms = amortized();
        let loan = facts(&terms);
        let history = Vec::new();
        let err = allocate(&loan, &history, 2_000_000, 0, d(2026, 2, 1), d(2030, 1, 1)).unwrap_err();
        assert_eq!(err, PaymentError::Overpayment { payoff: 1_200_000 + 12_230 });
        // Exactly the payoff closes the loan.
        let a = allocate(&loan, &history, 1_212_230, 0, d(2026, 2, 1), d(2030, 1, 1)).unwrap();
        assert!(a.pays_off());
    }

    #[test]
    fn rejects_backdated_future_and_closed() {
        let terms = amortized();
        let mut loan = facts(&terms);
        let mut history = Vec::new();
        pay(&loan, &mut history, 10_000, d(2026, 3, 1));
        let today = d(2026, 4, 1);
        assert_eq!(
            allocate(&loan, &history, 100, 0, d(2026, 2, 1), today),
            Err(PaymentError::BeforeLastPayment { last: d(2026, 3, 1) })
        );
        assert_eq!(allocate(&loan, &history, 100, 0, d(2026, 4, 2), today), Err(PaymentError::InFuture));
        assert_eq!(allocate(&loan, &history, 100, 0, d(2025, 12, 31), today), Err(PaymentError::BeforeStart));
        assert_eq!(allocate(&loan, &history, 0, 0, today, today), Err(PaymentError::NotPositive));
        loan.status = ContractStatus::PaidOff;
        assert_eq!(allocate(&loan, &history, 100, 0, today, today), Err(PaymentError::LoanClosed));
    }

    #[test]
    fn fee_is_allocated_first_and_not_interest() {
        let terms = amortized();
        let loan = facts(&terms);
        let a = allocate(&loan, &[], 15_000, 2_500, d(2026, 2, 1), d(2030, 1, 1)).unwrap();
        assert_eq!(a.fee, 2_500);
        assert_eq!(a.interest, 12_230);
        assert_eq!(a.principal, 15_000 - 2_500 - 12_230);
    }

    #[test]
    fn same_day_payments_accrue_nothing_between() {
        let terms = amortized();
        let loan = facts(&terms);
        let mut history = Vec::new();
        pay(&loan, &mut history, 12_230, d(2026, 2, 1));
        let a = pay(&loan, &mut history, 1_000, d(2026, 2, 1));
        assert_eq!(a.interest, 0);
        assert_eq!(a.principal, 1_000);
    }

    #[test]
    fn delinquency_tracks_oldest_uncovered_installment() {
        let terms = amortized();
        let loan = facts(&terms);
        let schedule = terms.schedule().unwrap();
        let mut history = Vec::new();
        // Nothing paid; first due Feb 1.
        let q = delinquency(&loan, &schedule, &history, d(2026, 1, 28));
        assert_eq!(q.overdue_amount, 0);
        assert_eq!(q.next_due, Some(d(2026, 2, 1)));
        assert_eq!(display_status(&loan, &q, d(2026, 1, 28)), DisplayStatus::DueSoon);
        // Due date itself is not overdue.
        let q = delinquency(&loan, &schedule, &history, d(2026, 2, 1));
        assert_eq!(q.overdue_amount, 0);
        // The day after is.
        let q = delinquency(&loan, &schedule, &history, d(2026, 2, 13));
        assert_eq!(q.overdue_amount, schedule[0].total());
        assert_eq!(q.days_overdue, 12);
        assert_eq!(display_status(&loan, &q, d(2026, 2, 13)), DisplayStatus::Overdue);
        // A partial payment reduces the overdue amount but keeps it overdue.
        pay(&loan, &mut history, 50_000, d(2026, 2, 13));
        let q = delinquency(&loan, &schedule, &history, d(2026, 2, 14));
        assert_eq!(q.overdue_amount, schedule[0].total() - 50_000);
        assert_eq!(q.next_due_amount, schedule[0].total() - 50_000);
        // Paying the rest clears it.
        pay(&loan, &mut history, schedule[0].total() - 50_000, d(2026, 2, 14));
        let q = delinquency(&loan, &schedule, &history, d(2026, 2, 15));
        assert_eq!(q.overdue_amount, 0);
        assert_eq!(q.next_due, Some(d(2026, 3, 1)));
        assert_eq!(display_status(&loan, &q, d(2026, 2, 15)), DisplayStatus::Active);
    }

    #[test]
    fn after_maturity_everything_owed_is_overdue() {
        let terms = LoanTerms {
            principal: 100_000,
            rate: Rate(100_000),
            method: InterestMethod::Simple,
            frequency: Frequency::AtMaturity,
            term_count: 1,
            start_date: d(2026, 1, 1),
        };
        let loan = facts(&terms);
        let schedule = terms.schedule().unwrap();
        let mut history = Vec::new();
        // Pay the scheduled amount 10 days late: 10 more days of interest remain.
        pay(&loan, &mut history, schedule[0].total(), d(2026, 2, 11));
        let q = delinquency(&loan, &schedule, &history, d(2026, 2, 12));
        let pos = position(&loan, &history, d(2026, 2, 12));
        assert!(pos.payoff() > 0);
        assert_eq!(q.overdue_amount, pos.payoff());
        assert_eq!(q.days_overdue, 11);
    }

    #[test]
    fn default_stops_accrual() {
        let terms = amortized();
        let mut loan = facts(&terms);
        loan.status = ContractStatus::Defaulted;
        loan.closed_on = Some(d(2026, 2, 1));
        let pos = position(&loan, &[], d(2026, 12, 1));
        assert_eq!(pos.interest_outstanding, 12_230);
        let q = delinquency(&loan, &terms.schedule().unwrap(), &[], d(2026, 12, 1));
        assert_eq!(q, Delinquency::default());
    }
}
