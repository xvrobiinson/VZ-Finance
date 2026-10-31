use std::collections::HashMap;

pub use super::receipt_text::ReceiptText;
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

/// Receipt name used before this setting existed (the old app name); read as
/// "not set".
const OLD_DEFAULT_NAME: &str = "VZ Finance";

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

/// Which client details are printed on receipts. Each is printed only
/// when it is ticked and the client has it. Photos are never printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptClientFields {
    pub name: bool,
    pub id_number: bool,
    pub phone: bool,
    pub address: bool,
}

impl Default for ReceiptClientFields {
    fn default() -> Self {
        Self { name: true, id_number: true, phone: true, address: true }
    }
}

/// Which optional parts of a payment receipt are printed. The business
/// name, the amount paid, the date and the receipt number always are. The
/// loan receipt is the client's agreement and keeps all of its terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptShow {
    pub loan_number: bool,
    pub amount_lent: bool,
    pub rate: bool,
    pub cycle: bool,
    pub installments: bool,
    pub split: bool,
    pub late_fees: bool,
    pub balance: bool,
    pub history: bool,
}

impl Default for ReceiptShow {
    fn default() -> Self {
        Self::ALL_ON
    }
}

impl ReceiptShow {
    const ALL_ON: Self = Self {
        loan_number: true,
        amount_lent: true,
        rate: true,
        cycle: true,
        installments: true,
        split: true,
        late_fees: true,
        balance: true,
        history: true,
    };

    /// (key on this PC, key in the cloud) for each part, in the order
    /// Settings lists them.
    pub const KEYS: [(&'static str, &'static str); 9] = [
        ("receipt_show_loan_number", "loanNumber"),
        ("receipt_show_amount_lent", "amountLent"),
        ("receipt_show_rate", "rate"),
        ("receipt_show_cycle", "cycle"),
        ("receipt_show_installments", "installments"),
        ("receipt_show_split", "split"),
        ("receipt_show_late_fees", "lateFees"),
        ("receipt_show_balance", "balance"),
        ("receipt_show_history", "history"),
    ];

    /// The parts in `KEYS` order.
    pub fn values(&self) -> [bool; 9] {
        [
            self.loan_number,
            self.amount_lent,
            self.rate,
            self.cycle,
            self.installments,
            self.split,
            self.late_fees,
            self.balance,
            self.history,
        ]
    }

    /// Built from values in `KEYS` order.
    pub fn from_values(v: [bool; 9]) -> Self {
        Self {
            loan_number: v[0],
            amount_lent: v[1],
            rate: v[2],
            cycle: v[3],
            installments: v[4],
            split: v[5],
            late_fees: v[6],
            balance: v[7],
            history: v[8],
        }
    }

    /// A copy with part `i` (in `KEYS` order) set to `on`.
    pub fn with(self, i: usize, on: bool) -> Self {
        let mut v = self.values();
        if let Some(x) = v.get_mut(i) {
            *x = on;
        }
        Self::from_values(v)
    }
}

/// Which company details are printed at the top of receipts. Each is
/// printed only when it is ticked and has a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiptCompanyFields {
    pub name: bool,
    pub address: bool,
    pub phone: bool,
}

impl Default for ReceiptCompanyFields {
    fn default() -> Self {
        Self { name: true, address: true, phone: true }
    }
}

