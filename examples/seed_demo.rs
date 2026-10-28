//! Builds a demo database for screenshots and manual testing.
//!
//!     cargo run --example seed_demo -- <path-to-new.db> [es]
//!
//! Refuses to touch an existing file. Every record goes through the same
//! validated services the app uses, with dates relative to today.

use anyhow::{Result, bail};
use chrono::{Duration, NaiveDate};
use vz_finance::database::repositories::clients::ClientInput;
use vz_finance::domain::installments::Instruction;
use vz_finance::domain::ledger::owed_at_payment;
use vz_finance::domain::loans::{Frequency, LoanTerms, Method};
use vz_finance::domain::money::{Cents, Rate};
use vz_finance::domain::portfolio::{Account, CapitalKind};
use vz_finance::i18n::Lang;
use vz_finance::services::Store;
use vz_finance::utils::dates::today;

fn client(s: &mut Store, name: &str, phone: &str, notes: &str) -> Result<i64> {
    s.create_client(&ClientInput { full_name: name.into(), phone: phone.into(), notes: notes.into(), ..Default::default() })
}

fn loan(s: &mut Store, client: i64, principal: Cents, pct: i64, frequency: Frequency, start: NaiveDate) -> Result<i64> {
    let terms = LoanTerms::interest_only(principal, Rate(pct * 10_000), frequency, start);
    s.create_loan(client, &terms, "", today())
}

