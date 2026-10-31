//! Receipts against the records they come from: open loans without any due
//! dates, scheduled loans installment by installment, partial payments,
//! the payment history, the client details chosen in Settings and the
//! Letter page.

use super::receipt::{self, PaymentStanding, ReceiptData, Standing};
use super::*;
use crate::domain::installments::{InstallmentStatus, Instruction};
use crate::domain::loans::{Frequency, LoanTerms, Method};
use crate::domain::money::Rate;
use crate::domain::portfolio::CapitalKind;
use settings::{ReceiptClientFields, ReceiptShow};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

const TODAY: (i32, u32, u32) = (2026, 12, 31);

fn today() -> NaiveDate {
    d(TODAY.0, TODAY.1, TODAY.2)
}

/// A client with every detail, 20,000 of capital, and a loan from Jan 1, 2026.
fn store_with(terms: LoanTerms) -> (Store, i64, i64) {
    let mut s = Store::in_memory();
    let input = ClientInput {
        full_name: "Marta Quiroga".into(),
        phone: "809-555-0101".into(),
        id_number: "001-1234567-8".into(),
        address: "Calle 5 #12".into(),
        ..Default::default()
    };
    let cid = s.create_client(&input).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today()).unwrap();
    let lid = s.create_loan(cid, &terms, "", today()).unwrap();
    (s, cid, lid)
}

/// 1,000 at 10% a month, no schedule.
fn open_loan() -> (Store, i64, i64) {
    store_with(LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1)))
}

/// 10,000 at 10% a month in 5 monthly installments (declining): 3,000,
/// 2,800, 2,600, 2,400 and 2,200, due Feb 1 to Jun 1.
fn scheduled_loan() -> (Store, i64, i64) {
    store_with(LoanTerms {
        principal: 1_000_000,
        rate: Rate(100_000),
        interest_period: Frequency::Monthly,
        frequency: Frequency::Monthly,
        start_date: d(2026, 1, 1),
        method: Method::Declining,
        installments: Some(5),
        late_fee_rate: Rate(50_000),
    })
}

fn pay_open(s: &mut Store, lid: i64, amount: Cents, on: NaiveDate) -> i64 {
    s.record_payment(lid, amount, 0, on, "", today()).unwrap()
}

fn pay_installments(s: &mut Store, lid: i64, amount: Cents, on: NaiveDate, how: Instruction) -> i64 {
    s.record_installment_payment(lid, amount, on, "", &how, on).unwrap()
}

fn receipt(s: &Store, pid: i64) -> (ReceiptData, String) {
    let snap = s.snapshot().unwrap();
    let r = receipt::data(&snap, pid).unwrap();
    let html = receipt::render(&r, &snap);
    (r, html)
}

/// The table rows of `html` that mention `needle`.
fn rows_with<'a>(html: &'a str, needle: &str) -> Vec<&'a str> {
    html.split("<tr").filter(|r| r.contains(needle)).collect()
}

const CHECK: &str = "&#10003;";

#[test]
fn open_loan_receipts_print_no_due_dates() {
    let (mut s, _, lid) = open_loan();
    pay_open(&mut s, lid, 60_000, d(2026, 2, 1));
    let pid = pay_open(&mut s, lid, 5_000, d(2026, 3, 1));
    let (r, html) = receipt(&s, pid);
    assert!(r.schedule.is_empty() && r.next_due.is_none());
    // The only dates are the loan date and the two payments.
    for date in ["Apr 1, 2026", "May 1, 2026", "Jun 1, 2026"] {
        assert!(!html.contains(date), "{date} was printed");
    }
    assert!(!html.contains("Next") && !html.contains("Installments"));
    let loan_html = {
        let snap = s.snapshot().unwrap();
        receipt::render_loan(&receipt::loan_data(&snap, lid).unwrap(), &snap)
    };
    assert!(!loan_html.contains("Feb 1, 2026") && !loan_html.contains("Payment dates"));
}

