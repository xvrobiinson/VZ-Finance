//! The engine's figures for a book as of a day, in the shape the cloud
//! importer compares (`figures.json`: the portfolio summary and each loan's
//! position, keys in camelCase). Used by `examples/export_for_cloud.rs` and
//! to check that a linked copy shows the same books as the cloud.

use chrono::NaiveDate;
use serde_json::{Map, Value, json};

use crate::domain::portfolio::Book;

/// `"a": 1, "b": 2` for the named fields of `$s`, keys in camelCase.
macro_rules! fields {
    ($s:expr; $($f:ident),+) => {{
        let mut m = Map::new();
        $( m.insert(camel(stringify!($f)), json!($s.$f)); )+
        m
    }};
}

fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn figures(book: &Book, as_of: NaiveDate) -> Value {
    let sum = book.summary(as_of);
    let summary = fields!(sum;
        contributions, withdrawals, principal_disbursed, principal_repaid, interest_collected, fees_collected,
        capital_expenses, interest_expenses, capital_income, other_income, capital_adjustments,
        interest_adjustments, available_capital, available_interest, available_cash, outstanding_principal,
        interest_receivable, fees_receivable, defaulted_principal, defaulted_interest, portfolio_value,
        active_loans, overdue_loans, due_soon_loans, overdue_amount, expected_monthly_interest);
    let loans: Vec<Value> = book
        .loans
        .iter()
        .map(|loan| {
            let pos = loan.position(as_of);
            let late = loan.delinquency(as_of);
            let mut m = Map::new();
            m.insert("no".into(), json!(loan.id));
            m.extend(fields!(pos;
                principal_outstanding, interest_outstanding, interest_charged_total, interest_paid, principal_paid,
                fees_paid, fees_outstanding, paid_total));
            m.extend(fields!(late; overdue_amount, days_overdue));
            Value::Object(m)
        })
        .collect();
    json!({ "asOf": as_of.to_string(), "summary": summary, "loans": loans })
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_are_camel_case() {
        assert_eq!(super::camel("interest_charged_total"), "interestChargedTotal");
    }
}
