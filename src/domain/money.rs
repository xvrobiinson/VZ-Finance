//! Money and rate primitives.
//!
//! Money is always an `i64` count of minor units (cents). Rates are stored as
//! parts-per-million of 1.0 (`125_000` = 12.5% per year). No floating point is
//! used for money arithmetic; see `docs/ACCOUNTING.md`.

use std::fmt;

/// Minor currency units (cents for USD).
pub type Cents = i64;

/// Annual interest rate in parts per million (1% = 10_000).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rate(pub i64);

pub const RATE_SCALE: i64 = 1_000_000;
/// Highest rate accepted on input: 1000% per year.
pub const MAX_RATE: Rate = Rate(10 * RATE_SCALE);
/// Largest single amount accepted on input: 1 trillion minor units.
pub const MAX_AMOUNT: Cents = 1_000_000_000_000;

impl Rate {
    pub fn from_percent_str(input: &str) -> Result<Rate, ParseError> {
        let s = input.trim().trim_end_matches('%').trim();
        if s.is_empty() {
            return Err(ParseError::Empty);
        }
        // Percent with up to 4 decimals -> ppm (percent * 10_000).
        let value = parse_fixed(s, 4)?;
        let rate = Rate(value);
        if rate > MAX_RATE {
            return Err(ParseError::TooLarge);
        }
        Ok(rate)
    }

    /// The rate as a percent string, trimmed of trailing zeros ("12.5%").
    pub fn percent_label(self) -> String {
        let whole = self.0 / 10_000;
        let frac = self.0 % 10_000;
        if frac == 0 {
            format!("{whole}%")
        } else {
            let frac = format!("{frac:04}");
            format!("{whole}.{}%", frac.trim_end_matches('0'))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Invalid,
    Negative,
    TooManyDecimals(u32),
    TooLarge,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "Enter an amount"),
            ParseError::Invalid => write!(f, "Use digits only, like 1250.00"),
            ParseError::Negative => write!(f, "Amount must be positive"),
            ParseError::TooManyDecimals(n) => write!(f, "Use at most {n} decimal places"),
            ParseError::TooLarge => write!(f, "That value is too large"),
        }
    }
}

/// Parses a non-negative decimal with at most `decimals` fractional digits into
/// an integer scaled by 10^decimals. Accepts thousands separators (`,` or a
/// space) and a leading currency symbol.
fn parse_fixed(input: &str, decimals: u32) -> Result<i64, ParseError> {
    let mut s: String = input
        .trim()
        .chars()
        .filter(|c| !matches!(c, ',' | ' ' | '_' | '\u{a0}'))
        .collect();
    if s.starts_with('-') {
        return Err(ParseError::Negative);
    }
    s = s.trim_start_matches(|c: char| !c.is_ascii_digit() && c != '.').to_string();
    if s.is_empty() {
        return Err(ParseError::Empty);
    }
    let (int_part, frac_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s.as_str(), ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return Err(ParseError::Invalid);
    }
    if !int_part.chars().all(|c| c.is_ascii_digit()) || !frac_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(ParseError::Invalid);
    }
    if frac_part.len() as u32 > decimals {
        return Err(ParseError::TooManyDecimals(decimals));
    }
    if int_part.len() > 15 {
        return Err(ParseError::TooLarge);
    }
    let int_value: i64 = if int_part.is_empty() { 0 } else { int_part.parse().map_err(|_| ParseError::Invalid)? };
    let mut frac = frac_part.to_string();
    while (frac.len() as u32) < decimals {
        frac.push('0');
    }
    let frac_value: i64 = if frac.is_empty() { 0 } else { frac.parse().map_err(|_| ParseError::Invalid)? };
    int_value
        .checked_mul(10_i64.pow(decimals))
        .and_then(|v| v.checked_add(frac_value))
        .ok_or(ParseError::TooLarge)
}

/// Parses a user-entered money amount ("1,250.50", "$300") into cents.
pub fn parse_money(input: &str) -> Result<Cents, ParseError> {
    let value = parse_fixed(input, 2)?;
    if value > MAX_AMOUNT {
        return Err(ParseError::TooLarge);
    }
    Ok(value)
}