#[test]
fn open_loan_receipts_show_the_payments_and_what_is_owed() {
    let (mut s, _, lid) = open_loan();
    pay_open(&mut s, lid, 60_000, d(2026, 2, 1)); // 100 interest, 500 capital
    pay_open(&mut s, lid, 5_000, d(2026, 3, 1)); // March's 50 of interest
    let pid = pay_open(&mut s, lid, 3_000, d(2026, 4, 1)); // 30 of April's 50
    let (r, html) = receipt(&s, pid);

    let amounts: Vec<_> = r.history.iter().map(|h| (h.paid_on, h.amount, h.standing)).collect();
    assert_eq!(
        amounts,
        vec![
            (d(2026, 2, 1), 60_000, PaymentStanding::Settled),
            (d(2026, 3, 1), 5_000, PaymentStanding::Settled),
            (d(2026, 4, 1), 3_000, PaymentStanding::Partial(2_000)),
        ]
    );
    assert!(r.history[2].current && !r.history[0].current);
    assert_eq!((r.principal_after, r.interest_after, r.owed_after()), (50_000, 2_000, 52_000));
    // Same figures as the ledger and the portfolio on that day.
    let snap = s.snapshot().unwrap();
    let book = snap.book_loan(lid).unwrap();
    assert_eq!(crate::domain::ledger::owed_at_payment(&book.facts, &book.payments, d(2026, 4, 1)).payoff(), 52_000);
    assert_eq!(snap.book.summary(d(2026, 4, 1)).outstanding_principal, 50_000);
    assert!(html.contains("Partly paid · RD$20.00 interest left"));

    // Paying the rest settles the loan, with no future dates anywhere.
    let last = pay_open(&mut s, lid, 52_000, d(2026, 4, 1));
    let (r, html) = receipt(&s, last);
    assert_eq!(r.balance_after(), 0);
    assert_eq!(r.history.last().unwrap().standing, PaymentStanding::PaidOff);
    assert!(html.contains("This loan is fully paid"));
}

#[test]
fn scheduled_receipts_show_each_installment_as_it_stands() {
    let (mut s, _, lid) = scheduled_loan();
    pay_installments(&mut s, lid, 300_000, d(2026, 2, 1), Instruction::Installment(1));
    let pid = pay_installments(&mut s, lid, 100_000, d(2026, 3, 1), Instruction::Installment(2));
    let (r, html) = receipt(&s, pid);

    let dues: Vec<_> = r.schedule.iter().map(|x| x.due).collect();
    assert_eq!(dues, vec![d(2026, 2, 1), d(2026, 3, 1), d(2026, 4, 1), d(2026, 5, 1), d(2026, 6, 1)], "only the loan's own installments");
    let standings: Vec<_> = r.schedule.iter().map(|x| x.standing).collect();
    assert_eq!(standings, vec![Standing::Paid, Standing::PartlyPaid, Standing::Pending, Standing::Pending, Standing::Pending]);
    assert_eq!((r.schedule[1].amount, r.schedule[1].paid, r.schedule[1].remaining), (280_000, 100_000, 180_000));
    assert_eq!(r.schedule[1].this_payment, 100_000);
    assert_eq!(r.installments_paid, Some((1, 5)));
    // Next: April's installment plus what is left of March's.
    assert_eq!(r.next_due, Some((d(2026, 4, 1), r.schedule[2].remaining + r.schedule[1].remaining)));
    assert_eq!(r.history[1].standing, PaymentStanding::Partial(180_000));

    // The same standings as the loan page's engine on that day.
    let st = s.snapshot().unwrap().book_loan(lid).unwrap().plan_state(d(2026, 3, 1)).unwrap();
    let engine: Vec<_> = st.installments.iter().map(|x| x.status(d(2026, 3, 1))).collect();
    assert_eq!(engine[..2], [InstallmentStatus::Paid, InstallmentStatus::PartlyPaid]);
    assert_eq!(r.left_to_pay, Some(st.installments.iter().map(|x| x.remaining()).sum()));

    // Checks only where something is paid in full.
    let partly = rows_with(&html, "Partly paid");
    assert!(!partly.is_empty() && partly.iter().all(|row| !row.contains(CHECK)));
    let first = rows_with(&html, "<td>1</td><td>Feb 1, 2026</td>")[0];
    assert!(first.contains(CHECK) && first.contains("Paid"));
    assert_eq!(rows_with(&html, "Pending").len(), 3);
}

#[test]
fn late_installments_show_as_overdue() {
    let (mut s, _, lid) = scheduled_loan();
    s.refresh(d(2026, 3, 10)).unwrap();
    // Only part of the first installment and its late fee, on Mar 10.
    let pid = pay_installments(&mut s, lid, 50_000, d(2026, 3, 10), Instruction::Installment(1));
    let (r, html) = receipt(&s, pid);
    assert_eq!(r.schedule[0].standing, Standing::Overdue);
    assert_eq!(r.schedule[1].standing, Standing::Overdue);
    assert!(r.schedule[0].paid > 0 && r.schedule[0].remaining > 0);
    assert!(html.contains("Overdue · partly paid"));
    assert!(r.fees_after > 0 && r.overdue_after > 0);
}

