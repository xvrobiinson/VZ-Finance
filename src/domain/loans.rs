//! Loan contracts and their repayment schedules.
//!
//! A schedule is the *contractual projection* of what is due when, assuming
//! every installment is paid on its due date. Actual balances come from the
//! accrual engine in `domain::ledger`, which uses the same Actual/365 daily
//! interest primitive, so an on-time borrower's allocations match the schedule
//! to the cent.

use chrono::NaiveDate;

use crate::domain::money::{Cents, Rate, interest_for_days};
use crate::utils::dates::{add_months, days_between};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InterestMethod {
    /// Interest accrues daily on the outstanding principal; principal plus
    /// interest is due in one payment at maturity.
    Simple,
    /// Level installments that repay interest and principal over the term.
    Amortized,
    /// Periodic interest-only installments; principal due at maturity.
    InterestOnly,
}

impl InterestMethod {
    pub const ALL: [InterestMethod; 3] = [InterestMethod::Simple, InterestMethod::Amortized, InterestMethod::InterestOnly];

    pub fn key(self) -> &'static str {
        match self {
            InterestMethod::Simple => "simple",
            InterestMethod::Amortized => "amortized",
            InterestMethod::InterestOnly => "interest_only",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "simple" => Some(InterestMethod::Simple),
            "amortized" => Some(InterestMethod::Amortized),
            "interest_only" => Some(InterestMethod::InterestOnly),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            InterestMethod::Simple => "Simple interest",
            InterestMethod::Amortized => "Amortized",
            InterestMethod::InterestOnly => "Interest-only",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            InterestMethod::Simple => "One payment at maturity: principal plus interest accrued daily.",
            InterestMethod::Amortized => "Equal installments that pay down interest and principal.",
            InterestMethod::InterestOnly => "Interest each period; the principal is repaid at maturity.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frequency {
    /// Single payment at maturity. The term is counted in months.
    AtMaturity,
    Weekly,
    Biweekly,
    Monthly,
    Quarterly,
}

impl Frequency {
    pub const PERIODIC: [Frequency; 4] = [Frequency::Weekly, Frequency::Biweekly, Frequency::Monthly, Frequency::Quarterly];

    pub fn key(self) -> &'static str {
        match self {
            Frequency::AtMaturity => "at_maturity",
            Frequency::Weekly => "weekly",
            Frequency::Biweekly => "biweekly",
            Frequency::Monthly => "monthly",
            Frequency::Quarterly => "quarterly",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "at_maturity" => Some(Frequency::AtMaturity),
            "weekly" => Some(Frequency::Weekly),
            "biweekly" => Some(Frequency::Biweekly),
            "monthly" => Some(Frequency::Monthly),
            "quarterly" => Some(Frequency::Quarterly),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Frequency::AtMaturity => "At maturity",
            Frequency::Weekly => "Weekly",
            Frequency::Biweekly => "Every 2 weeks",
            Frequency::Monthly => "Monthly",
            Frequency::Quarterly => "Quarterly",
        }
    }
    /// Noun for the term count ("payments" or "months").
    pub fn term_unit(self, n: u32) -> &'static str {
        match (self, n == 1) {
            (Frequency::AtMaturity, true) => "month",
            (Frequency::AtMaturity, false) => "months",
            (_, true) => "payment",
            (_, false) => "payments",
        }
    }
    fn periods_per_year(self) -> f64 {
        match self {
            Frequency::AtMaturity => 1.0,
            Frequency::Weekly => 52.0,
            Frequency::Biweekly => 26.0,
            Frequency::Monthly => 12.0,
            Frequency::Quarterly => 4.0,
        }
    }
    /// Due date of period `k` (1-based), always computed from the start date
    /// so month-end clamping never drifts.
    pub fn due_date(self, start: NaiveDate, k: u32) -> NaiveDate {
        match self {
            Frequency::AtMaturity | Frequency::Monthly => add_months(start, k),
            Frequency::Quarterly => add_months(start, 3 * k),
            Frequency::Weekly => start + chrono::Duration::days(7 * k as i64),
            Frequency::Biweekly => start + chrono::Duration::days(14 * k as i64),
        }
    }
}