/// Rounds `numerator / denominator` half away from zero. `denominator` > 0.
pub fn div_round_half_up(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0);
    if numerator >= 0 {
        (numerator + denominator / 2) / denominator
    } else {
        -((-numerator + denominator / 2) / denominator)
    }
}

/// Simple interest on `principal` for `days` at `rate`, Actual/365 Fixed,
/// rounded half-up to the cent. This is the single accrual primitive used by
/// schedules and by the payment engine.
pub fn interest_for_days(principal: Cents, rate: Rate, days: i64) -> Cents {
    if principal <= 0 || days <= 0 || rate.0 <= 0 {
        return 0;
    }
    let num = principal as i128 * rate.0 as i128 * days as i128;
    let den = RATE_SCALE as i128 * 365;
    div_round_half_up(num, den) as Cents
}

/// Number formatting style for figures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NumberStyle {
    /// 1,234.56
    #[default]
    CommaDot,
    /// 1.234,56
    DotComma,
    /// 1 234,56
    SpaceComma,
}

impl NumberStyle {
    pub fn key(self) -> &'static str {
        match self {
            NumberStyle::CommaDot => "comma_dot",
            NumberStyle::DotComma => "dot_comma",
            NumberStyle::SpaceComma => "space_comma",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "dot_comma" => NumberStyle::DotComma,
            "space_comma" => NumberStyle::SpaceComma,
            _ => NumberStyle::CommaDot,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            NumberStyle::CommaDot => "1,234.56",
            NumberStyle::DotComma => "1.234,56",
            NumberStyle::SpaceComma => "1 234,56",
        }
    }
    fn separators(self) -> (char, char) {
        match self {
            NumberStyle::CommaDot => (',', '.'),
            NumberStyle::DotComma => ('.', ','),
            NumberStyle::SpaceComma => ('\u{202f}', ','),
        }
    }
}

/// How amounts are printed: currency symbol and number style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoneyFormat {
    pub symbol: String,
    pub style: NumberStyle,
}

impl Default for MoneyFormat {
    fn default() -> Self {
        Self { symbol: "$".into(), style: NumberStyle::CommaDot }
    }
}

impl MoneyFormat {
    fn group(&self, mut n: u64) -> String {
        let (sep, _) = self.style.separators();
        let mut parts = Vec::new();
        loop {
            if n < 1000 {
                parts.push(format!("{n}"));
                break;
            }
            parts.push(format!("{:03}", n % 1000));
            n /= 1000;
        }
        parts.reverse();
        parts.join(&sep.to_string())
    }

    /// "$1,234.56"; negatives as "−$1,234.56".
    pub fn format(&self, cents: Cents) -> String {
        let (_, dec) = self.style.separators();
        let neg = cents < 0;
        let abs = cents.unsigned_abs();
        let s = format!("{}{}{}{dec}{:02}", if neg { "\u{2212}" } else { "" }, self.symbol, self.group(abs / 100), abs % 100);
        s
    }

    /// Always signed: "+$12.00" / "−$12.00" / "$0.00".
    pub fn format_signed(&self, cents: Cents) -> String {
        if cents > 0 { format!("+{}", self.format(cents)) } else { self.format(cents) }
    }

    /// Whole units without cents, for chart axes: "$12,500".
    pub fn format_whole(&self, cents: Cents) -> String {
        let neg = cents < 0;
        let units = (cents.unsigned_abs() + 50) / 100;
        format!("{}{}{}", if neg { "\u{2212}" } else { "" }, self.symbol, self.group(units))
    }

    /// Compact axis label: "$1.2k", "$3.4M".
    pub fn format_compact(&self, cents: Cents) -> String {
        let units = cents as f64 / 100.0;
        let abs = units.abs();
        let sign = if units < 0.0 { "\u{2212}" } else { "" };
        let (_, dec) = self.style.separators();
        let body = if abs >= 1_000_000.0 {
            trim_compact(abs / 1_000_000.0, "M")
        } else if abs >= 1_000.0 {
            trim_compact(abs / 1_000.0, "k")
        } else {
            format!("{abs:.0}")
        };
        format!("{sign}{}{}", self.symbol, body.replace('.', &dec.to_string()))
    }

    /// Plain amount for an editable input ("1250.00"), no symbol or grouping.
    pub fn input_value(cents: Cents) -> String {
        format!("{}.{:02}", cents / 100, (cents % 100).abs())
    }
}

