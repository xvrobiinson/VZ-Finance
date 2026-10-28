//! Version 0.6: income and expenses from New record, company details and
//! custom wording on receipts, and where receipts put the signatures.

use super::receipt;
use super::receipt_text::{ReceiptText, RuleLine, RuleSet, When};
use super::*;
use crate::domain::loans::{Frequency, LoanTerms, Method};
use crate::domain::money::Rate;
use crate::domain::portfolio::{Account, CapitalKind, EntryKind};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn today() -> NaiveDate {
    d(2026, 12, 31)
}

fn invalid_text(e: anyhow::Error) -> String {
    e.downcast_ref::<Invalid>().map(|i| i.0.clone()).unwrap_or_else(|| format!("not a validation error: {e:#}"))
}

// ----- income and expenses ------------------------------------------------

#[test]
fn income_needs_no_balance_and_counts_by_account() {
    let mut s = Store::in_memory();
    // Nothing in either account yet: income still records.
    s.add_income(Account::Interest, 500_000, d(2026, 3, 5), "Rent from the shop", today()).unwrap();
    s.add_income(Account::Capital, 2_000_000, d(2026, 3, 6), "Sold the old car", today()).unwrap();
    s.add_expense(Account::Interest, 100_000, d(2026, 3, 7), "Printer ink", today()).unwrap();
    let snap = s.snapshot().unwrap();
    let sum = snap.book.summary(today());
    assert_eq!((sum.available_interest, sum.available_capital), (400_000, 2_000_000));
    assert_eq!((sum.other_income, sum.capital_income), (500_000, 2_000_000));
    // Into capital it is capital invested; into interest it is income.
    assert_eq!(sum.net_invested(), 2_000_000);
    assert_eq!(sum.total_income(), 500_000);
    assert_eq!(sum.gain(), 400_000, "income 5,000 less the 1,000 expense");
    assert_eq!(sum.portfolio_value, 2_400_000);
    // Counted once: cash equals what came in less what went out.
    assert_eq!(sum.available_cash, 2_500_000 - 100_000);
    let march = snap.book.monthly_cash_flow(today()).into_iter().find(|f| f.month == d(2026, 3, 1)).unwrap();
    assert_eq!((march.income, march.expenses, march.net()), (2_500_000, 100_000, 2_400_000));
    let kinds: Vec<_> = snap.entries.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EntryKind::Income, EntryKind::Income, EntryKind::Expense]);
}

#[test]
fn income_and_expenses_need_an_amount_and_a_reason() {
    let mut s = Store::in_memory();
    s.add_capital(CapitalKind::Contribution, 100_000, d(2026, 1, 1), "", today()).unwrap();
    let on = d(2026, 2, 1);
    assert!(invalid_text(s.add_income(Account::Interest, 0, on, "Rent", today()).unwrap_err()).contains("greater than zero"));
    assert!(invalid_text(s.add_income(Account::Interest, -500, on, "Rent", today()).unwrap_err()).contains("greater than zero"));
    assert!(invalid_text(s.add_income(Account::Interest, 500, on, "   ", today()).unwrap_err()).contains("where the income came from"));
    assert!(invalid_text(s.add_expense(Account::Capital, 500, on, "", today()).unwrap_err()).contains("what the expense was for"));
    assert!(s.add_income(Account::Interest, 500, d(2027, 1, 1), "Rent", today()).is_err(), "no future dates");
    // An expense can't spend money that isn't there.
    assert!(s.add_expense(Account::Capital, 200_000, on, "Too much", today()).is_err());
    assert!(s.snapshot().unwrap().entries.is_empty(), "nothing refused was saved");
    // Editing can't empty the reason either.
    let id = s.add_income(Account::Interest, 500, on, "Rent", today()).unwrap();
    assert!(s.update_entry(id, Account::Interest, 500, on, " ", "", today()).is_err());
}