/// The contractual terms needed to build a schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoanTerms {
    pub principal: Cents,
    pub rate: Rate,
    pub method: InterestMethod,
    pub frequency: Frequency,
    /// Number of installments, or months for `Frequency::AtMaturity`.
    pub term_count: u32,
    pub start_date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installment {
    pub seq: u32,
    pub due_date: NaiveDate,
    pub principal: Cents,
    pub interest: Cents,
}

impl Installment {
    pub fn total(&self) -> Cents {
        self.principal + self.interest
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TermsError {
    #[error("Principal must be greater than zero")]
    Principal,
    #[error("Term must be between 1 and {max}")]
    Term { max: u32 },
    #[error("Simple-interest loans are repaid in one payment at maturity")]
    SimpleNeedsMaturity,
    #[error("Choose a repayment frequency")]
    PeriodicNeedsFrequency,
}

pub const MAX_TERM_COUNT: u32 = 600;

impl LoanTerms {
    pub fn validate(&self) -> Result<(), TermsError> {
        if self.principal <= 0 {
            return Err(TermsError::Principal);
        }
        if self.term_count == 0 || self.term_count > MAX_TERM_COUNT {
            return Err(TermsError::Term { max: MAX_TERM_COUNT });
        }
        match (self.method, self.frequency) {
            (InterestMethod::Simple, Frequency::AtMaturity) => Ok(()),
            (InterestMethod::Simple, _) => Err(TermsError::SimpleNeedsMaturity),
            (_, Frequency::AtMaturity) => Err(TermsError::PeriodicNeedsFrequency),
            _ => Ok(()),
        }
    }

    pub fn maturity_date(&self) -> NaiveDate {
        self.frequency.due_date(self.start_date, self.term_count)
    }

    /// Builds the contractual schedule. See `docs/ACCOUNTING.md` §Schedules.
    pub fn schedule(&self) -> Result<Vec<Installment>, TermsError> {
        self.validate()?;
        let p = self.principal;
        let r = self.rate;
        let n = self.term_count;
        let mut rows = Vec::with_capacity(n as usize);
        match self.method {
            InterestMethod::Simple => {
                let due = self.maturity_date();
                let interest = interest_for_days(p, r, days_between(self.start_date, due));
                rows.push(Installment { seq: 1, due_date: due, principal: p, interest });
            }
            InterestMethod::InterestOnly => {
                let mut prev = self.start_date;
                for k in 1..=n {
                    let due = self.frequency.due_date(self.start_date, k);
                    let interest = interest_for_days(p, r, days_between(prev, due));
                    let principal = if k == n { p } else { 0 };
                    rows.push(Installment { seq: k, due_date: due, principal, interest });
                    prev = due;
                }
            }
            InterestMethod::Amortized => {
                let level = level_payment(p, r, n, self.frequency);
                let mut balance = p;
                let mut prev = self.start_date;
                for k in 1..=n {
                    let due = self.frequency.due_date(self.start_date, k);
                    let interest = interest_for_days(balance, r, days_between(prev, due));
                    let principal = if k == n { balance } else { (level - interest).clamp(0, balance) };
                    rows.push(Installment { seq: k, due_date: due, principal, interest });
                    balance -= principal;
                    prev = due;
                }
            }
        }
        Ok(rows)
    }
}

/// Level installment for an amortized loan: the standard annuity payment at
/// the periodic rate (annual / periods per year), rounded half-up to the cent.
/// This is the only place a float is used, and only to choose the contractual
/// installment amount; every balance is then computed in integer cents.
pub fn level_payment(principal: Cents, rate: Rate, n: u32, frequency: Frequency) -> Cents {
    if n == 0 {
        return principal;
    }
    if rate.0 == 0 {
        return (principal + n as i64 - 1) / n as i64;
    }
    let i = rate.0 as f64 / 1_000_000.0 / frequency.periods_per_year();
    let p = principal as f64;
    let payment = p * i / (1.0 - (1.0 + i).powi(-(n as i32)));
    payment.round() as Cents
}

pub fn schedule_totals(rows: &[Installment]) -> (Cents, Cents) {
    rows.iter().fold((0, 0), |(p, i), r| (p + r.principal, i + r.interest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn simple_interest_single_payment() {
        let t = LoanTerms {
            principal: 1_000_000,
            rate: Rate(120_000),
            method: InterestMethod::Simple,
            frequency: Frequency::AtMaturity,
            term_count: 6,
            start_date: d(2026, 1, 15),
        };
        let s = t.schedule().unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].due_date, d(2026, 7, 15));
        // 181 days: 10,000 * 12% * 181/365 = 595.068... -> 595.07
        assert_eq!(s[0].interest, 59_507);
        assert_eq!(s[0].principal, 1_000_000);
    }

    #[test]
    fn interest_only_schedule() {
        let t = LoanTerms {
            principal: 500_000,
            rate: Rate(100_000),
            method: InterestMethod::InterestOnly,
            frequency: Frequency::Monthly,
            term_count: 3,
            start_date: d(2026, 1, 1),
        };
        let s = t.schedule().unwrap();
        assert_eq!(s.iter().map(|r| r.principal).collect::<Vec<_>>(), vec![0, 0, 500_000]);
        // Jan 31 days, Feb 28, Mar 31 at 5,000.00 * 10%
        assert_eq!(s.iter().map(|r| r.interest).collect::<Vec<_>>(), vec![4_247, 3_836, 4_247]);
    }

    #[test]
    fn amortized_schedule_repays_exactly() {
        let t = LoanTerms {
            principal: 1_200_000,
            rate: Rate(120_000),
            method: InterestMethod::Amortized,
            frequency: Frequency::Monthly,
            term_count: 12,
            start_date: d(2026, 1, 1),
        };
        let level = level_payment(t.principal, t.rate, 12, Frequency::Monthly);
        // 12,000 at 1%/month for 12 months = 1,066.19
        assert_eq!(level, 106_619);
        let s = t.schedule().unwrap();
        let (p, _) = schedule_totals(&s);
        assert_eq!(p, 1_200_000);
        assert_eq!(s.len(), 12);
        for row in &s[..11] {
            assert_eq!(row.total(), level);
        }
        // last installment absorbs rounding and day-count differences
        assert!((s[11].total() - level).abs() < 500);
        assert_eq!(s[11].due_date, d(2027, 1, 1));
    }

    #[test]
    fn zero_rate_amortized() {
        let t = LoanTerms {
            principal: 100_000,
            rate: Rate(0),
            method: InterestMethod::Amortized,
            frequency: Frequency::Monthly,
            term_count: 3,
            start_date: d(2026, 1, 1),
        };
        let s = t.schedule().unwrap();
        assert_eq!(s.iter().map(|r| r.principal).collect::<Vec<_>>(), vec![33_334, 33_334, 33_332]);
        assert!(s.iter().all(|r| r.interest == 0));
    }

    #[test]
    fn rejects_mixed_methods() {
        let mut t = LoanTerms {
            principal: 100_000,
            rate: Rate(0),
            method: InterestMethod::Simple,
            frequency: Frequency::Monthly,
            term_count: 3,
            start_date: d(2026, 1, 1),
        };
        assert_eq!(t.validate(), Err(TermsError::SimpleNeedsMaturity));
        t.method = InterestMethod::Amortized;
        t.frequency = Frequency::AtMaturity;
        assert_eq!(t.validate(), Err(TermsError::PeriodicNeedsFrequency));
        t.frequency = Frequency::Monthly;
        t.term_count = 0;
        assert!(t.validate().is_err());
    }

    #[test]
    fn month_end_due_dates_do_not_drift() {
        let f = Frequency::Monthly;
        let start = d(2026, 1, 31);
        assert_eq!(f.due_date(start, 1), d(2026, 2, 28));
        assert_eq!(f.due_date(start, 2), d(2026, 3, 31));
    }
}
