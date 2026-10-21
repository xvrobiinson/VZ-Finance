use std::collections::HashMap;

use crate::domain::loans::InterestMethod;
use crate::domain::money::{MoneyFormat, NumberStyle};
use crate::utils::dates::DateStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    pub name: &'static str,
}

/// Two-decimal currencies offered as the base currency. Changing it changes
/// display only; amounts are never converted.
pub const CURRENCIES: &[Currency] = &[
    Currency { code: "USD", symbol: "$", name: "US dollar" },
    Currency { code: "EUR", symbol: "€", name: "Euro" },
    Currency { code: "GBP", symbol: "£", name: "Pound sterling" },
    Currency { code: "CAD", symbol: "CA$", name: "Canadian dollar" },
    Currency { code: "AUD", symbol: "A$", name: "Australian dollar" },
    Currency { code: "MXN", symbol: "MX$", name: "Mexican peso" },
    Currency { code: "DOP", symbol: "RD$", name: "Dominican peso" },
    Currency { code: "COP", symbol: "COL$", name: "Colombian peso" },
    Currency { code: "PHP", symbol: "₱", name: "Philippine peso" },
    Currency { code: "INR", symbol: "₹", name: "Indian rupee" },
    Currency { code: "CHF", symbol: "CHF ", name: "Swiss franc" },
];

pub fn currency(code: &str) -> Currency {
    CURRENCIES.iter().copied().find(|c| c.code == code).unwrap_or(CURRENCIES[0])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemePref {
    #[default]
    Light,
    Dark,
}

impl ThemePref {
    pub fn key(self) -> &'static str {
        match self {
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChartRange {
    OneMonth,
    ThreeMonths,
    #[default]
    SixMonths,
    OneYear,
    All,
}

impl ChartRange {
    pub const ALL: [ChartRange; 5] = [ChartRange::OneMonth, ChartRange::ThreeMonths, ChartRange::SixMonths, ChartRange::OneYear, ChartRange::All];
    pub fn key(self) -> &'static str {
        match self {
            ChartRange::OneMonth => "1m",
            ChartRange::ThreeMonths => "3m",
            ChartRange::SixMonths => "6m",
            ChartRange::OneYear => "1y",
            ChartRange::All => "all",
        }
    }
    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|r| r.key() == k).unwrap_or_default()
    }
    pub fn label(self) -> &'static str {
        match self {
            ChartRange::OneMonth => "1M",
            ChartRange::ThreeMonths => "3M",
            ChartRange::SixMonths => "6M",
            ChartRange::OneYear => "1Y",
            ChartRange::All => "All",
        }
    }
    pub fn months(self) -> Option<u32> {
        match self {
            ChartRange::OneMonth => Some(1),
            ChartRange::ThreeMonths => Some(3),
            ChartRange::SixMonths => Some(6),
            ChartRange::OneYear => Some(12),
            ChartRange::All => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub currency_code: String,
    pub number_style: NumberStyle,
    pub date_style: DateStyle,
    pub default_method: InterestMethod,
    pub theme: ThemePref,
    pub chart_range: ChartRange,
    pub show_interest_line: bool,
    pub sidebar_collapsed: bool,
    /// Written by the backup command, never by `save_settings`.
    pub last_backup_at: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            currency_code: "USD".into(),
            number_style: NumberStyle::CommaDot,
            date_style: DateStyle::Medium,
            default_method: InterestMethod::Amortized,
            theme: ThemePref::Light,
            chart_range: ChartRange::SixMonths,
            show_interest_line: true,
            sidebar_collapsed: false,
            last_backup_at: None,
        }
    }
}

impl Settings {
    pub fn from_map(m: &HashMap<String, String>) -> Self {
        let d = Settings::default();
        let get = |k: &str| m.get(k).map(String::as_str);
        Settings {
            currency_code: get("currency").map(|c| currency(c).code.to_string()).unwrap_or(d.currency_code),
            number_style: get("number_style").map(NumberStyle::from_key).unwrap_or(d.number_style),
            date_style: get("date_style").map(DateStyle::from_key).unwrap_or(d.date_style),
            default_method: get("default_method").and_then(InterestMethod::from_key).unwrap_or(d.default_method),
            theme: match get("theme") {
                Some("dark") => ThemePref::Dark,
                _ => ThemePref::Light,
            },
            chart_range: get("chart_range").map(ChartRange::from_key).unwrap_or(d.chart_range),
            show_interest_line: get("show_interest_line").map(|v| v == "1").unwrap_or(d.show_interest_line),
            sidebar_collapsed: get("sidebar_collapsed").map(|v| v == "1").unwrap_or(d.sidebar_collapsed),
            last_backup_at: get("last_backup_at").map(str::to_string),
        }
    }

    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("currency", self.currency_code.clone()),
            ("number_style", self.number_style.key().into()),
            ("date_style", self.date_style.key().into()),
            ("default_method", self.default_method.key().into()),
            ("theme", self.theme.key().into()),
            ("chart_range", self.chart_range.key().into()),
            ("show_interest_line", if self.show_interest_line { "1" } else { "0" }.into()),
            ("sidebar_collapsed", if self.sidebar_collapsed { "1" } else { "0" }.into()),
        ]
    }

    pub fn money(&self) -> MoneyFormat {
        MoneyFormat { symbol: currency(&self.currency_code).symbol.to_string(), style: self.number_style }
    }
}
