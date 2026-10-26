use super::*;
use crate::domain::loans::{Frequency, LoanTerms};
use crate::domain::money::Rate;
use crate::domain::portfolio::{CapitalKind, EntryKind};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn terms(principal: Cents, start: NaiveDate) -> LoanTerms {
    LoanTerms::interest_only(principal, Rate(100_000), Frequency::Monthly, start)
}

fn client(store: &mut Store) -> i64 {
    store.create_client(&ClientInput { full_name: "Marta Quiroga".into(), ..Default::default() }).unwrap()
}

/// A store with 20,000 of capital and a 1,000 loan at 10% a month from
/// Jan 1, 2026, on which 600 was paid on Feb 1 (100 interest, 500 capital).
fn lent(today: NaiveDate) -> (Store, i64, i64, i64) {
    let mut s = Store::in_memory();
    let cid = client(&mut s);
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "Opening capital", today).unwrap();
    let lid = s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap();
    let pid = s.record_payment(lid, 60_000, 0, d(2026, 2, 1), "", today).unwrap();
    (s, cid, lid, pid)
}

fn split(s: &Store, pid: i64) -> (Cents, Cents) {
    let p = s.snapshot().unwrap().payments.into_iter().find(|p| p.id == pid).unwrap();
    (p.interest, p.principal)
}

#[test]
fn full_lifecycle_through_the_database() {
    let today = d(2026, 6, 1);
    let (mut s, _, lid, pid) = lent(today);
    // 1,000 at 10%: paying 600 is 100 interest and 500 capital.
    assert_eq!(split(&s, pid), (10_000, 50_000));
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.book.summary(today).interest_collected, 10_000);

    // Pay off in full; the loan closes in the same transaction.
    let book = snap.book_loan(lid).unwrap();
    let payoff = crate::domain::ledger::owed_at_payment(&book.facts, &book.payments, today).payoff();
    s.record_payment(lid, payoff, 0, today, "Payoff", today).unwrap();
    assert_eq!(s.snapshot().unwrap().loan(lid).unwrap().status, ContractStatus::PaidOff);
    assert!(s.record_payment(lid, 100, 0, today, "", today).is_err());
}

#[test]
fn editing_a_loan_splits_its_payments_again_and_is_logged() {
    let today = d(2026, 6, 1);
    let (mut s, cid, lid, pid) = lent(today);
    s.update_loan(lid, cid, &terms(200_000, d(2026, 1, 1)), None, "", "Typo in amount", today).unwrap();
    assert_eq!(split(&s, pid), (20_000, 40_000));
    let snap = s.snapshot().unwrap();
    let change = &snap.changes[0];
    assert_eq!((change.entity, change.entity_id, change.action), (Entity::Loan, lid, Action::Update));
    assert_eq!(change.reason, "Typo in amount");
    assert_eq!(change.fields, vec![FieldChange { field: "principal".into(), old: "100000".into(), new: "200000".into() }]);
    // Saving without changes records nothing.
    s.update_loan(lid, cid, &terms(200_000, d(2026, 1, 1)), None, "", "", today).unwrap();
    assert_eq!(s.snapshot().unwrap().changes.len(), 1);
}

#[test]
fn an_edit_that_breaks_a_payment_is_refused_and_nothing_changes() {
    let today = d(2026, 6, 1);
    let (mut s, cid, lid, pid) = lent(today);
    // 400 lent: the 600 payment would be more than the 440 owed.
    let err = s.update_loan(lid, cid, &terms(40_000, d(2026, 1, 1)), None, "", "", today).unwrap_err();
    assert!(err.downcast_ref::<Invalid>().unwrap().0.contains("more than the client owed"));
    assert_eq!(s.snapshot().unwrap().loan(lid).unwrap().terms.principal, 100_000);
    assert_eq!(split(&s, pid), (10_000, 50_000));
    assert!(s.snapshot().unwrap().changes.is_empty());
}