/// Pays on each due date until today: `extra(period)` is the capital paid on
/// top of the interest, or `None` to skip that payment.
fn pay_periods(s: &mut Store, loan_id: i64, extra: impl Fn(u32) -> Option<Cents>) -> Result<()> {
    for k in 1..200 {
        let snap = s.snapshot()?;
        let book = snap.book_loan(loan_id).unwrap();
        let due = book.facts.frequency.due_date(book.facts.start_date, k);
        if due > today() || snap.loan(loan_id).unwrap().status != vz_finance::domain::ledger::ContractStatus::Active {
            break;
        }
        let Some(capital) = extra(k) else { continue };
        let owed = owed_at_payment(&book.facts, &book.payments, due);
        let amount = (owed.interest_outstanding + capital).min(owed.payoff());
        if amount > 0 {
            s.record_payment(loan_id, amount, 0, due, "", today())?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let Some(path) = std::env::args().nth(1) else { bail!("usage: seed_demo <path-to-new.db> [es]") };
    let spanish = std::env::args().nth(2).as_deref() == Some("es");
    let path = std::path::PathBuf::from(path);
    if path.exists() {
        bail!("{} already exists; refusing to modify it", path.display());
    }
    let mut s = Store::open(path.clone())?;
    let t = today();
    let ago = |d: i64| t - Duration::days(d);

    let mut settings = s.snapshot()?.settings;
    settings.receipt_name = "Préstamos Valdez".into();
    settings.currency_code = "DOP".into();
    if spanish {
        settings.language = Lang::Es;
    }
    s.save_settings(&settings)?;

    s.add_capital(CapitalKind::Contribution, 30_000_000, ago(200), "Ahorros para prestar", t)?;
    s.add_capital(CapitalKind::Contribution, 10_000_000, ago(90), "", t)?;

    let marta = client(&mut s, "Marta Quiroga", "809-555-0142", "Prefiere WhatsApp después de las 6 pm.")?;
    let daniel = client(&mut s, "Daniel Okafor", "829-555-0178", "Negocio de comida; flujo bajo en diciembre.")?;
    let priya = client(&mut s, "Priya Raman", "849-555-0163", "")?;
    let tomas = client(&mut s, "Tomás Herrera", "809-555-0119", "Pagó parcial en septiembre.")?;
    let grace = client(&mut s, "Grace Lindqvist", "829-555-0190", "")?;
    let samuel = client(&mut s, "Samuel Mensah", "809-555-0101", "Préstamo para equipo, en cuotas.")?;

    // 50,000 at 10% monthly; pays interest plus 5,000 of capital each month.
    let a = loan(&mut s, marta, 5_000_000, 10, Frequency::Monthly, ago(185))?;
    pay_periods(&mut s, a, |_| Some(500_000))?;

    // 20,000 at 15% monthly; paid interest twice, then stopped.
    let b = loan(&mut s, daniel, 2_000_000, 15, Frequency::Monthly, ago(120))?;
    pay_periods(&mut s, b, |k| if k <= 2 { Some(0) } else { None })?;

    // 10,000 at 5% every 15 days; paid off after a few periods.
    let c = loan(&mut s, priya, 1_000_000, 5, Frequency::Biweekly, ago(160))?;
    pay_periods(&mut s, c, |k| if k < 4 { Some(0) } else { Some(1_000_000) })?;

    // 35,000 at 10% monthly; interest only, last one paid short.
    let d = loan(&mut s, tomas, 3_500_000, 10, Frequency::Monthly, ago(95))?;
    pay_periods(&mut s, d, |k| match k {
        1 | 2 => Some(0),
        _ => None,
    })?;
    {
        let snap = s.snapshot()?;
        let book = snap.book_loan(d).unwrap();
        let due = book.facts.frequency.due_date(book.facts.start_date, 3);
        if due <= t {
            s.record_payment(d, 200_000, 0, due, "Parcial", t)?;
        }
    }

    // 15,000 at 10% every 15 days, recent.
    let e = loan(&mut s, grace, 1_500_000, 10, Frequency::Biweekly, ago(26))?;
    pay_periods(&mut s, e, |_| Some(0))?;

    // A second loan for Priya, current.
    let f = loan(&mut s, priya, 2_500_000, 10, Frequency::Monthly, ago(40))?;
    pay_periods(&mut s, f, |_| Some(250_000))?;

    // 30,000 at 10% a month in 6 declining installments: two paid on time,
    // the third paid short (so it is overdue with a 5% late fee).
    let inst = LoanTerms {
        principal: 3_000_000,
        rate: Rate(100_000),
        interest_period: Frequency::Monthly,
        frequency: Frequency::Monthly,
        start_date: ago(100),
        method: Method::Declining,
        installments: Some(6),
        late_fee_rate: Rate(50_000),
    };
    let g = s.create_loan(samuel, &inst, "Equipo de cocina", t)?;
    let schedule = s.snapshot()?.loan(g).unwrap().schedule.clone();
    for x in schedule.iter().take(2) {
        s.record_installment_payment(g, x.total(), x.due, "", &Instruction::Auto, t)?;
    }
    if let Some(x) = schedule.get(2).filter(|x| x.due < t) {
        s.record_installment_payment(g, 300_000, x.due, "Pago parcial", &Instruction::Installment(3), t)?;
    }

    // 8,000 flat at 5% a week in 8 weekly installments, recent.
    let weekly = LoanTerms {
        principal: 800_000,
        rate: Rate(50_000),
        interest_period: Frequency::Weekly,
        frequency: Frequency::Weekly,
        start_date: ago(17),
        method: Method::Flat,
        installments: Some(8),
        late_fee_rate: Rate(50_000),
    };
    let w = s.create_loan(grace, &weekly, "", t)?;
    let first = s.snapshot()?.loan(w).unwrap().schedule[0].clone();
    s.record_installment_payment(w, first.total(), first.due, "", &Instruction::Auto, t)?;

    s.add_capital(CapitalKind::Withdrawal, 2_000_000, ago(30), "Gasto personal", t)?;

    // Interest put back to work, an expense paid from interest, and a
    // corrected note so the change history has something in it.
    s.add_transfer(Account::Interest, 1_000_000, ago(20), "Reinvertir ganancias", t)?;
    s.add_expense(Account::Interest, 350_000, ago(6), "Reparación del carro", t)?;
    s.add_income(Account::Interest, 800_000, ago(12), "Alquiler del local", t)?;
    let snap = s.snapshot()?;
    let fl = snap.loan(f).unwrap();
    s.update_loan(f, fl.client_id, &fl.terms.clone(), None, "Para inventario de la tienda", "Faltaba el motivo", t)?;

    s.refresh(t)?;
    let snap = s.snapshot()?;
    let sum = snap.book.summary(t);
    let m = snap.settings.money();
    let overdue: Vec<_> = snap.book.loans.iter().filter(|l| l.delinquency(t).overdue_amount > 0).map(|l| l.id).collect();
    println!(
        "Seeded {}: {} clients, {} loans, {} payments. Value {} / cash {} / overdue {} (loans {:?})",
        path.display(),
        snap.clients.len(),
        snap.loans.len(),
        snap.payments.len(),
        m.format(sum.portfolio_value),
        m.format(sum.available_cash),
        m.format(sum.overdue_amount),
        overdue
    );
    Ok(())
}