fn trim_compact(v: f64, suffix: &str) -> String {
    let s = format!("{v:.1}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}{suffix}")
}

/// A ratio as a percent with one decimal ("12.3%"); `None` when undefined.
pub fn percent_label(numerator: Cents, denominator: Cents) -> Option<String> {
    if denominator == 0 {
        return None;
    }
    let tenths = div_round_half_up(numerator as i128 * 1000, denominator as i128);
    let neg = tenths < 0;
    let abs = tenths.abs();
    Some(format!("{}{}.{}%", if neg { "\u{2212}" } else { "" }, abs / 10, abs % 10))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_money() {
        assert_eq!(parse_money("1,250.5"), Ok(125_050));
        assert_eq!(parse_money("$300"), Ok(30_000));
        assert_eq!(parse_money(" 0.07 "), Ok(7));
        assert_eq!(parse_money(".5"), Ok(50));
        assert_eq!(parse_money("12.345"), Err(ParseError::TooManyDecimals(2)));
        assert_eq!(parse_money("-5"), Err(ParseError::Negative));
        assert_eq!(parse_money("abc"), Err(ParseError::Empty));
        assert_eq!(parse_money("1.2.3"), Err(ParseError::Invalid));
        assert_eq!(parse_money(""), Err(ParseError::Empty));
    }

    #[test]
    fn parses_rates() {
        assert_eq!(Rate::from_percent_str("12.5"), Ok(Rate(125_000)));
        assert_eq!(Rate::from_percent_str("12.5%"), Ok(Rate(125_000)));
        assert_eq!(Rate::from_percent_str("0"), Ok(Rate(0)));
        assert_eq!(Rate::from_percent_str("7.1234"), Ok(Rate(71_234)));
        assert_eq!(Rate::from_percent_str("7.12345"), Err(ParseError::TooManyDecimals(4)));
        assert_eq!(Rate::from_percent_str("1001"), Err(ParseError::TooLarge));
        assert_eq!(Rate(125_000).percent_label(), "12.5%");
        assert_eq!(Rate(100_000).percent_label(), "10%");
        assert_eq!(Rate(71_234).percent_label(), "7.1234%");
    }

    #[test]
    fn interest_actual_365_half_up() {
        // 10,000.00 at 12% for 30 days = 98.630136... -> 98.63
        assert_eq!(interest_for_days(1_000_000, Rate(120_000), 30), 9_863);
        // 1,000.00 at 10% for 365 days = exactly 100.00
        assert_eq!(interest_for_days(100_000, Rate(100_000), 365), 10_000);
        // half-up: 0.5 cent rounds up. 73.00 at 5% for 1 day = 0.01 cents*... compute exact
        // 7300 * 50000 * 1 / (1e6*365) = 1.0 -> 1 cent
        assert_eq!(interest_for_days(7_300, Rate(50_000), 1), 1);
        // 3650 * 50000 / 365e6 = 0.5 -> rounds to 1
        assert_eq!(interest_for_days(3_650, Rate(50_000), 1), 1);
        assert_eq!(interest_for_days(3_649, Rate(50_000), 1), 0);
        assert_eq!(interest_for_days(100_000, Rate(0), 30), 0);
        assert_eq!(interest_for_days(100_000, Rate(100_000), 0), 0);
    }

    #[test]
    fn formats_money() {
        let f = MoneyFormat::default();
        assert_eq!(f.format(123_456_789), "$1,234,567.89");
        assert_eq!(f.format(5), "$0.05");
        assert_eq!(f.format(-1_000), "\u{2212}$10.00");
        assert_eq!(f.format_signed(1_000), "+$10.00");
        assert_eq!(f.format_signed(0), "$0.00");
        assert_eq!(f.format_compact(1_250_000), "$12.5k");
        assert_eq!(f.format_whole(1_250_049), "$12,500");
        let eu = MoneyFormat { symbol: "€".into(), style: NumberStyle::DotComma };
        assert_eq!(eu.format(123_456_789), "€1.234.567,89");
        assert_eq!(percent_label(1_234, 10_000), Some("12.3%".into()));
        assert_eq!(percent_label(1, 0), None);
    }
}