/// Company details printed on a receipt (see `Settings::receipt_company_lines`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompanyLines {
    pub name: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub language: Lang,
    /// The company name printed on receipts. Empty: the app name. The app
    /// itself is always called Acervus Finance.
    pub receipt_name: String,
    /// Optional; printed on receipts when ticked.
    pub company_address: String,
    pub company_phone: String,
    /// Company details printed on every receipt.
    pub receipt_company: ReceiptCompanyFields,
    /// Receipt wording; built-in text unless changed.
    pub receipt_text: ReceiptText,
    pub currency_code: String,
    pub number_style: NumberStyle,
    pub date_style: DateStyle,
    pub default_rate: Rate,
    pub default_frequency: Frequency,
    /// Late fee for new installment loans, charged once on what is unpaid
    /// when an installment becomes overdue.
    pub late_fee_rate: Rate,
    /// Open a printable receipt right after recording a payment.
    pub receipt_after_payment: bool,
    /// Open the printable loan receipt right after creating a loan.
    pub receipt_after_loan: bool,
    /// Client details printed on every receipt.
    pub receipt_client: ReceiptClientFields,
    /// Optional parts of payment receipts.
    pub receipt_show: ReceiptShow,
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
            receipt_name: String::new(),
            company_address: String::new(),
            company_phone: String::new(),
            receipt_company: ReceiptCompanyFields::default(),
            receipt_text: ReceiptText::default(),
            currency_code: "DOP".into(),
            number_style: NumberStyle::CommaDot,
            date_style: DateStyle::Medium,
            default_rate: Rate(100_000),
            default_frequency: Frequency::Monthly,
            late_fee_rate: Rate(50_000),
            receipt_after_payment: true,
            receipt_after_loan: true,
            receipt_client: ReceiptClientFields::default(),
            receipt_show: ReceiptShow::default(),
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
            // Stored under its original key so older backups keep it.
            receipt_name: get("business_name").map(str::trim).filter(|s| *s != OLD_DEFAULT_NAME).unwrap_or_default().to_string(),
            company_address: get("company_address").unwrap_or_default().trim().to_string(),
            company_phone: get("company_phone").unwrap_or_default().trim().to_string(),
            receipt_company: ReceiptCompanyFields {
                name: flag("receipt_company_name", d.receipt_company.name),
                address: flag("receipt_company_address", d.receipt_company.address),
                phone: flag("receipt_company_phone", d.receipt_company.phone),
            },
            receipt_text: ReceiptText::from_map(|k| get(k).map(str::to_string)),
            currency_code: get("currency").map(|c| currency(c).code.to_string()).unwrap_or(d.currency_code),
            number_style: get("number_style").map(NumberStyle::from_key).unwrap_or(d.number_style),
            date_style: get("date_style").map(DateStyle::from_key).unwrap_or(d.date_style),
            default_rate: get("default_rate")
                .and_then(|v| v.parse().ok())
                .map(Rate)
                .filter(|r| r.0 >= 0 && r.0 <= 1_000_000)
                .unwrap_or(d.default_rate),
            default_frequency: get("default_frequency").and_then(Frequency::from_key).unwrap_or(d.default_frequency),
            late_fee_rate: get("late_fee_rate")
                .and_then(|v| v.parse().ok())
                .map(Rate)
                .filter(|r| r.0 >= 0 && r.0 <= 1_000_000)
                .unwrap_or(d.late_fee_rate),
            receipt_after_payment: flag("receipt_after_payment", d.receipt_after_payment),
            receipt_after_loan: flag("receipt_after_loan", d.receipt_after_loan),
            receipt_client: ReceiptClientFields {
                name: flag("receipt_client_name", d.receipt_client.name),
                id_number: flag("receipt_client_id_number", d.receipt_client.id_number),
                phone: flag("receipt_client_phone", d.receipt_client.phone),
                address: flag("receipt_client_address", d.receipt_client.address),
            },
            receipt_show: ReceiptShow::from_values(ReceiptShow::KEYS.map(|(key, _)| flag(key, true))),
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
        let mut pairs = vec![
            ("language", self.language.key().into()),
            ("business_name", self.receipt_name.trim().to_string()),
            ("company_address", self.company_address.trim().to_string()),
            ("company_phone", self.company_phone.trim().to_string()),
            ("receipt_company_name", b(self.receipt_company.name)),
            ("receipt_company_address", b(self.receipt_company.address)),
            ("receipt_company_phone", b(self.receipt_company.phone)),
            ("currency", self.currency_code.clone()),
            ("number_style", self.number_style.key().into()),
            ("date_style", self.date_style.key().into()),
            ("default_rate", self.default_rate.0.to_string()),
            ("default_frequency", self.default_frequency.key().into()),
            ("late_fee_rate", self.late_fee_rate.0.to_string()),
            ("receipt_after_payment", b(self.receipt_after_payment)),
            ("receipt_after_loan", b(self.receipt_after_loan)),
            ("receipt_client_name", b(self.receipt_client.name)),
            ("receipt_client_id_number", b(self.receipt_client.id_number)),
            ("receipt_client_phone", b(self.receipt_client.phone)),
            ("receipt_client_address", b(self.receipt_client.address)),
            ("theme", self.theme.key().into()),
            ("chart_range", self.chart_range.key().into()),
            ("show_interest_line", b(self.show_interest_line)),
            ("sidebar_collapsed", b(self.sidebar_collapsed)),
        ];
        pairs.extend(ReceiptShow::KEYS.iter().zip(self.receipt_show.values()).map(|((key, _), on)| (*key, b(on))));
        pairs.extend(self.receipt_text.to_pairs());
        pairs
    }

    /// The company details to print at the top of receipts: ticked and not
    /// empty, in order (name, address, phone). The name is always the
    /// receipt name, or the app name when that is empty.
    pub fn receipt_company_lines(&self) -> CompanyLines {
        let keep = |on: bool, v: &str| Some(v.trim()).filter(|v| on && !v.is_empty()).map(str::to_string);
        CompanyLines {
            name: self.receipt_company.name.then(|| self.receipt_title()),
            address: keep(self.receipt_company.address, &self.company_address),
            phone: keep(self.receipt_company.phone, &self.company_phone),
        }
    }

    /// The name printed on receipts.
    pub fn receipt_title(&self) -> String {
        let name = self.receipt_name.trim();
        if name.is_empty() { crate::APP_NAME.to_string() } else { name.to_string() }
    }

    pub fn money(&self) -> MoneyFormat {
        MoneyFormat { symbol: currency(&self.currency_code).symbol.to_string(), style: self.number_style }
    }
}