#[test]
fn payments_can_be_backdated_edited_and_deleted() {
    let today = d(2026, 6, 1);
    let (mut s, _, lid, pid) = lent(today);
    // A payment dated before the existing one now comes first.
    let early = s.record_payment(lid, 10_000, 0, d(2026, 1, 20), "", today).unwrap();
    assert_eq!(split(&s, early), (10_000, 0));
    // January's interest is already paid, so all 600 on Feb 1 is capital.
    assert_eq!(split(&s, pid), (0, 60_000));

    s.update_payment(pid, 30_000, 0, d(2026, 2, 1), "", "Was 300", today).unwrap();
    assert_eq!(split(&s, pid), (0, 30_000));

    // Without the early payment, the 300 covers January's 100 first.
    s.delete_payment(early, "Entered by mistake", today).unwrap();
    assert_eq!(split(&s, pid), (10_000, 20_000));
    let snap = s.snapshot().unwrap();
    assert!(snap.payment(early).is_none());
    assert_eq!(snap.changes.len(), 2);
    assert_eq!(snap.changes[0].action, Action::Delete);
    assert_eq!(snap.changes[0].loan_id, Some(lid));
}

#[test]
fn deleting_the_payoff_reopens_the_loan() {
    let today = d(2026, 6, 1);
    let (mut s, _, lid, _) = lent(today);
    // 500 of capital and February's 50 of interest.
    let payoff = s.record_payment(lid, 55_000, 0, d(2026, 3, 1), "", today).unwrap();
    assert_eq!(s.snapshot().unwrap().loan(lid).unwrap().status, ContractStatus::PaidOff);
    s.delete_payment(payoff, "", today).unwrap();
    assert_eq!(s.snapshot().unwrap().loan(lid).unwrap().status, ContractStatus::Active);
}

#[test]
fn deleting_a_loan_removes_its_payments() {
    let today = d(2026, 6, 1);
    let (mut s, _, lid, _) = lent(today);
    s.delete_loan(lid, "Duplicate").unwrap();
    let snap = s.snapshot().unwrap();
    assert!(snap.loans.is_empty() && snap.payments.is_empty());
    assert_eq!(snap.book.summary(today).available_cash, 2_000_000);
    let f = &snap.changes[0].fields;
    assert!(f.iter().any(|x| x.field == "received" && x.old == "60000"));
}

#[test]
fn cannot_lend_or_withdraw_money_that_is_not_there() {
    let mut s = Store::in_memory();
    let today = d(2026, 6, 1);
    let cid = client(&mut s);
    let err = s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap_err();
    assert_eq!(err.downcast_ref::<InsufficientCash>().unwrap().available, 0);
    s.add_capital(CapitalKind::Contribution, 100_000, d(2026, 1, 1), "", today).unwrap();
    s.create_loan(cid, &terms(100_000, d(2026, 1, 1)), "", today).unwrap();
    assert!(s.add_capital(CapitalKind::Withdrawal, 1, d(2026, 1, 2), "", today).is_err());
    s.add_capital(CapitalKind::Contribution, 50_000, d(2026, 3, 1), "", today).unwrap();
    assert!(s.create_loan(cid, &terms(50_000, d(2026, 2, 1)), "", today).is_err());
}

#[test]
fn removing_capital_that_was_lent_is_refused() {
    let today = d(2026, 6, 1);
    let (mut s, _, _, _) = lent(today);
    let cap = s.snapshot().unwrap().capital[0].id;
    let err = s.delete_capital(cap, "").unwrap_err();
    let short = err.downcast_ref::<CashShortfall>().unwrap().0;
    assert_eq!((short.date, short.account), (d(2026, 1, 1), None));
    // Lowering it to what is still needed is fine.
    s.update_capital(cap, CapitalKind::Contribution, 100_000, d(2026, 1, 1), "", "Real amount", today).unwrap();
    assert_eq!(s.snapshot().unwrap().book.summary(today).available_capital, 50_000);
}

