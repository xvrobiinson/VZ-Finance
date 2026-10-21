//! Portfolio accounting: the capital cash ledger, portfolio value, returns,
//! history and monthly cash flow. Everything is derived from persisted
//! transactions (capital movements, loan disbursements, payments).
//!
//! Definitions (see `docs/ACCOUNTING.md`):
//! - Net capital invested = contributions - withdrawals.
//! - Available cash = net capital invested - principal disbursed + all
//!   payments received (principal, interest and fees).
//! - Portfolio value = available cash + outstanding principal on performing
//!   loans + accrued interest receivable on performing loans. Defaulted
//!   balances are excluded (written down) and reported separately.
//! - Portfolio gain = portfolio value - net capital invested.
//! - Return on invested capital = gain / net capital invested (simple, not
//!   annualized).

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::domain::ledger::{
    ContractStatus, Delinquency, DisplayStatus, LoanFacts, PaymentFact, Position, delinquency, display_status, position,
};
use crate::domain::loans::{Installment, schedule_totals};
use crate::domain::money::Cents;
use crate::utils::dates::{add_months, month_start};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapitalKind {
    Contribution,
    Withdrawal,
}

impl CapitalKind {
    pub fn key(self) -> &'static str {
        match self {
            CapitalKind::Contribution => "contribution",
            CapitalKind::Withdrawal => "withdrawal",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "contribution" => Some(CapitalKind::Contribution),
            "withdrawal" => Some(CapitalKind::Withdrawal),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            CapitalKind::Contribution => "Capital added",
            CapitalKind::Withdrawal => "Capital withdrawn",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CapitalFact {
    pub kind: CapitalKind,
    pub amount: Cents,
    pub occurred_on: NaiveDate,
}

impl CapitalFact {
    fn signed(&self) -> Cents {
        match self.kind {
            CapitalKind::Contribution => self.amount,
            CapitalKind::Withdrawal => -self.amount,
        }
    }
}

/// One loan with everything needed to evaluate it.
#[derive(Clone, Debug)]
pub struct LoanBook {
    pub id: i64,
    pub client_id: i64,
    pub facts: LoanFacts,
    pub schedule: Vec<Installment>,
    pub payments: Vec<PaymentFact>,
}

impl LoanBook {
    pub fn position(&self, as_of: NaiveDate) -> Position {
        position(&self.facts, &self.payments, as_of)
    }
    pub fn delinquency(&self, today: NaiveDate) -> Delinquency {
        delinquency(&self.facts, &self.schedule, &self.payments, today)
    }
    pub fn display_status(&self, today: NaiveDate) -> DisplayStatus {
        display_status(&self.facts, &self.delinquency(today), today)
    }
    /// Status as it was on `as_of`, for history: a loan defaulted later was
    /// still performing before its default date.
    fn performing_on(&self, as_of: NaiveDate) -> bool {
        match (self.facts.status, self.facts.closed_on) {
            (ContractStatus::Defaulted, Some(d)) => as_of < d,
            (ContractStatus::Defaulted, None) => false,
            _ => true,
        }
    }
    pub fn scheduled_interest(&self) -> Cents {
        schedule_totals(&self.schedule).1
    }
}

#[derive(Clone, Debug, Default)]
pub struct Book {
    pub loans: Vec<LoanBook>,
    pub capital: Vec<CapitalFact>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub contributions: Cents,
    pub withdrawals: Cents,
    pub principal_disbursed: Cents,
    pub principal_repaid: Cents,
    pub interest_collected: Cents,
    pub fees_collected: Cents,
    pub available_cash: Cents,
    /// Outstanding principal on performing (non-defaulted) loans.
    pub outstanding_principal: Cents,
    /// Accrued, unpaid interest on performing loans.
    pub interest_receivable: Cents,
    pub defaulted_principal: Cents,
    pub defaulted_interest: Cents,
    pub portfolio_value: Cents,
    pub active_loans: usize,
    pub overdue_loans: usize,
    pub due_soon_loans: usize,
    pub overdue_amount: Cents,
    /// Contractual interest not yet collected on active loans (projection).
    pub expected_interest_remaining: Cents,
}

impl Summary {
    pub fn net_invested(&self) -> Cents {
        self.contributions - self.withdrawals
    }
    pub fn gain(&self) -> Cents {
        self.portfolio_value - self.net_invested()
    }
    pub fn total_received(&self) -> Cents {
        self.principal_repaid + self.interest_collected + self.fees_collected
    }
    /// Realized income: interest and fees actually received.
    pub fn realized_income(&self) -> Cents {
        self.interest_collected + self.fees_collected
    }
}

impl Book {
    /// The portfolio position at the end of `as_of`, with delinquency judged
    /// against `as_of` as "today".
    pub fn summary(&self, as_of: NaiveDate) -> Summary {
        let mut s = Summary::default();
        for c in self.capital.iter().filter(|c| c.occurred_on <= as_of) {
            match c.kind {
                CapitalKind::Contribution => s.contributions += c.amount,
                CapitalKind::Withdrawal => s.withdrawals += c.amount,
            }
        }
        for loan in &self.loans {
            if loan.facts.start_date > as_of {
                continue;
            }
            s.principal_disbursed += loan.facts.principal;
            let pos = loan.position(as_of);
            s.principal_repaid += pos.principal_paid;
            s.interest_collected += pos.interest_paid;
            s.fees_collected += pos.fees_paid;
            if loan.performing_on(as_of) {
                s.outstanding_principal += pos.principal_outstanding;
                s.interest_receivable += pos.interest_outstanding;
            } else {
                s.defaulted_principal += pos.principal_outstanding;
                s.defaulted_interest += pos.interest_outstanding;
            }
            let active = loan.facts.status == ContractStatus::Active && pos.payoff() > 0;
            if active {
                s.active_loans += 1;
                let q = loan.delinquency(as_of);
                s.overdue_amount += q.overdue_amount;
                match display_status(&loan.facts, &q, as_of) {
                    DisplayStatus::Overdue => s.overdue_loans += 1,
                    DisplayStatus::DueSoon => s.due_soon_loans += 1,
                    _ => {}
                }
                s.expected_interest_remaining += (loan.scheduled_interest() - pos.interest_paid).max(0);
            }
        }
        s.available_cash = s.net_invested() - s.principal_disbursed + s.total_received();
        s.portfolio_value = s.available_cash + s.outstanding_principal + s.interest_receivable;
        s
    }

    /// Cash available at the end of `as_of` (used to validate disbursements
    /// and withdrawals).
    pub fn cash_on(&self, as_of: NaiveDate) -> Cents {
        let capital: Cents = self.capital.iter().filter(|c| c.occurred_on <= as_of).map(|c| c.signed()).sum();
        let disbursed: Cents = self.loans.iter().filter(|l| l.facts.start_date <= as_of).map(|l| l.facts.principal).sum();
        let received: Cents = self
            .loans
            .iter()
            .flat_map(|l| l.payments.iter())
            .filter(|p| p.paid_on <= as_of)
            .map(|p| p.total())
            .sum();
        capital - disbursed + received
    }

    /// The lowest cash balance on or after `from`, given all recorded
    /// movements. A new outflow dated `from` must not exceed this, or cash
    /// would go negative at some later point in the ledger.
    pub fn min_cash_from(&self, from: NaiveDate) -> Cents {
        let mut dates: Vec<NaiveDate> = vec![from];
        dates.extend(self.capital.iter().map(|c| c.occurred_on).filter(|d| *d > from));
        dates.extend(self.loans.iter().map(|l| l.facts.start_date).filter(|d| *d > from));
        dates.sort();
        dates.dedup();
        dates.into_iter().map(|d| self.cash_on(d)).min().unwrap_or(0)
    }

    pub fn first_activity(&self) -> Option<NaiveDate> {
        let a = self.capital.iter().map(|c| c.occurred_on).min();
        let b = self.loans.iter().map(|l| l.facts.start_date).min();
        match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Daily or weekly points between `from` and `to` (inclusive), always
    /// including `to`. Values are end-of-day positions, never interpolated.
    pub fn history(&self, from: NaiveDate, to: NaiveDate) -> Vec<HistoryPoint> {
        if from > to {
            return Vec::new();
        }
        let days = (to - from).num_days();
        let step = if days <= 120 { 1 } else if days <= 400 { 3 } else { 7 };
        let mut points = Vec::new();
        let mut d = from;
        while d < to {
            points.push(self.point(d));
            d += chrono::Duration::days(step);
        }
        points.push(self.point(to));
        points
    }

    fn point(&self, d: NaiveDate) -> HistoryPoint {
        let s = self.summary(d);
        HistoryPoint {
            date: d,
            portfolio_value: s.portfolio_value,
            net_invested: s.net_invested(),
            interest_collected: s.interest_collected,
        }
    }

    /// Cash flow per calendar month from the first activity through `to`.
    pub fn monthly_cash_flow(&self, to: NaiveDate) -> Vec<MonthFlow> {
        let Some(first) = self.first_activity() else {
            return Vec::new();
        };
        let mut months: BTreeMap<NaiveDate, MonthFlow> = BTreeMap::new();
        let mut m = month_start(first);
        while m <= to {
            months.insert(m, MonthFlow { month: m, ..Default::default() });
            m = add_months(m, 1);
        }
        for c in self.capital.iter().filter(|c| c.occurred_on <= to) {
            if let Some(f) = months.get_mut(&month_start(c.occurred_on)) {
                match c.kind {
                    CapitalKind::Contribution => f.contributions += c.amount,
                    CapitalKind::Withdrawal => f.withdrawals += c.amount,
                }
            }
        }
        for l in self.loans.iter().filter(|l| l.facts.start_date <= to) {
            if let Some(f) = months.get_mut(&month_start(l.facts.start_date)) {
                f.disbursed += l.facts.principal;
            }
            for p in l.payments.iter().filter(|p| p.paid_on <= to) {
                if let Some(f) = months.get_mut(&month_start(p.paid_on)) {
                    f.principal_returned += p.principal;
                    f.interest_received += p.interest;
                    f.fees_received += p.fee;
                }
            }
        }
        let mut cash = 0;
        months
            .into_values()
            .map(|mut f| {
                cash += f.net();
                f.ending_cash = cash;
                f
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPoint {
    pub date: NaiveDate,
    pub portfolio_value: Cents,
    pub net_invested: Cents,
    pub interest_collected: Cents,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MonthFlow {
    pub month: NaiveDate,
    pub contributions: Cents,
    pub withdrawals: Cents,
    pub disbursed: Cents,
    pub principal_returned: Cents,
    pub interest_received: Cents,
    pub fees_received: Cents,
    pub ending_cash: Cents,
}

impl MonthFlow {
    pub fn net(&self) -> Cents {
        self.contributions - self.withdrawals - self.disbursed + self.principal_returned + self.interest_received + self.fees_received
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ledger::allocate;
    use crate::domain::loans::{Frequency, InterestMethod, LoanTerms};
    use crate::domain::money::Rate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn loan_book(id: i64, terms: LoanTerms) -> LoanBook {
        LoanBook {
            id,
            client_id: 1,
            facts: LoanFacts {
                principal: terms.principal,
                rate: terms.rate,
                method: terms.method,
                start_date: terms.start_date,
                maturity_date: terms.maturity_date(),
                status: ContractStatus::Active,
                closed_on: None,
            },
            schedule: terms.schedule().unwrap(),
            payments: Vec::new(),
        }
    }

    fn pay(l: &mut LoanBook, amount: Cents, on: NaiveDate) {
        let a = allocate(&l.facts, &l.payments, amount, 0, on, d(2030, 1, 1)).unwrap();
        l.payments.push(PaymentFact { paid_on: on, principal: a.principal, interest: a.interest, fee: 0 });
        if a.pays_off() {
            l.facts.status = ContractStatus::PaidOff;
        }
    }

    /// A fixture with known results:
    /// - Jan 1: contribute 20,000.00
    /// - Jan 1: lend 10,000.00 simple, 12%, 12 months (365 days)
    /// - Jul 1: borrower pays 2,000.00 (interest 595.07 for 181 days, principal 1,404.93)
    /// - Aug 1: withdraw 1,000.00
    fn fixture() -> Book {
        let mut l = loan_book(
            1,
            LoanTerms {
                principal: 1_000_000,
                rate: Rate(120_000),
                method: InterestMethod::Simple,
                frequency: Frequency::AtMaturity,
                term_count: 12,
                start_date: d(2026, 1, 1),
            },
        );
        pay(&mut l, 200_000, d(2026, 7, 1));
        Book {
            loans: vec![l],
            capital: vec![
                CapitalFact { kind: CapitalKind::Contribution, amount: 2_000_000, occurred_on: d(2026, 1, 1) },
                CapitalFact { kind: CapitalKind::Withdrawal, amount: 100_000, occurred_on: d(2026, 8, 1) },
            ],
        }
    }

    #[test]
    fn disbursement_is_not_a_loss_and_repayment_is_not_income() {
        let book = fixture();
        let s = book.summary(d(2026, 1, 1));
        assert_eq!(s.available_cash, 1_000_000);
        assert_eq!(s.outstanding_principal, 1_000_000);
        assert_eq!(s.portfolio_value, 2_000_000);
        assert_eq!(s.gain(), 0);

        let s = book.summary(d(2026, 7, 1));
        assert_eq!(s.interest_collected, 59_507);
        assert_eq!(s.principal_repaid, 200_000 - 59_507);
        assert_eq!(s.realized_income(), 59_507);
        assert_eq!(s.interest_receivable, 0);
        assert_eq!(s.gain(), 59_507);
    }

    #[test]
    fn withdrawal_is_not_a_loss() {
        let book = fixture();
        let before = book.summary(d(2026, 7, 31));
        let after = book.summary(d(2026, 8, 1));
        assert_eq!(after.net_invested(), 1_900_000);
        // One day of interest accrued between the two dates; the withdrawal
        // itself does not change the gain.
        let one_day = after.interest_receivable - before.interest_receivable;
        assert_eq!(after.gain() - before.gain(), one_day);
    }

    #[test]
    fn ledger_invariants_hold_every_day() {
        let book = fixture();
        let mut day = d(2025, 12, 30);
        while day <= d(2027, 3, 1) {
            let s = book.summary(day);
            // Cash + all principal still owed = capital + income received.
            assert_eq!(
                s.available_cash + s.outstanding_principal + s.defaulted_principal,
                s.net_invested() + s.interest_collected + s.fees_collected,
                "{day}"
            );
            // Gain = realized income + receivable - write-downs.
            assert_eq!(s.gain(), s.realized_income() + s.interest_receivable - s.defaulted_principal, "{day}");
            assert_eq!(book.cash_on(day), s.available_cash, "{day}");
            day += chrono::Duration::days(1);
        }
    }

    #[test]
    fn default_writes_down_the_balance() {
        let mut book = fixture();
        book.loans[0].facts.status = ContractStatus::Defaulted;
        book.loans[0].facts.closed_on = Some(d(2026, 9, 1));
        let before = book.summary(d(2026, 8, 31));
        assert_eq!(before.defaulted_principal, 0);
        let s = book.summary(d(2026, 10, 1));
        assert_eq!(s.outstanding_principal, 0);
        assert_eq!(s.defaulted_principal, 1_000_000 - (200_000 - 59_507));
        assert_eq!(s.active_loans, 0);
        assert_eq!(s.portfolio_value, s.available_cash);
    }

    #[test]
    fn overdue_totals_roll_up() {
        let book = fixture();
        let s = book.summary(d(2027, 1, 11));
        // Simple loan matured Jan 1 2027 with a balance outstanding.
        assert_eq!(s.overdue_loans, 1);
        let pos = book.loans[0].position(d(2027, 1, 11));
        assert_eq!(s.overdue_amount, pos.payoff());
    }

    #[test]
    fn history_uses_real_end_of_day_values() {
        let book = fixture();
        let h = book.history(d(2026, 1, 1), d(2026, 3, 1));
        assert_eq!(h.first().unwrap().date, d(2026, 1, 1));
        assert_eq!(h.last().unwrap().date, d(2026, 3, 1));
        assert_eq!(h.len(), 60);
        for p in &h {
            assert_eq!(p.portfolio_value, book.summary(p.date).portfolio_value);
        }
        assert!(h.windows(2).all(|w| w[1].portfolio_value >= w[0].portfolio_value));
    }

    #[test]
    fn monthly_cash_flow_reconciles_to_cash() {
        let book = fixture();
        let flows = book.monthly_cash_flow(d(2026, 9, 15));
        assert_eq!(flows.len(), 9);
        assert_eq!(flows[0].contributions, 2_000_000);
        assert_eq!(flows[0].disbursed, 1_000_000);
        assert_eq!(flows[6].interest_received, 59_507);
        assert_eq!(flows[7].withdrawals, 100_000);
        assert_eq!(flows.last().unwrap().ending_cash, book.cash_on(d(2026, 9, 15)));
    }

    #[test]
    fn min_cash_guards_future_outflows() {
        let book = fixture();
        // Cash on Jan 1 is 10,000; the Aug 1 withdrawal leaves less later.
        assert_eq!(book.cash_on(d(2026, 1, 1)), 1_000_000);
        assert_eq!(book.min_cash_from(d(2026, 1, 1)), 1_000_000);
        assert_eq!(book.min_cash_from(d(2026, 7, 15)), 1_100_000);
    }
}
