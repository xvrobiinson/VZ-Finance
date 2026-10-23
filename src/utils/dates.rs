use chrono::{Datelike, Local, Months, NaiveDate};

use crate::i18n::{self, Lang};

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

/// User-selectable date display formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DateStyle {
    /// Oct 21, 2026 / 21 oct 2026
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

    pub fn format_in(self, d: NaiveDate, lang: Lang) -> String {
        match self {
            DateStyle::Medium => match lang {
                Lang::En => format!("{} {}, {}", short_month_in(d.month(), lang), d.day(), d.year()),
                Lang::Es => format!("{} {} {}", d.day(), short_month_in(d.month(), lang), d.year()),
            },
            DateStyle::Iso => d.format("%Y-%m-%d").to_string(),
            DateStyle::DayMonthYear => d.format("%d/%m/%Y").to_string(),
            DateStyle::MonthDayYear => d.format("%m/%d/%Y").to_string(),
        }
    }

    pub fn format(self, d: NaiveDate) -> String {
        self.format_in(d, i18n::get())
    }

    /// Short form without the year when it is the current year ("Oct 21").
    pub fn format_short(self, d: NaiveDate, today: NaiveDate) -> String {
        let lang = i18n::get();
        match self {
            DateStyle::Medium if d.year() == today.year() => match lang {
                Lang::En => format!("{} {}", short_month_in(d.month(), lang), d.day()),
                Lang::Es => format!("{} {}", d.day(), short_month_in(d.month(), lang)),
            },
            _ => self.format_in(d, lang),
        }
    }
}

fn short_month_in(m: u32, lang: Lang) -> String {
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const ES: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
    let i = (m.clamp(1, 12) - 1) as usize;
    match lang {
        Lang::En => EN[i].to_string(),
        Lang::Es => ES[i].to_string(),
    }
}

/// "October 2026" / "Octubre 2026".
pub fn month_year(d: NaiveDate) -> String {
    let name = i18n::month_name(d.month());
    let mut c = name.chars();
    let name = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
    format!("{name} {}", d.year())
}

/// "Oct 2026" / "oct 2026".
pub fn short_month_year(d: NaiveDate) -> String {
    format!("{} {}", short_month_in(d.month(), i18n::get()), d.year())
}

pub fn short_month(d: NaiveDate) -> String {
    short_month_in(d.month(), i18n::get())
}

/// The numeric pattern used for typed dates: dd/mm/yyyy in Spanish,
/// mm/dd/yyyy in English.
pub fn input_format(lang: Lang) -> &'static str {
    match lang {
        Lang::Es => "%d/%m/%Y",
        Lang::En => "%m/%d/%Y",
    }
}

pub fn input_placeholder(lang: Lang) -> &'static str {
    match lang {
        Lang::Es => "dd/mm/aaaa",
        Lang::En => "mm/dd/yyyy",
    }
}

/// Parses a typed date in the language's numeric order. Also accepts ISO
/// (2026-10-21), dashes or dots as separators, and two-digit years.
pub fn parse_input(s: &str, lang: Lang) -> Option<NaiveDate> {
    let s = s.trim();
    // ISO only when it starts with a four-digit year ("2026-10-21").
    if s.split('-').next().is_some_and(|y| y.len() == 4)
        && let Some(d) = parse_iso(s)
    {
        return Some(d);
    }
    let parts: Vec<&str> = s.split(['/', '-', '.', ' ']).filter(|p| !p.is_empty()).collect();
    if parts.len() != 3 {
        return None;
    }
    let nums: Vec<u32> = parts.iter().map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let (day, month) = match lang {
        Lang::Es => (nums[0], nums[1]),
        Lang::En => (nums[1], nums[0]),
    };
    let year = if nums[2] < 100 { 2000 + nums[2] as i32 } else { nums[2] as i32 };
    NaiveDate::from_ymd_opt(year, month, day)
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
    fn formats_in_both_languages() {
        assert_eq!(DateStyle::Medium.format_in(d(2026, 10, 1), Lang::En), "Oct 1, 2026");
        assert_eq!(DateStyle::Medium.format_in(d(2026, 10, 1), Lang::Es), "1 oct 2026");
        assert_eq!(DateStyle::Iso.format_in(d(2026, 10, 1), Lang::Es), "2026-10-01");
        assert_eq!(DateStyle::DayMonthYear.format_in(d(2026, 10, 1), Lang::En), "01/10/2026");
    }

    #[test]
    fn parses_typed_dates() {
        assert_eq!(parse_input("21/10/2026", Lang::Es), Some(d(2026, 10, 21)));
        assert_eq!(parse_input("10/21/2026", Lang::En), Some(d(2026, 10, 21)));
        assert_eq!(parse_input("2026-10-21", Lang::Es), Some(d(2026, 10, 21)));
        assert_eq!(parse_input("5-1-26", Lang::Es), Some(d(2026, 1, 5)));
        assert_eq!(parse_input("31/02/2026", Lang::Es), None);
        assert_eq!(parse_input("hoy", Lang::Es), None);
    }
}
