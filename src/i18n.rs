//! English / Spanish UI text.
//!
//! The current language is a process-wide setting, applied from the user's
//! settings on every reload. Call sites keep both strings side by side:
//! `t("Loans", "Préstamos")`, or `tr!("Loan #{id}", "Préstamo #{id}")` for
//! formatted text, so a screen reads naturally in either language.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    En,
    Es,
}

impl Lang {
    pub fn key(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
        }
    }
    pub fn from_key(k: &str) -> Self {
        if k == "es" { Lang::Es } else { Lang::En }
    }
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Español",
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    LANG.store(if lang == Lang::Es { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn get() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::Es } else { Lang::En }
}

pub fn is_es() -> bool {
    get() == Lang::Es
}

/// Pick the string for the current language.
pub fn t(en: &'static str, es: &'static str) -> &'static str {
    if is_es() { es } else { en }
}

/// `format!` in the current language: `tr!("{n} loans", "{n} préstamos")`.
#[macro_export]
macro_rules! tr {
    ($en:literal, $es:literal $(,)?) => {
        if $crate::i18n::is_es() { format!($es) } else { format!($en) }
    };
}

/// "1 loan" / "2 loans" in the current language.
pub fn count(n: usize, en_one: &str, en_many: &str, es_one: &str, es_many: &str) -> String {
    let word = match (is_es(), n == 1) {
        (false, true) => en_one,
        (false, false) => en_many,
        (true, true) => es_one,
        (true, false) => es_many,
    };
    format!("{n} {word}")
}

const MONTHS_EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS_ES: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const WEEKDAYS_EN: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
const WEEKDAYS_ES: [&str; 7] = ["Do", "Lu", "Ma", "Mi", "Ju", "Vi", "Sá"];

/// Full month name, 1-based.
pub fn month_name(m: u32) -> &'static str {
    let i = (m.clamp(1, 12) - 1) as usize;
    if is_es() { MONTHS_ES[i] } else { MONTHS_EN[i] }
}

/// Three-letter month: "Oct" / "oct".
pub fn month_short(m: u32) -> String {
    month_name(m).chars().take(3).collect()
}

/// Weekday abbreviations starting on Sunday.
pub fn weekdays() -> [&'static str; 7] {
    if is_es() { WEEKDAYS_ES } else { WEEKDAYS_EN }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_language() {
        set(Lang::Es);
        assert_eq!(t("Loans", "Préstamos"), "Préstamos");
        let n = 3;
        assert_eq!(tr!("{n} loans", "{n} préstamos"), "3 préstamos");
        assert_eq!(month_short(10), "oct");
        set(Lang::En);
        assert_eq!(month_short(10), "Oct");
        assert_eq!(count(1, "loan", "loans", "préstamo", "préstamos"), "1 loan");
    }
}