#[test]
fn the_same_record_submitted_twice_is_saved_once() {
    let mut s = Store::in_memory();
    let a = s.add_income(Account::Interest, 75_000, d(2026, 4, 1), "Consulting", today()).unwrap();
    let b = s.add_income(Account::Interest, 75_000, d(2026, 4, 1), "Consulting", today()).unwrap();
    assert_eq!(a, b);
    // A different record a moment later is a new one.
    s.add_income(Account::Interest, 75_000, d(2026, 4, 1), "Consulting, second job", today()).unwrap();
    assert_eq!(s.snapshot().unwrap().entries.len(), 2);
}

#[test]
fn spent_income_cannot_be_deleted_out_from_under_the_expense() {
    let mut s = Store::in_memory();
    let income = s.add_income(Account::Interest, 50_000, d(2026, 5, 1), "Rent", today()).unwrap();
    s.add_expense(Account::Interest, 40_000, d(2026, 5, 2), "Repair", today()).unwrap();
    assert!(s.delete_entry(income, "").is_err());
    assert_eq!(s.snapshot().unwrap().entries.len(), 2);
}

#[test]
fn income_is_exported_and_kept_through_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("data").join("portfolio.db")).unwrap();
    s.add_income(Account::Interest, 12_345, d(2026, 6, 1), "Bank interest", today()).unwrap();
    let files = s.export_csv(&dir.path().join("csv")).unwrap();
    let entries = files.iter().find(|f| f.to_string_lossy().contains("cash-entries")).unwrap();
    let csv = std::fs::read_to_string(entries).unwrap();
    assert!(csv.contains("income,interest,123.45,2026-06-01,Bank interest"), "{csv}");
    let copy = dir.path().join("copy.db");
    s.backup_to(&copy).unwrap();
    s.add_income(Account::Interest, 1, d(2026, 6, 2), "later", today()).unwrap();
    s.restore_from(&copy).unwrap();
    let entries = s.snapshot().unwrap().entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, EntryKind::Income);
}

// ----- receipts -----------------------------------------------------------

fn loan_store(method: Method) -> (Store, i64) {
    let mut s = Store::in_memory();
    let cid =
        s.create_client(&ClientInput { full_name: "Marta Quiroga".into(), phone: "809-555-0101".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today()).unwrap();
    let terms = match method {
        Method::InterestOnly => LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1)),
        _ => LoanTerms {
            principal: 1_000_000,
            rate: Rate(100_000),
            interest_period: Frequency::Monthly,
            frequency: Frequency::Monthly,
            start_date: d(2026, 1, 1),
            method,
            installments: Some(5),
            late_fee_rate: Rate(50_000),
        },
    };
    let lid = s.create_loan(cid, &terms, "", today()).unwrap();
    (s, lid)
}

fn loan_html(s: &Store, lid: i64) -> String {
    let snap = s.snapshot().unwrap();
    receipt::render_loan(&receipt::loan_data(&snap, lid).unwrap(), &snap)
}

fn payment_html(s: &mut Store, lid: i64) -> String {
    let pid = s.record_payment(lid, 10_000, 0, d(2026, 2, 1), "", today()).unwrap();
    let snap = s.snapshot().unwrap();
    receipt::render(&receipt::data(&snap, pid).unwrap(), &snap)
}