#[test]
fn expenses_come_from_the_chosen_account() {
    let today = d(2026, 6, 1);
    let (mut s, _, _, _) = lent(today);
    // Interest available: 100. Capital available: 19,000 + 500.
    let err = s.add_expense(Account::Interest, 20_000, d(2026, 3, 1), "Phone", today).unwrap_err();
    let e = err.downcast_ref::<InsufficientCash>().unwrap();
    assert_eq!((e.available, e.account), (10_000, Some(Account::Interest)));
    assert!(s.add_expense(Account::Interest, 5_000, d(2026, 3, 1), "  ", today).is_err(), "needs a description");

    let id = s.add_expense(Account::Interest, 4_000, d(2026, 3, 1), "Phone", today).unwrap();
    let sum = s.snapshot().unwrap().book.summary(today);
    assert_eq!((sum.available_interest, sum.available_capital), (6_000, 1_950_000));
    assert_eq!(sum.interest_collected, 10_000);

    s.update_entry(id, Account::Capital, 4_000, d(2026, 3, 1), "Phone", "Paid from capital", today).unwrap();
    let sum = s.snapshot().unwrap().book.summary(today);
    assert_eq!((sum.available_interest, sum.available_capital), (10_000, 1_946_000));
    s.delete_entry(id, "").unwrap();
    assert!(s.snapshot().unwrap().entries.is_empty());
}

#[test]
fn transfers_and_adjustments() {
    let today = d(2026, 6, 1);
    let (mut s, _, _, _) = lent(today);
    s.add_transfer(Account::Interest, 10_000, d(2026, 3, 1), "Reinvest", today).unwrap();
    let sum = s.snapshot().unwrap().book.summary(today);
    assert_eq!((sum.available_capital, sum.available_interest), (1_960_000, 0));
    assert!(s.add_transfer(Account::Interest, 1, d(2026, 3, 1), "", today).is_err());

    // The cash really on hand is 19,500 and 20.
    assert_eq!(s.adjust_balances(1_950_000, 2_000, today, "Counted the cash", today).unwrap(), 2);
    let snap = s.snapshot().unwrap();
    let sum = snap.book.summary(today);
    assert_eq!((sum.available_capital, sum.available_interest), (1_950_000, 2_000));
    assert!(snap.entries.iter().filter(|e| e.kind == EntryKind::Adjustment).count() == 2);
    assert!(s.adjust_balances(1_950_000, 2_000, today, "Again", today).is_err(), "nothing to adjust");
    assert!(s.adjust_balances(1, 1, today, " ", today).is_err(), "needs a reason");
}

#[test]
fn failed_payment_leaves_no_trace() {
    let mut s = Store::in_memory();
    let today = d(2026, 6, 1);
    let cid = client(&mut s);
    s.add_capital(CapitalKind::Contribution, 1_000_000, d(2026, 1, 1), "", today).unwrap();
    let lid = s.create_loan(cid, &terms(1_000_000, d(2026, 1, 1)), "", today).unwrap();
    assert!(s.record_payment(lid, 99_999_999, 0, d(2026, 2, 1), "", today).is_err());
    assert!(s.snapshot().unwrap().payments.is_empty());
    // A rejected allocation at the database level rolls back the payment row.
    let tx_result: Result<()> = s.write(|c| {
        payments::insert(
            c,
            lid,
            d(2026, 2, 1),
            1_000,
            &Allocation { interest: 1, principal: 1, fee: 0, principal_before: 0, principal_after: 0, interest_after: 0 },
            "",
        )?;
        Ok(())
    });
    assert!(tx_result.is_err());
    let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM payments", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0);
}

