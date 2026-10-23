//! Loan contracts: a principal lent at a fixed percentage per period
//! (for example 10% a month), paid monthly or every 15 days. Each period's
//! interest is that percentage of the capital still owed when the period
//! begins. Loans stay open until the capital is fully repaid.

use chrono::NaiveDate;

use crate::domain::money::{Cents, RATE_SCALE, Rate, div_round_half_up};
use crate::i18n::t;
use crate::utils::dates::add_months;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frequency {
    Monthly,
    /// Every 15 days (quincenal): the start day and 15 days later, each month.
    Biweekly,
}

impl Frequency {
    pub const ALL: [Frequency; 2] = [Frequency::Monthly, Frequency::Biweekly];

    pub fn key(self) -> &'static str {
        match self {
            Frequency::Monthly => "monthly",
            Frequency::Biweekly => "biweekly",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "monthly" => Some(Frequency::Monthly),
            "biweekly" => Some(Frequency::Biweekly),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Frequency::Monthly => t("Monthly", "Mensual"),
            Frequency::Biweekly => t("Every 15 days", "Quincenal"),
        }
    }
    /// "a month" / "mensual", used after a rate: "10% a month".
    pub fn per(self) -> &'static str {
        match self {
            Frequency::Monthly => t("a month", "mensual"),
            Frequency::Biweekly => t("every 15 days", "quincenal"),
        }
    }
    pub fn periods_per_month(self) -> i64 {
        match self {
            Frequency::Monthly => 1,
            Frequency::Biweekly => 2,
        }
    }
    /// End (due date) of period `k`, 1-based. Always counted from the start
    /// date so month-end dates never drift.
    pub fn due_date(self, start: NaiveDate, k: u32) -> NaiveDate {
        match self {
            Frequency::Monthly => add_months(start, k),
            Frequency::Biweekly => {
                let base = add_months(start, k / 2);
                if k % 2 == 1 { base + chrono::Duration::days(15) } else { base }
            }
        }
    }
    /// Start of period `k` (the previous period's due date, or the loan start).
    pub fn period_start(self, start: NaiveDate, k: u32) -> NaiveDate {
        if k <= 1 { start } else { self.due_date(start, k - 1) }
    }
}

/// Common rates offered as one-click choices.
pub const RATE_PRESETS: [Rate; 3] = [Rate(50_000), Rate(100_000), Rate(150_000)];

/// Highest rate accepted per period: 100%.
pub const MAX_PERIOD_RATE: Rate = Rate(RATE_SCALE);

/// Interest for one period on `balance`, rounded half-up to the cent.
pub fn period_interest(balance: Cents, rate: Rate) -> Cents {
    if balance <= 0 || rate.0 <= 0 {
        return 0;
    }
    div_round_half_up(balance as i128 * rate.0 as i128, RATE_SCALE as i128) as Cents
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoanTerms {
    pub principal: Cents,
    /// Percentage charged per period.
    pub rate: Rate,
    pub frequency: Frequency,
    pub start_date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TermsError {
    #[error("principal")]
    Principal,
    #[error("rate")]
    Rate,
}

impl TermsError {
    pub fn message(&self) -> &'static str {
        match self {
            TermsError::Principal => t("Enter an amount greater than zero", "Escribe un monto mayor que cero"),
            TermsError::Rate => t("The rate must be between 0% and 100% per period", "La tasa debe estar entre 0% y 100% por período"),
        }
    }
}

impl LoanTerms {
    pub fn validate(&self) -> Result<(), TermsError> {
        if self.principal <= 0 {
            return Err(TermsError::Principal);
        }
        if self.rate.0 < 0 || self.rate > MAX_PERIOD_RATE {
            return Err(TermsError::Rate);
        }
        Ok(())
    }

    /// Interest of the first period, on the full principal.
    pub fn interest_per_period(&self) -> Cents {
        period_interest(self.principal, self.rate)
    }

    pub fn first_due(&self) -> NaiveDate {
        self.frequency.due_date(self.start_date, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn ten_percent_of_a_thousand_is_a_hundred() {
        assert_eq!(period_interest(100_000, Rate(100_000)), 10_000);
        assert_eq!(period_interest(100_000, Rate(50_000)), 5_000);
        assert_eq!(period_interest(100_000, Rate(150_000)), 15_000);
        // Half a cent rounds up: 10% of 0.05 = 0.005
        assert_eq!(period_interest(5, Rate(100_000)), 1);
        assert_eq!(period_interest(0, Rate(100_000)), 0);
    }

    #[test]
    fn due_dates() {
        let s = d(2026, 1, 31);
        assert_eq!(Frequency::Monthly.due_date(s, 1), d(2026, 2, 28));
        assert_eq!(Frequency::Monthly.due_date(s, 2), d(2026, 3, 31));
        let s = d(2026, 1, 5);
        assert_eq!(Frequency::Biweekly.due_date(s, 1), d(2026, 1, 20));
        assert_eq!(Frequency::Biweekly.due_date(s, 2), d(2026, 2, 5));
        assert_eq!(Frequency::Biweekly.due_date(s, 3), d(2026, 2, 20));
        assert_eq!(Frequency::Biweekly.period_start(s, 1), s);
        assert_eq!(Frequency::Biweekly.period_start(s, 3), d(2026, 2, 5));
    }

    #[test]
    fn validates() {
        let mut t = LoanTerms { principal: 100_000, rate: Rate(100_000), frequency: Frequency::Monthly, start_date: d(2026, 1, 1) };
        assert!(t.validate().is_ok());
        assert_eq!(t.interest_per_period(), 10_000);
        t.rate = Rate(1_000_001);
        assert_eq!(t.validate(), Err(TermsError::Rate));
        t.rate = Rate(0);
        t.principal = 0;
        assert_eq!(t.validate(), Err(TermsError::Principal));
    }
}