/// Text a receipt shows, without markup.
fn shown(html: &str) -> String {
    let body = &html[html.find("<body>").unwrap()..];
    let mut out = String::new();
    let mut tag = false;
    for c in body.chars() {
        match c {
            '<' => tag = true,
            '>' => {
                tag = false;
                out.push(' ');
            }
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_built_in_wording_is_unchanged() {
    let (s, lid) = loan_store(Method::Declining);
    let text = shown(&loan_html(&s, lid));
    for line in [
        "How payments work",
        "An installment not fully paid by its due date is charged a late fee of 5% of what is still unpaid, once, the day after.",
        "Payments cover late fees first, then interest, then capital. The loan can be paid off early with the capital owed plus the interest due.",
        "Client: Marta Quiroga",
        "For Acervus Finance",
    ] {
        assert!(text.contains(line), "missing “{line}” in {text}");
    }
    let (s, lid) = loan_store(Method::InterestOnly);
    let text = shown(&loan_html(&s, lid));
    for line in [
        "Paying RD$100.00 each time covers the interest; the capital stays at RD$1,000.00.",
        "Anything paid above the interest lowers the capital. For example, paying RD$600.00 is RD$100.00 of interest and RD$500.00 of capital, and the next interest drops to RD$50.00.",
        "The loan can be paid off at any time with the capital owed plus the interest due. It closes when the capital reaches zero.",
    ] {
        assert!(text.contains(line), "missing “{line}”");
    }
    let (mut s, lid) = loan_store(Method::InterestOnly);
    assert!(shown(&payment_html(&mut s, lid)).contains("Thank you for your payment."));
    assert!(s.snapshot().unwrap().settings.receipt_text.is_default());
}

#[test]
fn custom_wording_is_printed_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("portfolio.db");
    let lid = {
        let mut s = Store::open(path.clone()).unwrap();
        let cid = s.create_client(&ClientInput { full_name: "Ana Ruiz".into(), ..Default::default() }).unwrap();
        s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today()).unwrap();
        let terms = LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1));
        let lid = s.create_loan(cid, &terms, "", today()).unwrap();
        let mut st = s.snapshot().unwrap().settings;
        st.receipt_name = "Préstamos Valdez".into();
        st.receipt_text = ReceiptText {
            heading: Some("Our terms".into()),
            client_sign: Some("Borrower".into()),
            lender_sign: Some("Signed for {company}".into()),
            payment_footer: Some("See you next month.".into()),
            loan_footer: Some("Questions? Call {company}.".into()),
            ..Default::default()
        };
        // Reordered, one removed, one added; amounts still filled in.
        st.receipt_text.set_rules(
            RuleSet::InterestOnly,
            Some(vec![
                RuleLine { text: "Interest each month: {interest}.".into(), when: When::Always },
                RuleLine { text: "Rate note: {conversion}".into(), when: When::Converted },
                RuleLine { text: "Pay at the office.".into(), when: When::Always },
            ]),
        );
        s.save_settings(&st).unwrap();
        lid
    };
    let mut s = Store::open(path).unwrap();
    let text = shown(&loan_html(&s, lid));
    assert!(text.contains("Our terms"));
    assert!(text.contains("Interest each month: RD$100.00."));
    assert!(text.contains("Pay at the office."));
    assert!(!text.contains("Rate note"), "the rate isn't converted, so that line is skipped");
    assert!(!text.contains("How payments work") && !text.contains("No fixed term"));
    assert!(text.contains("Borrower: Ana Ruiz") && text.contains("Signed for Préstamos Valdez"));
    assert!(text.contains("Questions? Call Préstamos Valdez."));
    // The order is the one saved.
    assert!(text.find("Interest each month").unwrap() < text.find("Pay at the office").unwrap());
    assert!(shown(&payment_html(&mut s, lid)).contains("See you next month."));
    // Other loan types keep the built-in lines.
    assert!(s.snapshot().unwrap().settings.receipt_text.rules(RuleSet::Declining) == receipt_text::default_rules(RuleSet::Declining));

    // Restoring the defaults brings every original line back.
    let mut st = s.snapshot().unwrap().settings;
    st.receipt_text = ReceiptText::default();
    s.save_settings(&st).unwrap();
    let text = shown(&loan_html(&s, lid));
    assert!(text.contains("How payments work") && text.contains("Client: Ana Ruiz") && !text.contains("Our terms"));
}

#[test]
fn wording_never_changes_the_numbers() {
    let (mut s, lid) = loan_store(Method::Declining);
    let before = s.snapshot().unwrap();
    let mut st = before.settings.clone();
    st.receipt_text.set_rules(RuleSet::Declining, Some(Vec::new()));
    st.receipt_text.heading = Some(String::new());
    s.save_settings(&st).unwrap();
    let after = s.snapshot().unwrap();
    assert_eq!(after.loan(lid).unwrap().schedule, before.loan(lid).unwrap().schedule);
    assert_eq!(after.book.summary(today()), before.book.summary(today()));
    // No lines: no "How payments work" section at all.
    let html = loan_html(&s, lid);
    assert!(!html.contains("class=\"rules\""));
}

