use chrono::{Datelike, Local, Months, NaiveDate};

pub const ISO: &str = "%Y-%m-%d";

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

pub fn parse_iso(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), ISO).ok()
}

pub fn to_iso(d: NaiveDate) -> String {
    d.format(ISO).to_string()
}

/// Adds calendar months, clamping to the last day of shorter months
/// (Jan 31 + 1 month = Feb 28/29).
pub fn add_months(d: NaiveDate, months: u32) -> NaiveDate {
    d.checked_add_months(Months::new(months)).unwrap_or(d)
}

pub fn days_between(from: NaiveDate, to: NaiveDate) -> i64 {
    (to - from).num_days()
}

pub fn month_start(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).unwrap_or(d)
}

pub fn next_month_start(d: NaiveDate) -> NaiveDate {
    add_months(month_start(d), 1)
}

/// User-selectable date display formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DateStyle {
    /// Oct 21, 2026
    #[default]
    Medium,
    /// 2026-10-21
    Iso,
    /// 21/10/2026
    DayMonthYear,
    /// 10/21/2026
    MonthDayYear,
}

impl DateStyle {
    pub const ALL: [DateStyle; 4] = [DateStyle::Medium, DateStyle::Iso, DateStyle::DayMonthYear, DateStyle::MonthDayYear];

    pub fn key(self) -> &'static str {
        match self {
            DateStyle::Medium => "medium",
            DateStyle::Iso => "iso",
            DateStyle::DayMonthYear => "dmy",
            DateStyle::MonthDayYear => "mdy",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "iso" => DateStyle::Iso,
            "dmy" => DateStyle::DayMonthYear,
            "mdy" => DateStyle::MonthDayYear,
            _ => DateStyle::Medium,
        }
    }
    pub fn pattern(self) -> &'static str {
        match self {
            DateStyle::Medium => "%b %-d, %Y",
            DateStyle::Iso => "%Y-%m-%d",
            DateStyle::DayMonthYear => "%d/%m/%Y",
            DateStyle::MonthDayYear => "%m/%d/%Y",
        }
    }
    pub fn format(self, d: NaiveDate) -> String {
        d.format(self.pattern()).to_string()
    }
    /// Short form without the year when it is the current year ("Oct 21").
    pub fn format_short(self, d: NaiveDate, today: NaiveDate) -> String {
        match self {
            DateStyle::Medium if d.year() == today.year() => d.format("%b %-d").to_string(),
            _ => self.format(d),
        }
    }
}

/// "Today", "Yesterday", "in 3 days", "12 days ago".
pub fn relative_days(d: NaiveDate, today: NaiveDate) -> String {
    let n = days_between(today, d);
    match n {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        n if n > 1 => format!("in {n} days"),
        n => format!("{} days ago", -n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn month_math_clamps() {
        assert_eq!(add_months(d(2026, 1, 31), 1), d(2026, 2, 28));
        assert_eq!(add_months(d(2028, 1, 31), 1), d(2028, 2, 29));
        assert_eq!(add_months(d(2026, 1, 31), 3), d(2026, 4, 30));
        assert_eq!(add_months(d(2026, 11, 15), 2), d(2027, 1, 15));
    }

    #[test]
    fn formats() {
        assert_eq!(DateStyle::Medium.format(d(2026, 10, 1)), "Oct 1, 2026");
        assert_eq!(DateStyle::Iso.format(d(2026, 10, 1)), "2026-10-01");
        assert_eq!(DateStyle::Medium.format_short(d(2026, 10, 1), d(2026, 12, 1)), "Oct 1");
        assert_eq!(relative_days(d(2026, 10, 1), d(2026, 10, 13)), "12 days ago");
    }
}
