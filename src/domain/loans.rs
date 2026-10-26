//! Loan contracts. A loan is a principal lent at a percentage per **interest
//! period** (for example 10% a month) and repaid on a separate **repayment
//! frequency** (daily, weekly, every 15 days or monthly). When the two differ,
//! the rate is converted in proportion to their lengths on a 30-day month
//! (10% a month paid weekly is 10% × 7/30 each week).
//!
//! Three methods:
//! - **Declining balance**: a fixed number of installments, each repaying an
//!   equal share of the capital; each period's interest is charged on the
//!   capital still owed when the period begins.
//! - **Flat interest**: a fixed number of installments; each period's interest
//!   is charged on the original principal.
//! - **Interest only**: no fixed term; each period's interest is charged on the
//!   capital still owed and capital is repaid whenever the client chooses.

use chrono::NaiveDate;

use crate::domain::money::{Cents, RATE_SCALE, Rate, div_round_half_up};
use crate::i18n::t;
use crate::tr;
use crate::utils::dates::add_months;

/// How often something happens: payments fall due, or interest is quoted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frequency {
    Daily,
    Weekly,
    /// Every 15 days (quincenal): the start day and 15 days later, each month.
    Biweekly,
    Monthly,
}

impl Frequency {
    pub const ALL: [Frequency; 4] = [Frequency::Daily, Frequency::Weekly, Frequency::Biweekly, Frequency::Monthly];

    pub fn key(self) -> &'static str {
        match self {
            Frequency::Daily => "daily",
            Frequency::Weekly => "weekly",
            Frequency::Biweekly => "biweekly",
            Frequency::Monthly => "monthly",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "daily" => Some(Frequency::Daily),
            "weekly" => Some(Frequency::Weekly),
            "biweekly" => Some(Frequency::Biweekly),
            "monthly" => Some(Frequency::Monthly),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Frequency::Daily => t("Daily", "Diario"),
            Frequency::Weekly => t("Weekly", "Semanal"),
            Frequency::Biweekly => t("Every 15 days", "Quincenal"),
            Frequency::Monthly => t("Monthly", "Mensual"),
        }
    }
    /// "a month" / "mensual", used after a rate: "10% a month".
    pub fn per(self) -> &'static str {
        match self {
            Frequency::Daily => t("a day", "diario"),
            Frequency::Weekly => t("a week", "semanal"),
            Frequency::Biweekly => t("every 15 days", "quincenal"),
            Frequency::Monthly => t("a month", "mensual"),
        }
    }
    /// Length in days for converting rates, on a 30-day month.
    pub fn days(self) -> i64 {
        match self {
            Frequency::Daily => 1,
            Frequency::Weekly => 7,
            Frequency::Biweekly => 15,
            Frequency::Monthly => 30,
        }
    }
    /// End (due date) of period `k`, 1-based. Always counted from the start
    /// date so month-end dates never drift.
    pub fn due_date(self, start: NaiveDate, k: u32) -> NaiveDate {
        match self {
            Frequency::Daily => start + chrono::Duration::days(k as i64),
            Frequency::Weekly => start + chrono::Duration::days(7 * k as i64),
            Frequency::Biweekly => {
                let base = add_months(start, k / 2);
                if k % 2 == 1 { base + chrono::Duration::days(15) } else { base }
            }
            Frequency::Monthly => add_months(start, k),
        }
    }
    /// Start of period `k` (the previous period's due date, or the loan start).
    pub fn period_start(self, start: NaiveDate, k: u32) -> NaiveDate {
        if k <= 1 { start } else { self.due_date(start, k - 1) }
    }
}

/// An amount per period `per` turned into an amount per period `every`, on a
/// 30-day month, rounded half-up to the cent.
pub fn convert_per_period(amount: Cents, per: Frequency, every: Frequency) -> Cents {
    div_round_half_up(amount as i128 * every.days() as i128, per.days() as i128) as Cents
}

/// How a loan is repaid and its interest worked out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    Declining,
    Flat,
    InterestOnly,
}

impl Method {
    pub const ALL: [Method; 3] = [Method::Declining, Method::Flat, Method::InterestOnly];

