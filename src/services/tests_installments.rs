//! Installment loans through the database: schedules, the three ways to
//! apply a payment, late fees, edits, reset, and settings that must survive
//! a restart or a restore.

use super::*;
use crate::domain::installments::{Installment, InstallmentStatus, Instruction};
use crate::domain::loans::{Frequency, LoanTerms, Method};
use crate::domain::money::Rate;
use crate::domain::portfolio::CapitalKind;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// 10% a month, monthly installments, 5% late fee.
fn terms(principal: Cents, count: u32, method: Method) -> LoanTerms {
    LoanTerms {
        principal,
        rate: Rate(100_000),
        interest_period: Frequency::Monthly,
        frequency: Frequency::Monthly,
        start_date: d(2026, 1, 1),
        method,
        installments: Some(count),
        late_fee_rate: Rate(50_000),
    }
}

/// 20,000 of capital and 10,000 lent on Jan 1, 2026 in 5 installments.
fn store(method: Method) -> (Store, i64, i64) {
    let today = d(2026, 12, 31);
    let mut s = Store::in_memory();
    let cid = s.create_client(&ClientInput { full_name: "Daniel Okafor".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
    let lid = s.create_loan(cid, &terms(1_000_000, 5, method), "", today).unwrap();
    (s, cid, lid)
}

fn pay(s: &mut Store, lid: i64, amount: Cents, on: NaiveDate, how: Instruction) -> i64 {
    s.record_installment_payment(lid, amount, on, "", &how, on).unwrap()
}

fn state(s: &Store, lid: i64, on: NaiveDate) -> crate::domain::installments::PlanState {
    s.snapshot().unwrap().book_loan(lid).unwrap().plan_state(on).unwrap()
}

#[test]
fn creates_the_declining_schedule_of_the_example() {
    let (s, _, lid) = store(Method::Declining);
    let snap = s.snapshot().unwrap();
    let loan = snap.loan(lid).unwrap();
    let totals: Vec<_> = loan.schedule.iter().map(Installment::total).collect();
    assert_eq!(totals, vec![300_000, 280_000, 260_000, 240_000, 220_000]);
    assert_eq!(loan.schedule.iter().map(|x| x.interest).sum::<Cents>(), 300_000);
    assert!(!loan.schedule_edited);
    let sum = snap.book.summary(d(2026, 1, 1));
    assert_eq!((sum.outstanding_principal, sum.available_cash), (1_000_000, 1_000_000));
}

#[test]
fn flat_interest_schedule() {
    let (s, _, lid) = store(Method::Flat);
    let snap = s.snapshot().unwrap();
    assert!(snap.loan(lid).unwrap().schedule.iter().all(|x| x.principal == 200_000 && x.interest == 100_000));
}

#[test]
fn option_one_partial_payment_and_one_late_fee() {
    let (mut s, _, lid) = store(Method::Declining);
    pay(&mut s, lid, 100_000, d(2026, 2, 1), Instruction::Installment(1));
    // Brought up to date twice (two app starts): one fee, assessed once.
    assert_eq!(s.refresh(d(2026, 2, 20)).unwrap(), 1);
    assert_eq!(s.refresh(d(2026, 2, 20)).unwrap(), 0);
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.late_fees.len(), 1);
    let fee = &snap.late_fees[0];
    assert_eq!((fee.seq, fee.assessed_on, fee.base_amount, fee.amount), (1, d(2026, 2, 2), 200_000, 10_000));

    let st = state(&s, lid, d(2026, 2, 20));
    let first = &st.installments[0];
    assert_eq!(first.status(d(2026, 2, 20)), InstallmentStatus::Overdue);
    assert_eq!(first.owed(), 210_000, "2,000 of capital and the 100 fee");
    let sum = s.snapshot().unwrap().book.summary(d(2026, 2, 20));
    assert_eq!(sum.fees_receivable, 10_000);
    assert_eq!(sum.overdue_amount, 210_000);
    assert_eq!(sum.portfolio_value, sum.available_cash + sum.outstanding_principal + sum.interest_receivable + sum.fees_receivable);

    // Paying the rest later: fee first, then capital; the fee doesn't change.
    pay(&mut s, lid, 210_000, d(2026, 2, 25), Instruction::Installment(1));
    let st = state(&s, lid, d(2026, 2, 25));
    assert_eq!(st.installments[0].status(d(2026, 2, 25)), InstallmentStatus::Paid);
    assert_eq!(s.snapshot().unwrap().late_fees[0].amount, 10_000);
    // More than an installment owes is refused, with a reason.
    let err = s.record_installment_payment(lid, 900_000, d(2026, 3, 1), "", &Instruction::Installment(2), d(2026, 12, 31)).unwrap_err();
    assert!(err.downcast_ref::<Invalid>().is_some());
}

#[test]
fn option_two_spread_with_chosen_amounts() {
    let (mut s, _, lid) = store(Method::Declining);
    // Nothing paid by Mar 10: installments 1 and 2 are overdue with 150 each.
    s.refresh(d(2026, 3, 10)).unwrap();
    assert_eq!(s.snapshot().unwrap().late_fees.iter().map(|f| f.amount).collect::<Vec<_>>(), vec![15_000, 15_000]);
    let pid = pay(&mut s, lid, 415_000, d(2026, 3, 10), Instruction::Spread(vec![(1, 315_000), (2, 100_000)]));
    let st = state(&s, lid, d(2026, 3, 10));
    assert_eq!(st.installments[0].remaining(), 0);
    assert_eq!((st.installments[1].paid_fee, st.installments[1].paid_interest), (15_000, 85_000));
    let snap = s.snapshot().unwrap();
    let p = snap.payment(pid).unwrap();
    assert_eq!((p.fee, p.interest, p.principal), (30_000, 185_000, 200_000));
    assert_eq!(p.applied.len(), 2);

    // The receipt shows where the money went and the late fees.
    let r = receipt::data(&snap, pid).unwrap();
    assert_eq!(r.installments.len(), 2);
    assert!(r.installments[0].paid);
    let html = receipt::render(&r, &snap);
    assert!(html.contains("Applied to") && html.contains("Late fees"));
}

#[test]
fn option_three_custom_split_and_limits() {
    let (mut s, _, lid) = store(Method::Declining);
    pay(&mut s, lid, 300_000, d(2026, 1, 15), Instruction::Custom { principal: 300_000, interest: 0, fee: 0 });
    let st = state(&s, lid, d(2026, 1, 15));
    assert_eq!(st.principal_outstanding(), 700_000);
    // Capital paid ahead lowers the next interest (on 7,000).
    assert_eq!(state(&s, lid, d(2026, 2, 2)).installments[1].interest, 70_000);
    let too_much = Instruction::Custom { principal: 0, interest: 200_000, fee: 0 };
    let err = s.record_installment_payment(lid, 200_000, d(2026, 1, 20), "", &too_much, d(2026, 12, 31)).unwrap_err();
    assert!(err.downcast_ref::<Invalid>().unwrap().0.contains("interest"));
    let err = s.record_installment_payment(lid, 5_000_000, d(2026, 1, 20), "", &Instruction::Auto, d(2026, 12, 31)).unwrap_err();
    assert!(err.downcast_ref::<Invalid>().unwrap().0.contains("more than"));
    assert_eq!(s.snapshot().unwrap().payments.len(), 1, "refused payments leave nothing behind");
}

#[test]
fn paying_on_time_closes_the_loan_without_fees() {
    let (mut s, _, lid) = store(Method::Declining);
    let schedule = s.snapshot().unwrap().loan(lid).unwrap().schedule.clone();
    for x in &schedule {
        pay(&mut s, lid, x.total(), x.due, Instruction::Auto);
    }
    s.refresh(d(2026, 12, 31)).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.loan(lid).unwrap().status, ContractStatus::PaidOff);
    assert!(snap.late_fees.is_empty());
    let sum = snap.book.summary(d(2026, 12, 31));
    assert_eq!((sum.interest_collected, sum.outstanding_principal), (300_000, 0));
    // Deleting the last payment reopens it.
    let last = snap.payments.last().unwrap().id;
    s.delete_payment(last, "", d(2026, 12, 31)).unwrap();
    assert_eq!(s.snapshot().unwrap().loan(lid).unwrap().status, ContractStatus::Active);
}