#[test]
fn company_details_print_only_when_ticked_and_filled() {
    let (mut s, lid) = loan_store(Method::InterestOnly);
    let header = |s: &Store| {
        let html = loan_html(s, lid);
        let start = html.find("<div class=\"top\">").unwrap();
        let end = start + html[start..].find("<div class=\"doc\">").unwrap();
        shown(&format!("<body>{}", &html[start..end]))
    };
    // Defaults: the name only, exactly as before.
    assert_eq!(header(&s), "Acervus Finance Loan receipt · No. 000001");
    let mut st = s.snapshot().unwrap().settings;
    st.receipt_name = "Préstamos Valdez".into();
    st.company_address = "Calle Duarte 45, Santiago".into();
    st.company_phone = "809-555-0100".into();
    s.save_settings(&st).unwrap();
    assert_eq!(header(&s), "Préstamos Valdez Calle Duarte 45, Santiago · 809-555-0100 Loan receipt · No. 000001");
    st.receipt_company.address = false;
    s.save_settings(&st).unwrap();
    assert_eq!(header(&s), "Préstamos Valdez 809-555-0100 Loan receipt · No. 000001");
    st.receipt_company.name = false;
    st.receipt_company.address = true;
    st.company_phone = String::new();
    s.save_settings(&st).unwrap();
    let html = loan_html(&s, lid);
    assert_eq!(header(&s), "Calle Duarte 45, Santiago Loan receipt · No. 000001");
    assert!(!html.contains("<h1>"), "no empty name heading");
    assert!(shown(&html).contains("For the lender"));
    // Client details stay their own setting.
    st.receipt_client.name = false;
    s.save_settings(&st).unwrap();
    let text = shown(&loan_html(&s, lid));
    assert!(text.contains("The client received") && !text.contains("Marta Quiroga"));
    assert!(s.snapshot().unwrap().settings.receipt_company.address);
}

#[test]
fn company_details_and_wording_are_checked_before_saving() {
    let mut s = Store::in_memory();
    let mut st = s.snapshot().unwrap().settings;
    st.company_phone = "1".repeat(41);
    assert!(s.save_settings(&st).is_err());
    st.company_phone = "809\n555".into();
    assert!(s.save_settings(&st).is_err());
    st.company_phone = "809-555-0100".into();
    st.receipt_text.set_rules(RuleSet::Flat, Some(vec![RuleLine { text: "x".repeat(501), when: When::Always }; 1]));
    assert!(s.save_settings(&st).is_err());
    st.receipt_text.set_rules(RuleSet::Flat, Some(vec![RuleLine { text: "ok".into(), when: When::Always }; 21]));
    assert!(s.save_settings(&st).is_err());
    assert_eq!(s.snapshot().unwrap().settings.company_phone, "", "nothing refused was saved");
}

#[test]
fn signatures_sit_at_the_foot_of_a_letter_page_and_long_text_is_kept() {
    let (mut s, lid) = loan_store(Method::Declining);
    let html = loan_html(&s, lid);
    assert!(html.contains("size: letter"));
    assert!(html.contains("<div class=\"sheet loan\">"));
    assert!(html.contains(".sheet.loan { min-height:9.9in; }"));
    assert!(html.contains("<div class=\"foot\"><div class=\"sign\">"));
    assert!(!html.contains("class=\"note\""), "a short receipt fits one page");

    // Twenty long lines: every word printed, and the receipt says it goes on.
    let long = "Payments are due at the office before noon on the due date. ".repeat(7);
    let mut st = s.snapshot().unwrap().settings;
    let lines: Vec<RuleLine> = (0..20).map(|i| RuleLine { text: format!("{i}: {long}"), when: When::Always }).collect();
    st.receipt_text.set_rules(RuleSet::Declining, Some(lines));
    s.save_settings(&st).unwrap();
    let text = shown(&loan_html(&s, lid));
    for i in 0..20 {
        assert!(text.contains(&format!("{i}: Payments are due")), "line {i} missing");
    }
    assert!(text.contains("This receipt continues on the next page; the signatures are at the end."));
}