    pub fn key(self) -> &'static str {
        match self {
            Method::Declining => "declining",
            Method::Flat => "flat",
            Method::InterestOnly => "interest_only",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "declining" => Some(Method::Declining),
            "flat" => Some(Method::Flat),
            "interest_only" => Some(Method::InterestOnly),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Method::Declining => t("Installments · declining balance", "Cuotas · saldo decreciente"),
            Method::Flat => t("Installments · flat interest", "Cuotas · interés fijo"),
            Method::InterestOnly => t("Interest only · no fixed term", "Solo interés · sin plazo"),
        }
    }
    pub fn short_label(self) -> &'static str {
        match self {
            Method::Declining => t("Declining balance", "Saldo decreciente"),
            Method::Flat => t("Flat interest", "Interés fijo"),
            Method::InterestOnly => t("Interest only", "Solo interés"),
        }
    }
    /// One sentence on how interest is worked out.
    pub fn explanation(self) -> &'static str {
        match self {
            Method::Declining => t(
                "Each installment repays an equal part of the capital. Interest is charged each period on the capital still owed when the period begins, so it goes down as the capital is repaid.",
                "Cada cuota paga una parte igual del capital. El interés de cada período se calcula sobre el capital adeudado al empezar el período, así que baja a medida que se paga el capital.",
            ),
            Method::Flat => t(
                "Each installment repays an equal part of the capital. Interest is charged each period on the original amount lent, so every installment has the same interest.",
                "Cada cuota paga una parte igual del capital. El interés de cada período se calcula sobre el monto original prestado, así que todas las cuotas tienen el mismo interés.",
            ),
            Method::InterestOnly => t(
                "No fixed term. Each period's interest is charged on the capital still owed; anything paid above the interest lowers the capital and the next interest.",
                "Sin plazo fijo. El interés de cada período se calcula sobre el capital adeudado; lo que pase del interés baja el capital y el próximo interés.",
            ),
        }
    }
    pub fn has_installments(self) -> bool {
        self != Method::InterestOnly
    }
}

/// A rate quoted per interest period `per`, charged on a repayment period
/// `every`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeriodRate {
    pub rate: Rate,
    pub per: Frequency,
    pub every: Frequency,
}

impl PeriodRate {
    pub fn new(rate: Rate, per: Frequency, every: Frequency) -> Self {
        Self { rate, per, every }
    }

    /// One repayment period's interest on `balance`, rounded half-up.
    pub fn interest(&self, balance: Cents) -> Cents {
        if balance <= 0 || self.rate.0 <= 0 {
            return 0;
        }
        let num = balance as i128 * self.rate.0 as i128 * self.every.days() as i128;
        let den = RATE_SCALE as i128 * self.per.days() as i128;
        div_round_half_up(num, den) as Cents
    }

    /// The rate per repayment period (rounded to 0.0001%).
    pub fn effective(&self) -> Rate {
        Rate(div_round_half_up(self.rate.0 as i128 * self.every.days() as i128, self.per.days() as i128) as i64)
    }

    pub fn converts(&self) -> bool {
        self.per != self.every
    }

    /// "10% a month".
    pub fn label(&self) -> String {
        format!("{} {}", self.rate.percent_label(), self.per.per())
    }

    /// How the quoted rate becomes the rate per payment, when they differ:
    /// "10% a month = 2.3333% a week (7 of 30 days)".
    pub fn conversion_note(&self) -> Option<String> {
        self.converts().then(|| {
            let (quoted, eff, per) = (self.label(), self.effective().percent_label(), self.every.per());
            let (a, b) = (self.every.days(), self.per.days());
            tr!("{quoted} = {eff} {per} ({a} of {b} days)", "{quoted} = {eff} {per} ({a} de {b} días)")
        })
    }
}

/// Common rates offered as one-click choices.
pub const RATE_PRESETS: [Rate; 3] = [Rate(50_000), Rate(100_000), Rate(150_000)];

/// Highest rate accepted per period: 100%.
pub const MAX_PERIOD_RATE: Rate = Rate(RATE_SCALE);

/// Most installments a loan may have.
pub const MAX_INSTALLMENTS: u32 = 1000;

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
    /// Percentage charged per interest period.
    pub rate: Rate,
    pub interest_period: Frequency,
    /// How often payments fall due.
    pub frequency: Frequency,
    pub start_date: NaiveDate,
    pub method: Method,
    /// Number of installments; `None` for interest-only loans.
    pub installments: Option<u32>,
    /// Charged once on what is unpaid when an installment becomes overdue.
    pub late_fee_rate: Rate,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TermsError {
    #[error("principal")]
    Principal,
    #[error("rate")]
    Rate,
    #[error("installments")]
    Installments,
    #[error("late fee")]
    LateFee,
}

impl TermsError {
    pub fn message(&self) -> &'static str {
        match self {
            TermsError::Principal => t("Enter an amount greater than zero", "Escribe un monto mayor que cero"),
            TermsError::Rate => t("The rate must be between 0% and 100% per period", "La tasa debe estar entre 0% y 100% por período"),
            TermsError::Installments => t("Enter between 1 and 1,000 installments", "Escribe entre 1 y 1,000 cuotas"),
            TermsError::LateFee => t("The late fee must be between 0% and 100%", "El cargo por mora debe estar entre 0% y 100%"),
        }
    }
}

