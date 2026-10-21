//! Builds a demo database for screenshots and manual testing.
//!
//!     cargo run --example seed_demo -- <path-to-new.db>
//!
//! Refuses to touch an existing file. Every record goes through the same
//! validated services the app uses, with dates relative to today.

use anyhow::{Result, bail};
use chrono::{Duration, NaiveDate};
use vz_finance::database::repositories::clients::ClientInput;
use vz_finance::domain::loans::{Frequency, InterestMethod, LoanTerms};
use vz_finance::domain::money::Rate;
use vz_finance::domain::portfolio::CapitalKind;
use vz_finance::services::Store;
use vz_finance::utils::dates::today;

fn client(s: &mut Store, name: &str, phone: &str, email: &str, notes: &str) -> Result<i64> {
    s.create_client(&ClientInput { full_name: name.into(), phone: phone.into(), email: email.into(), notes: notes.into() })
}

#[allow(clippy::too_many_arguments)]
fn loan(s: &mut Store, client: i64, principal: i64, rate_pct: i64, method: InterestMethod, freq: Frequency, n: u32, start: NaiveDate, notes: &str) -> Result<i64> {
    let terms = LoanTerms { principal, rate: Rate(rate_pct * 10_000), method, frequency: freq, term_count: n, start_date: start };
    s.create_loan(client, &terms, notes, today())
}

/// Pays each scheduled installment on its due date while `keep(seq)` holds.
fn pay_on_schedule(s: &mut Store, loan_id: i64, keep: impl Fn(u32) -> Option<i64>) -> Result<()> {
    let book = s.snapshot()?.book_loan(loan_id).cloned().expect("loan");
    for row in &book.schedule {
        if row.due_date > today() {
            break;
        }
        match keep(row.seq) {
            Some(0) => break,
            Some(amount) => {
                s.record_payment(loan_id, amount, 0, row.due_date, "", today())?;
            }
            None => {
                // Pay what is actually owed on the day, capped at the payoff.
                let pos = s.snapshot()?.book_loan(loan_id).unwrap().position(row.due_date);
                let amount = row.total().min(pos.payoff());
                if amount > 0 {
                    s.record_payment(loan_id, amount, 0, row.due_date, "", today())?;
                }
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let Some(path) = std::env::args().nth(1) else { bail!("usage: seed_demo <path-to-new.db>") };
    let path = std::path::PathBuf::from(path);
    if path.exists() {
        bail!("{} already exists; refusing to modify it", path.display());
    }
    let mut s = Store::open(path.clone())?;
    let t = today();
    let ago = |d: i64| t - Duration::days(d);

    s.add_capital(CapitalKind::Contribution, 4_000_000, ago(230), "Savings set aside for lending", t)?;
    s.add_capital(CapitalKind::Contribution, 1_500_000, ago(120), "Bonus", t)?;

    let marta = client(&mut s, "Marta Quiroga", "+1 (305) 555-0142", "marta.quiroga@example.com", "Referred by Daniel. Prefers WhatsApp after 6 pm.")?;
    let daniel = client(&mut s, "Daniel Okafor", "+1 (305) 555-0178", "", "Runs a catering business; seasonal cash flow in winter.")?;
    let priya = client(&mut s, "Priya Raman", "", "priya.raman@example.com", "")?;
    let tomas = client(&mut s, "Tomás Herrera", "+1 (786) 555-0119", "", "Car repair loan. Paid partially in July, promised to catch up.")?;
    let grace = client(&mut s, "Grace Lindqvist", "+1 (954) 555-0163", "grace.l@example.com", "")?;
    client(&mut s, "Samuel Mensah", "+1 (305) 555-0101", "", "Asked about a loan for equipment; follow up next month.")?;

    let a = loan(&mut s, marta, 1_200_000, 14, InterestMethod::Amortized, Frequency::Monthly, 12, ago(210), "Home renovation")?;
    pay_on_schedule(&mut s, a, |_| None)?;

    let c = loan(&mut s, priya, 500_000, 12, InterestMethod::Simple, Frequency::AtMaturity, 4, ago(200), "Short bridge loan")?;
    pay_on_schedule(&mut s, c, |_| None)?;

    let b = loan(&mut s, daniel, 800_000, 18, InterestMethod::InterestOnly, Frequency::Monthly, 6, ago(150), "Equipment, principal at maturity")?;
    pay_on_schedule(&mut s, b, |seq| if seq <= 3 { None } else { Some(0) })?;

    let e = loan(&mut s, tomas, 1_000_000, 16, InterestMethod::Amortized, Frequency::Monthly, 10, ago(95), "")?;
    pay_on_schedule(&mut s, e, |seq| match seq {
        1 | 2 => None,
        3 => Some(40_000),
        _ => Some(0),
    })?;

    loan(&mut s, grace, 300_000, 10, InterestMethod::Simple, Frequency::AtMaturity, 3, ago(86), "")?;

    let d = loan(&mut s, priya, 650_000, 15, InterestMethod::Amortized, Frequency::Biweekly, 20, ago(60), "Second loan after the bridge was repaid")?;
    pay_on_schedule(&mut s, d, |_| None)?;

    s.add_capital(CapitalKind::Withdrawal, 250_000, ago(40), "Personal expense", t)?;

    let snap = s.snapshot()?;
    let sum = snap.book.summary(t);
    println!(
        "Seeded {}: {} clients, {} loans, {} payments. Value {} / cash {} / overdue {}",
        path.display(),
        snap.clients.len(),
        snap.loans.len(),
        snap.payments.len(),
        snap.settings.money().format(sum.portfolio_value),
        snap.settings.money().format(sum.available_cash),
        snap.settings.money().format(sum.overdue_amount)
    );
    Ok(())
}
