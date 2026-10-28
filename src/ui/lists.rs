//! Searching and sorting the Clients and Loans tables. Pure functions over
//! plain rows, so the screens stay thin and the rules are tested here.
//!
//! Search ignores case and accents ("jose" finds "José") and every word of
//! the query must match one of the row's fields ("maria 809" finds Maria
//! with an 809 phone). Sorting is stable, so rows that tie keep the
//! table's own order; empty values go last whichever the direction.

use std::cmp::Ordering;

use chrono::NaiveDate;

use crate::ui::kit::Sort;

/// Lowercase without accents, for matching and alphabetical order.
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Whether every word of `query` appears in one of `fields`. An empty query
/// matches everything.
pub fn matches(query: &str, fields: &[&str]) -> bool {
    let fields: Vec<String> = fields.iter().map(|f| fold(f)).collect();
    fold(query).split_whitespace().all(|word| fields.iter().any(|f| f.contains(word)))
}

/// An amount the ways someone might type it: as shown ("RD$10,000.50"),
/// and plain ("10000.50", "10000").
pub fn amount_fields(shown: String, cents: i64) -> [String; 3] {
    let whole = cents / 100;
    [shown, format!("{whole}.{:02}", (cents % 100).abs()), whole.to_string()]
}

/// A value to sort a column by.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    /// Compared alphabetically, ignoring case and accents; empty goes last.
    Text(String),
    Number(i64),
    /// None (no date) goes last.
    Date(Option<NaiveDate>),
}

impl Key {
    pub fn text(s: &str) -> Self {
        Key::Text(fold(s.trim()))
    }
    fn is_empty(&self) -> bool {
        match self {
            Key::Text(s) => s.is_empty(),
            Key::Number(_) => false,
            Key::Date(d) => d.is_none(),
        }
    }
    fn cmp(&self, other: &Key) -> Ordering {
        match (self, other) {
            (Key::Text(a), Key::Text(b)) => a.cmp(b),
            (Key::Number(a), Key::Number(b)) => a.cmp(b),
            (Key::Date(a), Key::Date(b)) => a.cmp(b),
            _ => Ordering::Equal,
        }
    }
}

/// Sorts `rows` by `sort` using `key` for each column. With no column the
/// rows keep their order.
pub fn sort_rows<T, C: Copy + PartialEq>(rows: &mut [T], sort: Sort<C>, key: impl Fn(&T, C) -> Key) {
    let Some(column) = sort.column else { return };
    rows.sort_by(|a, b| {
        let (ka, kb) = (key(a, column), key(b, column));
        match (ka.is_empty(), kb.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => sort.apply(ka.cmp(&kb)),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, m, day).unwrap()
    }

    #[test]
    fn search_ignores_case_and_accents_and_needs_every_word() {
        assert!(matches("jose", &["José Peña"]));
        assert!(matches("PEÑA", &["jose pena"]));
        assert!(matches("maria 809", &["María Gómez", "809-555-0142"]));
        assert!(!matches("maria 829", &["María Gómez", "809-555-0142"]));
        assert!(matches("  ", &["anything"]));
        assert!(!matches("x", &[]));
    }

    #[test]
    fn amounts_match_as_shown_or_plain() {
        let fields = amount_fields("RD$10,000.50".into(), 1_000_050);
        let f: Vec<&str> = fields.iter().map(String::as_str).collect();
        assert!(matches("10,000", &f));
        assert!(matches("10000.50", &f));
        assert!(matches("10000", &f));
        assert!(!matches("9,999", &f));
    }

    type Row = (&'static str, i64, Option<NaiveDate>);
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Col {
        Name,
        Amount,
        Due,
    }
    fn key(r: &Row, c: Col) -> Key {
        match c {
            Col::Name => Key::text(r.0),
            Col::Amount => Key::Number(r.1),
            Col::Due => Key::Date(r.2),
        }
    }
    fn names(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.0).collect()
    }

    #[test]
    fn sorts_text_numbers_and_dates_both_ways() {
        let rows: Vec<Row> =
            vec![("Óscar", 500, Some(d(3, 1))), ("ana", 12_000, None), ("Bruno", 2_000, Some(d(1, 15))), ("", 700, Some(d(2, 1)))];
        let by = |sort| {
            let mut r = rows.clone();
            sort_rows(&mut r, sort, key);
            names(&r)
        };
        let asc = |c| Sort::default().toggle(c);
        let desc = |c| Sort::default().toggle(c).toggle(c);
        // Alphabetical ignoring case and accents; the empty name stays last.
        assert_eq!(by(asc(Col::Name)), ["ana", "Bruno", "Óscar", ""]);
        assert_eq!(by(desc(Col::Name)), ["Óscar", "Bruno", "ana", ""]);
        // Numerically, not as text (12,000 is not before 2,000).
        assert_eq!(by(asc(Col::Amount)), ["Óscar", "", "Bruno", "ana"]);
        assert_eq!(by(desc(Col::Amount)), ["ana", "Bruno", "", "Óscar"]);
        // Chronologically; no date stays last.
        assert_eq!(by(asc(Col::Due)), ["Bruno", "", "Óscar", "ana"]);
        assert_eq!(by(desc(Col::Due)), ["Óscar", "", "Bruno", "ana"]);
        // No column: the table's own order.
        assert_eq!(by(Sort::default()), ["Óscar", "ana", "Bruno", ""]);
    }

    #[test]
    fn ties_keep_their_order_and_a_new_column_starts_ascending() {
        let mut rows: Vec<Row> = vec![("b", 1, None), ("a", 1, None), ("c", 0, None)];
        let s = Sort::default().toggle(Col::Name).toggle(Col::Name).toggle(Col::Amount);
        assert!(!s.descending);
        sort_rows(&mut rows, s, key);
        assert_eq!(names(&rows), ["c", "b", "a"]);
    }

    #[test]
    fn search_then_sort_keeps_only_matches_in_order() {
        let rows: Vec<Row> = vec![("Ana Ruiz", 300, None), ("Bruno Ruiz", 100, None), ("Carla Díaz", 200, None)];
        let mut found: Vec<Row> = rows.into_iter().filter(|r| matches("ruiz", &[r.0])).collect();
        sort_rows(&mut found, Sort::default().toggle(Col::Amount), key);
        assert_eq!(names(&found), ["Bruno Ruiz", "Ana Ruiz"]);
    }
}