#[test]
fn edited_schedule_is_checked_and_kept() {
    let today = d(2026, 12, 31);
    let mut s = Store::in_memory();
    let cid = s.create_client(&ClientInput { full_name: "Ana".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
    let t = terms(1_000_000, 2, Method::Declining);
    let mut edited = vec![
        Installment { seq: 1, due: d(2026, 2, 15), principal: 500_000, interest: 40_000 },
        Installment { seq: 2, due: d(2026, 3, 15), principal: 400_000, interest: 40_000 },
    ];
    let err = s.create_loan_with_schedule(cid, &t, Some(edited.clone()), "", today).unwrap_err();
    assert!(err.downcast_ref::<Invalid>().unwrap().0.contains("capital"));
    edited[1].principal = 500_000;
    let lid = s.create_loan_with_schedule(cid, &t, Some(edited.clone()), "", today).unwrap();
    let loan = s.snapshot().unwrap().loan(lid).unwrap().clone();
    assert!(loan.schedule_edited);
    assert_eq!(loan.schedule, edited);
    // Its interest is used as written.
    pay(&mut s, lid, 540_000, d(2026, 2, 15), Instruction::Auto);
    assert_eq!(state(&s, lid, d(2026, 2, 16)).installments[1].interest, 40_000);
    // Saving the loan again with the same terms keeps the edited schedule.
    s.update_loan(lid, cid, &t, None, "Changed the note", "", today).unwrap();
    assert_eq!(s.snapshot().unwrap().loan(lid).unwrap().schedule, edited);
}

#[test]
fn changing_terms_rebuilds_the_schedule_and_resplits_payments() {
    let (mut s, cid, lid) = store(Method::Declining);
    let pid = pay(&mut s, lid, 300_000, d(2026, 2, 1), Instruction::Auto);
    s.update_loan(lid, cid, &terms(2_000_000, 5, Method::Declining), None, "", "Was 20,000", d(2026, 12, 31)).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.loan(lid).unwrap().schedule[0].total(), 600_000);
    let p = snap.payment(pid).unwrap();
    assert_eq!((p.interest, p.principal), (200_000, 100_000));
    assert!(snap.changes[0].fields.iter().any(|f| f.field == "schedule"));
}

#[test]
fn waiving_a_fee_works_the_payments_out_again() {
    let (mut s, _, lid) = store(Method::Declining);
    pay(&mut s, lid, 100_000, d(2026, 2, 1), Instruction::Installment(1));
    s.refresh(d(2026, 2, 20)).unwrap();
    let pid = pay(&mut s, lid, 210_000, d(2026, 2, 20), Instruction::Auto);
    assert_eq!(s.snapshot().unwrap().payment(pid).unwrap().fee, 10_000);
    s.set_late_fee_waived(lid, 1, true, "Good client", d(2026, 12, 31)).unwrap();
    let snap = s.snapshot().unwrap();
    assert!(snap.late_fees[0].is_waived());
    let p = snap.payment(pid).unwrap();
    assert_eq!(p.fee, 0);
    assert_eq!(p.principal + p.interest, 210_000);
    assert_eq!(snap.changes[0].entity, Entity::LateFee);
    s.set_late_fee_waived(lid, 1, false, "", d(2026, 12, 31)).unwrap();
    assert_eq!(s.snapshot().unwrap().payment(pid).unwrap().fee, 10_000);
}

#[test]
fn weekly_installments_on_a_daily_rate() {
    let today = d(2026, 12, 31);
    let mut s = Store::in_memory();
    let cid = s.create_client(&ClientInput { full_name: "Ana".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
    let t = LoanTerms {
        rate: Rate(10_000),
        interest_period: Frequency::Daily,
        frequency: Frequency::Weekly,
        late_fee_rate: Rate(0),
        ..terms(1_000_000, 4, Method::Declining)
    };
    let lid = s.create_loan(cid, &t, "", today).unwrap();
    let schedule = s.snapshot().unwrap().loan(lid).unwrap().schedule.clone();
    // 1% a day is 7% a week: 700 on 10,000, then on 7,500.
    assert_eq!((schedule[0].interest, schedule[1].interest), (70_000, 52_500));
    assert_eq!(schedule[3].due, d(2026, 1, 29));
    // No late fee when the rate is 0%.
    assert_eq!(s.refresh(d(2026, 3, 1)).unwrap(), 0);
}

#[test]
fn reset_deletes_records_keeps_settings_and_saves_a_copy() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("data").join("portfolio.db")).unwrap();
    let cid = s.create_client(&ClientInput { full_name: "Ana".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 1_000_000, d(2026, 1, 1), "", d(2026, 6, 1)).unwrap();
    s.create_loan(cid, &terms(500_000, 2, Method::Flat), "", d(2026, 6, 1)).unwrap();
    let mut st = s.snapshot().unwrap().settings;
    st.receipt_name = "Préstamos Valdez".into();
    s.save_settings(&st).unwrap();

    let copy = s.reset_all_data().unwrap();
    let snap = s.snapshot().unwrap();
    assert!(snap.clients.is_empty() && snap.loans.is_empty() && snap.capital.is_empty() && snap.payments.is_empty());
    assert_eq!(snap.settings.receipt_name, "Préstamos Valdez");
    let info = Store::inspect_backup(&copy).unwrap();
    assert_eq!((info.clients, info.loans), (1, 1));
}

#[test]
fn receipt_name_and_logo_survive_a_restart_and_a_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("portfolio.db");
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(64, 32, image::Rgba([10, 20, 30, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    {
        let mut s = Store::open(path.clone()).unwrap();
        let mut st = s.snapshot().unwrap().settings;
        st.receipt_name = "Préstamos Valdez".into();
        s.save_settings(&st).unwrap();
        s.set_logo(png.get_ref()).unwrap();
    }
    // Reopened, as after closing the app.
    let mut s = Store::open(path).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.settings.receipt_title(), "Préstamos Valdez");
    assert!(snap.logo.is_some());

    let backup = dir.path().join("copy.db");
    s.backup_to(&backup).unwrap();
    let mut st = snap.settings.clone();
    st.receipt_name = String::new();
    s.save_settings(&st).unwrap();
    s.remove_logo().unwrap();
    assert_eq!(s.snapshot().unwrap().settings.receipt_title(), crate::APP_NAME);

    s.restore_from(&backup).unwrap();
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.settings.receipt_name, "Préstamos Valdez");
    assert!(snap.logo.is_some());
}

#[test]
fn backup_to_an_unwritable_place_fails_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path().join("data").join("portfolio.db")).unwrap();
    let blocker = dir.path().join("a-file");
    std::fs::write(&blocker, b"x").unwrap();
    let err = s.backup_to(&blocker.join("backup.db")).unwrap_err();
    assert!(!format!("{err:#}").is_empty());
}

#[test]
fn installment_loan_migrated_from_version_3() {
    // An interest-only loan made before installments keeps its numbers.
    let (s, _, lid, _) = {
        let today = d(2026, 6, 1);
        let mut s = Store::in_memory();
        let cid = s.create_client(&ClientInput { full_name: "Marta".into(), ..Default::default() }).unwrap();
        s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
        let t = LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1));
        let lid = s.create_loan(cid, &t, "", today).unwrap();
        let pid = s.record_payment(lid, 60_000, 0, d(2026, 2, 1), "", today).unwrap();
        (s, cid, lid, pid)
    };
    let snap = s.snapshot().unwrap();
    let loan = snap.loan(lid).unwrap();
    assert_eq!((loan.terms.method, loan.terms.interest_period), (Method::InterestOnly, Frequency::Monthly));
    assert!(loan.schedule.is_empty());
    assert_eq!(snap.book_loan(lid).unwrap().position(d(2026, 2, 2)).principal_outstanding, 50_000);
}