#[test]
fn client_validation_archive_and_delete() {
    let mut s = Store::in_memory();
    assert!(s.create_client(&ClientInput { full_name: "  ".into(), ..Default::default() }).is_err());
    assert!(s.create_client(&ClientInput { full_name: "A".into(), email: "nope".into(), ..Default::default() }).is_err());
    let id = client(&mut s);
    s.set_client_archived(id, true).unwrap();
    s.add_capital(CapitalKind::Contribution, 100_000, d(2026, 1, 1), "", d(2026, 6, 1)).unwrap();
    assert!(s.create_loan(id, &terms(1_000, d(2026, 1, 1)), "", d(2026, 6, 1)).is_err());
    s.update_client(id, &ClientInput { full_name: "Marta Quiroga Ruiz".into(), ..Default::default() }).unwrap();
    assert_eq!(s.snapshot().unwrap().changes[0].fields[0].new, "Marta Quiroga Ruiz");
    s.delete_client(id, "Never borrowed").unwrap();
    assert!(s.snapshot().unwrap().clients.is_empty());

    let (mut s, cid, _, _) = lent(d(2026, 6, 1));
    assert!(s.delete_client(cid, "").is_err(), "clients with loans are kept");
}

#[test]
fn settings_round_trip() {
    let mut s = Store::in_memory();
    let mut st = s.snapshot().unwrap().settings;
    assert_eq!(st, Settings::default());
    st.currency_code = "USD".into();
    st.receipt_name = "Préstamos Valdez".into();
    st.language = crate::i18n::Lang::Es;
    st.default_rate = Rate(150_000);
    st.sidebar_collapsed = true;
    s.save_settings(&st).unwrap();
    assert_eq!(s.snapshot().unwrap().settings, st);
    st.receipt_name = "x".repeat(61);
    assert!(s.save_settings(&st).is_err());
}

#[test]
fn logo_is_stored_with_the_data() {
    let mut s = Store::in_memory();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(800, 400, image::Rgba([0, 0, 0, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    s.set_logo(png.get_ref()).unwrap();
    let logo = s.snapshot().unwrap().logo.unwrap();
    assert_eq!(image::load_from_memory(&logo).unwrap().width(), 512);
    assert!(s.set_logo(b"text").is_err());
    s.remove_logo().unwrap();
    assert!(s.snapshot().unwrap().logo.is_none());
}

#[test]
fn writes_a_loan_receipt() {
    let today = d(2026, 6, 1);
    let (s, _, lid, _) = lent(today);
    let snap = s.snapshot().unwrap();
    let r = receipt::loan_data(&snap, lid).unwrap();
    assert_eq!((r.principal, r.interest_per_period, r.payoff_at_first_due()), (100_000, 10_000, 110_000));
    assert_eq!(r.upcoming.len(), 6);
    assert_eq!(r.upcoming[0], (1, d(2026, 2, 1), 10_000));
    assert_eq!(r.upcoming[5].1, d(2026, 7, 1));
    let dir = tempfile::tempdir().unwrap();
    let html = std::fs::read_to_string(receipt::write_loan(&snap, lid, dir.path()).unwrap()).unwrap();
    assert!(html.contains("Marta Quiroga") && html.contains("Loan receipt"));
}

#[test]
fn writes_a_receipt() {
    let today = d(2026, 6, 1);
    let (s, _, _, pid) = lent(today);
    let dir = tempfile::tempdir().unwrap();
    let snap = s.snapshot().unwrap();
    let data = receipt::data(&snap, pid).unwrap();
    assert_eq!((data.interest, data.principal), (10_000, 50_000));
    assert_eq!((data.principal_before, data.principal_after), (100_000, 50_000));
    assert_eq!(data.next_due, Some((d(2026, 3, 1), 5_000)));
    let path = receipt::write(&snap, pid, dir.path()).unwrap();
    let html = std::fs::read_to_string(path).unwrap();
    assert!(html.contains("Marta Quiroga"));
    assert!(html.contains("50.0%"));
}