#[test]
fn one_payment_can_settle_several_installments() {
    let (mut s, _, lid) = scheduled_loan();
    s.refresh(d(2026, 3, 1)).unwrap();
    // #1 with its 150 late fee, and all of #2, in one payment.
    // #2 was charged interest on all 10,000: #1 was still unpaid when its period began.
    let pid = pay_installments(&mut s, lid, 615_000, d(2026, 3, 1), Instruction::Spread(vec![(1, 315_000), (2, 300_000)]));
    let (r, html) = receipt(&s, pid);
    let line = r.history.last().unwrap();
    assert_eq!(line.installments.iter().map(|x| (x.seq, x.paid)).collect::<Vec<_>>(), vec![(1, true), (2, true)]);
    assert_eq!(line.standing, PaymentStanding::Settled);
    assert_eq!(r.installments_paid, Some((2, 5)));
    assert!(rows_with(&html, "This receipt")[0].contains(&format!("#1 {}", "<span class=\"check\">&#10003;</span>")));
}

#[test]
fn the_history_is_in_ledger_order_with_reversals_marked_void() {
    let (mut s, _, lid) = scheduled_loan();
    let a = pay_installments(&mut s, lid, 100_000, d(2026, 1, 20), Instruction::Installment(1));
    let b = pay_installments(&mut s, lid, 100_000, d(2026, 1, 20), Instruction::Installment(1));
    let c = pay_installments(&mut s, lid, 100_000, d(2026, 1, 25), Instruction::Installment(1));
    // An old reversed payment is listed but counts nowhere.
    s.conn.execute("UPDATE payments SET reversed_at = 'x', reversal_reason = 'Bounced' WHERE id = ?1", [b]).unwrap();
    let (r, html) = receipt(&s, c);
    let order: Vec<_> = r.history.iter().map(|h| (h.payment_id, h.standing == PaymentStanding::Void)).collect();
    assert_eq!(order, vec![(a, false), (b, true), (c, false)]);
    let void = rows_with(&html, "Void · not counted");
    assert_eq!(void.len(), 1);
    assert!(!void[0].contains(CHECK));
    // Two live payments of 1,000 on #1 (3,000): still partly paid.
    assert_eq!(r.schedule[0].paid, 200_000);
    assert_eq!(r.schedule[0].standing, Standing::PartlyPaid);
    assert!(html.contains("2 payments, oldest first"));
}

#[test]
fn receipts_print_only_the_client_details_chosen() {
    let (mut s, _, lid) = open_loan();
    let pid = pay_open(&mut s, lid, 60_000, d(2026, 2, 1));
    // By default every detail the client has.
    let (_, html) = receipt(&s, pid);
    for v in ["Marta Quiroga", "001-1234567-8", "809-555-0101", "Calle 5 #12"] {
        assert!(html.contains(v), "{v} missing");
    }
    let mut st = s.snapshot().unwrap().settings.clone();
    st.receipt_client = ReceiptClientFields { name: true, id_number: true, phone: false, address: false };
    s.save_settings(&st).unwrap();
    let (_, html) = receipt(&s, pid);
    assert!(html.contains("Marta Quiroga") && html.contains("001-1234567-8"));
    assert!(!html.contains("809-555-0101") && !html.contains("Calle 5"));

    // Without the name, it appears nowhere on the loan receipt either.
    st.receipt_client = ReceiptClientFields { name: false, id_number: true, phone: false, address: false };
    s.save_settings(&st).unwrap();
    let snap = s.snapshot().unwrap();
    let loan_html = receipt::render_loan(&receipt::loan_data(&snap, lid).unwrap(), &snap);
    assert!(!loan_html.contains("Marta") && loan_html.contains("The client received"));
    let (_, html) = receipt(&s, pid);
    assert!(!html.contains("Marta"));
}

#[test]
fn receipts_print_only_the_parts_chosen() {
    let (mut s, _, lid) = scheduled_loan();
    s.refresh(d(2026, 3, 10)).unwrap();
    pay_installments(&mut s, lid, 100_000, d(2026, 2, 1), Instruction::Installment(1));
    let pid = pay_installments(&mut s, lid, 210_000, d(2026, 3, 10), Instruction::Installment(1));
    // Every part by default.
    let (r, html) = receipt(&s, pid);
    assert!(r.fee > 0, "the test needs a late fee on this payment");
    for part in [
        "<td>Loan</td>",
        "<td>Type</td>",
        "Interest rate",
        "Amount lent",
        ">Late fees<",
        ">Interest<",
        "Balance after this payment",
        "Payment history",
        "Installments",
        "Loan #",
    ] {
        assert!(html.contains(part), "{part} missing by default");
    }

    // Everything optional switched off: only what a receipt can't do without.
    let mut st = s.snapshot().unwrap().settings.clone();
    st.receipt_show = ReceiptShow::from_values([false; 9]);
    s.save_settings(&st).unwrap();
    let (_, html) = receipt(&s, pid);
    for part in [
        "<td>Loan</td>",
        "<td>Type</td>",
        "Interest rate",
        "Amount lent",
        ">Late fees<",
        ">Interest<",
        "Balance after this payment",
        "Payment history",
        "Installments",
        "Loan #",
        "Next installment",
    ] {
        assert!(!html.contains(part), "{part} still printed");
    }
    for kept in ["Amount paid", "Date paid", "Loan date", &format!("Receipt {pid:06}")] {
        assert!(html.contains(kept), "{kept} missing");
    }

    // One part back on brings only that part back.
    st.receipt_show = ReceiptShow::from_values([false; 9]).with(2, true);
    s.save_settings(&st).unwrap();
    let (_, html) = receipt(&s, pid);
    assert!(html.contains("Interest rate") && !html.contains("Amount lent"));

    // The loan receipt is the client's agreement: it keeps every term.
    let snap = s.snapshot().unwrap();
    let loan_html = receipt::render_loan(&receipt::loan_data(&snap, lid).unwrap(), &snap);
    assert!(loan_html.contains("Interest rate"));
}

