use std::collections::HashMap;

use crate::domain::loans::Frequency;
use crate::domain::money::{MoneyFormat, NumberStyle, Rate};
use crate::i18n::{Lang, t};
use crate::utils::dates::DateStyle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Currency {
    pub code: &'static str,
    pub symbol: &'static str,
    pub name_en: &'static str,
    pub name_es: &'static str,
}

impl Currency {
    pub fn name(&self) -> &'static str {
        t(self.name_en, self.name_es)
    }
}

/// Two-decimal currencies offered as the base currency. Changing it changes
/// display only; amounts are never converted.
pub const CURRENCIES: &[Currency] = &[
    Currency { code: "DOP", symbol: "RD$", name_en: "Dominican peso", name_es: "Peso dominicano" },
    Currency { code: "USD", symbol: "US$", name_en: "US dollar", name_es: "Dólar estadounidense" },
    Currency { code: "EUR", symbol: "€", name_en: "Euro", name_es: "Euro" },
    Currency { code: "MXN", symbol: "MX$", name_en: "Mexican peso", name_es: "Peso mexicano" },
    Currency { code: "COP", symbol: "COL$", name_en: "Colombian peso", name_es: "Peso colombiano" },
    Currency { code: "GBP", symbol: "£", name_en: "Pound sterling", name_es: "Libra esterlina" },
    Currency { code: "CAD", symbol: "CA$", name_en: "Canadian dollar", name_es: "Dólar canadiense" },
];

pub fn currency(code: &str) -> Currency {
    CURRENCIES.iter().copied().find(|c| c.code == code).unwrap_or(CURRENCIES[0])
}

pub const DEFAULT_BUSINESS_NAME: &str = "VZ Finance";

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
    pub const ALL: [ChartRange; 5] =
        [ChartRange::OneMonth, ChartRange::ThreeMonths, ChartRange::SixMonths, ChartRange::OneYear, ChartRange::All];
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
            ChartRange::OneMonth => t("1M", "1M"),
            ChartRange::ThreeMonths => t("3M", "3M"),
            ChartRange::SixMonths => t("6M", "6M"),
            ChartRange::OneYear => t("1Y", "1A"),
            ChartRange::All => t("All", "Todo"),
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
    pub language: Lang,
    /// Shown in the sidebar, the window title and on receipts.
    pub business_name: String,
    pub currency_code: String,
    pub number_style: NumberStyle,
    pub date_style: DateStyle,
    pub default_rate: Rate,
    pub default_frequency: Frequency,
    /// Open a printable receipt right after recording a payment.
    pub receipt_after_payment: bool,
    /// Open the printable loan receipt right after creating a loan.
    pub receipt_after_loan: bool,
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
            language: Lang::En,
            business_name: DEFAULT_BUSINESS_NAME.into(),
            currency_code: "DOP".into(),
            number_style: NumberStyle::CommaDot,
            date_style: DateStyle::Medium,
            default_rate: Rate(100_000),
            default_frequency: Frequency::Monthly,
            receipt_after_payment: true,
            receipt_after_loan: true,
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
        let flag = |k: &str, default: bool| get(k).map(|v| v == "1").unwrap_or(default);
        Settings {
            language: get("language").map(Lang::from_key).unwrap_or(d.language),
            business_name: get("business_name").map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).unwrap_or(d.business_name),
            currency_code: get("currency").map(|c| currency(c).code.to_string()).unwrap_or(d.currency_code),
            number_style: get("number_style").map(NumberStyle::from_key).unwrap_or(d.number_style),
            date_style: get("date_style").map(DateStyle::from_key).unwrap_or(d.date_style),
            default_rate: get("default_rate")
                .and_then(|v| v.parse().ok())
                .map(Rate)
                .filter(|r| r.0 >= 0 && r.0 <= 1_000_000)
                .unwrap_or(d.default_rate),
            default_frequency: get("default_frequency").and_then(Frequency::from_key).unwrap_or(d.default_frequency),
            receipt_after_payment: flag("receipt_after_payment", d.receipt_after_payment),
            receipt_after_loan: flag("receipt_after_loan", d.receipt_after_loan),
            theme: match get("theme") {
                Some("dark") => ThemePref::Dark,
                _ => ThemePref::Light,
            },
            chart_range: get("chart_range").map(ChartRange::from_key).unwrap_or(d.chart_range),
            show_interest_line: flag("show_interest_line", d.show_interest_line),
            sidebar_collapsed: flag("sidebar_collapsed", d.sidebar_collapsed),
            last_backup_at: get("last_backup_at").map(str::to_string),
        }
    }

    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let b = |v: bool| if v { "1" } else { "0" }.to_string();
        vec![
            ("language", self.language.key().into()),
            ("business_name", self.business_name.trim().to_string()),
            ("currency", self.currency_code.clone()),
            ("number_style", self.number_style.key().into()),
            ("date_style", self.date_style.key().into()),
            ("default_rate", self.default_rate.0.to_string()),
            ("default_frequency", self.default_frequency.key().into()),
            ("receipt_after_payment", b(self.receipt_after_payment)),
            ("receipt_after_loan", b(self.receipt_after_loan)),
            ("theme", self.theme.key().into()),
            ("chart_range", self.chart_range.key().into()),
            ("show_interest_line", b(self.show_interest_line)),
            ("sidebar_collapsed", b(self.sidebar_collapsed)),
        ]
    }

    pub fn money(&self) -> MoneyFormat {
        MoneyFormat { symbol: currency(&self.currency_code).symbol.to_string(), style: self.number_style }
    }
}