impl LoanTerms {
    /// Interest-only terms, as loans made before installments existed.
    pub fn interest_only(principal: Cents, rate: Rate, frequency: Frequency, start_date: NaiveDate) -> Self {
        Self {
            principal,
            rate,
            interest_period: frequency,
            frequency,
            start_date,
            method: Method::InterestOnly,
            installments: None,
            late_fee_rate: Rate(0),
        }
    }

    pub fn validate(&self) -> Result<(), TermsError> {
        if self.principal <= 0 {
            return Err(TermsError::Principal);
        }
        if self.rate.0 < 0 || self.rate > MAX_PERIOD_RATE {
            return Err(TermsError::Rate);
        }
        if self.late_fee_rate.0 < 0 || self.late_fee_rate > MAX_PERIOD_RATE {
            return Err(TermsError::LateFee);
        }
        match (self.method.has_installments(), self.installments) {
            (true, Some(n)) if (1..=MAX_INSTALLMENTS).contains(&n) => {}
            (false, None) => {}
            _ => return Err(TermsError::Installments),
        }
        Ok(())
    }

    pub fn period_rate(&self) -> PeriodRate {
        PeriodRate::new(self.rate, self.interest_period, self.frequency)
    }

    /// Interest of the first period, on the full principal.
    pub fn interest_per_period(&self) -> Cents {
        self.period_rate().interest(self.principal)
    }

    pub fn first_due(&self) -> NaiveDate {
        self.frequency.due_date(self.start_date, 1)
    }

    /// "10% a month · paid weekly".
    pub fn rate_line(&self) -> String {
        let rate = self.period_rate().label();
        if self.interest_period == self.frequency {
            rate
        } else {
            let paid = self.frequency.label().to_lowercase();
            tr!("{rate} · paid {paid}", "{rate} · pago {paid}")
        }
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
    fn rates_convert_on_a_thirty_day_month() {
        let monthly = |every| PeriodRate::new(Rate(100_000), Frequency::Monthly, every);
        // 10,000.00 at 10% a month.
        assert_eq!(monthly(Frequency::Monthly).interest(1_000_000), 100_000);
        assert_eq!(monthly(Frequency::Biweekly).interest(1_000_000), 50_000);
        // 7/30 of 1,000.00 = 233.333... -> 233.33
        assert_eq!(monthly(Frequency::Weekly).interest(1_000_000), 23_333);
        assert_eq!(monthly(Frequency::Daily).interest(1_000_000), 3_333);
        assert_eq!(monthly(Frequency::Weekly).effective(), Rate(23_333));
        // 1% a day paid monthly is 30% a month.
        assert_eq!(PeriodRate::new(Rate(10_000), Frequency::Daily, Frequency::Monthly).interest(100_000), 30_000);
        assert!(monthly(Frequency::Weekly).conversion_note().is_some());
        assert!(monthly(Frequency::Monthly).conversion_note().is_none());
    }

    #[test]
    fn due_dates() {
        let s = d(2026, 1, 31);
        assert_eq!(Frequency::Monthly.due_date(s, 1), d(2026, 2, 28));
        assert_eq!(Frequency::Monthly.due_date(s, 2), d(2026, 3, 31));
        // Leap year.
        assert_eq!(Frequency::Monthly.due_date(d(2028, 1, 31), 1), d(2028, 2, 29));
        let s = d(2026, 1, 5);
        assert_eq!(Frequency::Biweekly.due_date(s, 1), d(2026, 1, 20));
        assert_eq!(Frequency::Biweekly.due_date(s, 2), d(2026, 2, 5));
        assert_eq!(Frequency::Biweekly.due_date(s, 3), d(2026, 2, 20));
        assert_eq!(Frequency::Biweekly.period_start(s, 1), s);
        assert_eq!(Frequency::Biweekly.period_start(s, 3), d(2026, 2, 5));
        assert_eq!(Frequency::Weekly.due_date(d(2026, 12, 29), 1), d(2027, 1, 5));
        assert_eq!(Frequency::Daily.due_date(d(2028, 2, 28), 1), d(2028, 2, 29));
    }

    #[test]
    fn validates() {
        let mut t = LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1));
        assert!(t.validate().is_ok());
        assert_eq!(t.interest_per_period(), 10_000);
        t.rate = Rate(1_000_001);
        assert_eq!(t.validate(), Err(TermsError::Rate));
        t.rate = Rate(0);
        t.principal = 0;
        assert_eq!(t.validate(), Err(TermsError::Principal));
        t.principal = 100_000;
        t.method = Method::Declining;
        assert_eq!(t.validate(), Err(TermsError::Installments));
        t.installments = Some(5);
        assert!(t.validate().is_ok());
        t.installments = Some(0);
        assert_eq!(t.validate(), Err(TermsError::Installments));
    }
}