#[test]
fn receipt_parts_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("portfolio.db");
    let chosen = ReceiptShow::default().with(0, false).with(8, false);
    {
        let mut s = Store::open(path.clone()).unwrap();
        assert_eq!(s.snapshot().unwrap().settings.receipt_show, ReceiptShow::default());
        let mut st = s.snapshot().unwrap().settings.clone();
        st.receipt_show = chosen;
        s.save_settings(&st).unwrap();
    }
    assert_eq!(Store::open(path).unwrap().snapshot().unwrap().settings.receipt_show, chosen);
}

#[test]
fn missing_details_are_left_out_cleanly() {
    let mut s = Store::in_memory();
    let cid = s.create_client(&ClientInput { full_name: "Ana Ruiz".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today()).unwrap();
    let lid =
        s.create_loan(cid, &LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1)), "", today()).unwrap();
    let pid = pay_open(&mut s, lid, 10_000, d(2026, 2, 1));
    let (r, html) = receipt(&s, pid);
    assert_eq!((r.client.id_number.as_ref(), r.client.phone.as_ref(), r.client.address.as_ref()), (None, None, None));
    assert!(!html.contains(">ID number<") && !html.contains(">Phone<") && !html.contains(">Address<"));
    assert!(html.contains("Ana Ruiz"));
}

#[test]
fn receipt_settings_survive_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("portfolio.db");
    let chosen = ReceiptClientFields { name: true, id_number: true, phone: false, address: false };
    {
        let mut s = Store::open(path.clone()).unwrap();
        assert_eq!(s.snapshot().unwrap().settings.receipt_client, ReceiptClientFields::default());
        let mut st = s.snapshot().unwrap().settings.clone();
        st.receipt_client = chosen;
        s.save_settings(&st).unwrap();
    }
    assert_eq!(Store::open(path).unwrap().snapshot().unwrap().settings.receipt_client, chosen);
}

#[test]
fn receipts_are_letter_pages_with_small_payment_rules() {
    let (mut s, _, lid) = scheduled_loan();
    let pid = pay_installments(&mut s, lid, 300_000, d(2026, 2, 1), Instruction::Installment(1));
    let (_, html) = receipt(&s, pid);
    let snap = s.snapshot().unwrap();
    let loan_html = receipt::render_loan(&receipt::loan_data(&snap, lid).unwrap(), &snap);
    for page in [&html, &loan_html] {
        assert!(page.contains("size: letter") && page.contains("counter(pages)"));
        assert!(page.contains("width: 8.5in"));
    }
    assert!(loan_html.contains("How payments work") && loan_html.contains(".rules { margin-top:10pt; font-size:7.5pt"));
}

#[test]
fn a_long_history_lists_every_payment_and_says_it_continues() {
    let mut s = Store::in_memory();
    let cid = s.create_client(&ClientInput { full_name: "Ana Ruiz".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2023, 1, 1), "", today()).unwrap();
    let lid =
        s.create_loan(cid, &LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2023, 1, 1)), "", today()).unwrap();
    let mut last = 0;
    for k in 1..=40 {
        last = pay_open(&mut s, lid, 10_000, Frequency::Monthly.due_date(d(2023, 1, 1), k));
    }
    let (r, html) = receipt(&s, last);
    assert_eq!(r.history.len(), 40);
    assert_eq!(rows_with(&html, "Interest paid to date").len(), 40);
    assert!(html.contains("continues on the next page. Every payment is listed."));
    // A short one says nothing of the kind.
    let (_, short) = receipt(&s, s.snapshot().unwrap().payments.iter().map(|p| p.id).min().unwrap());
    assert!(!short.contains("continues on the next page"));
}
