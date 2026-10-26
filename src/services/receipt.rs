//! Printable receipts: one for each payment and one for a loan when the
//! client receives it. A receipt is a self-contained HTML page laid out on
//! US Letter paper, written to the receipts folder and opened in the default
//! browser, which shows the print dialog. It works offline. The name at the
//! top is the receipt name from Settings, and Settings chooses which client
//! details are printed. Client photos are never printed.
//!
//! Everything on a receipt comes from the same engine as the loan page:
//! installment statuses from the stored per-installment splits, open-loan
//! balances from the ledger. Open (interest-only) loans have no schedule, so
//! their receipts show no due dates at all.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow};
use chrono::NaiveDate;

use super::Snapshot;
use super::settings::ReceiptClientFields;
use crate::database::repositories::clients::Client;
use crate::database::repositories::payments::Payment;
use crate::domain::installments::{self, Applied, Installment, InstallmentStatus, Instruction, PlanPayment, PlanState};
use crate::domain::ledger::owed_at_payment;
use crate::domain::loans::{Frequency, Method};
use crate::domain::money::{Cents, MoneyFormat, Rate, percent_label};
use crate::i18n::{self, t};
use crate::tr;
use crate::utils::dates::DateStyle;

/// Table rows that fit on one Letter page under the rest of a payment
/// receipt, measured by printing receipts in Edge; past this the receipt says
/// it continues on the next page. A schedule table costs two more rows (its
/// title and column names).
const PAYMENT_PAGE_ROWS: usize = 26;
/// The same for a loan receipt, which also carries the rules and signatures.
const LOAN_PAGE_ROWS: usize = 20;

/// Client details printed on a receipt: only those ticked in Settings that
/// the client has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientLines {
    pub name: Option<String>,
    pub id_number: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
}

impl ClientLines {
    pub fn new(client: &Client, fields: ReceiptClientFields) -> Self {
        let keep = |on: bool, v: Option<&str>| v.map(str::trim).filter(|v| on && !v.is_empty()).map(str::to_string);
        Self {
            name: keep(fields.name, Some(&client.full_name)),
            id_number: keep(fields.id_number, client.id_number.as_deref()),
            phone: keep(fields.phone, client.phone.as_deref()),
            address: keep(fields.address, client.address.as_deref()),
        }
    }

    /// (label, value) for each detail printed, in order.
    fn rows(&self) -> Vec<(&'static str, &str)> {
        [
            (t("Name", "Nombre"), &self.name),
            (t("ID number", "Identificación"), &self.id_number),
            (t("Phone", "Teléfono"), &self.phone),
            (t("Address", "Dirección"), &self.address),
        ]
        .into_iter()
        .filter_map(|(label, v)| v.as_deref().map(|v| (label, v)))
        .collect()
    }
}

/// What one payment did to one installment, and how it stood right after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallmentLine {
    pub seq: u32,
    pub due: NaiveDate,
    pub applied: Applied,
    /// Still owed on it right after the payment.
    pub remaining: Cents,
    pub paid: bool,
}

/// Where an installment stands right after the payment on the receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Paid,
    PartlyPaid,
    /// Past its due date with something unpaid (partly paid or not).
    Overdue,
    /// Not due yet, nothing paid.
    Pending,
}

/// One installment of the schedule as it stood right after the payment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleLine {
    pub seq: u32,
    pub due: NaiveDate,
    /// Capital, interest and any late fee.
    pub amount: Cents,
    pub paid: Cents,
    pub remaining: Cents,
    pub standing: Standing,
    /// What the payment on the receipt put on it.
    pub this_payment: Cents,
}

/// How a payment in the history left what it paid for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentStanding {
    /// Installment loans: every installment it went to is paid in full.
    /// Open loans: all interest charged by then is paid.
    Settled,
    /// Something it went to still had this much left.
    Partial(Cents),
    /// Nothing was owed after it.
    PaidOff,
    /// Reversed: listed for the record and counted nowhere.
    Void,
}

/// One payment in the loan's history, as it stood right after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryLine {
    pub payment_id: i64,
    pub paid_on: NaiveDate,
    pub amount: Cents,
    pub principal: Cents,
    pub interest: Cents,
    pub fee: Cents,
    /// The payment this receipt is for.
    pub current: bool,
    pub standing: PaymentStanding,
    /// Installment loans: what it did to each installment.
    pub installments: Vec<InstallmentLine>,
    /// Capital still owed right after it.
    pub capital_left: Cents,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptData {
    pub payment_id: i64,
    pub business: String,
    pub client: ClientLines,
    pub loan_id: i64,
    pub method: Method,
    /// "10% a month · paid weekly".
    pub rate_line: String,
    pub start_date: NaiveDate,
    pub paid_on: NaiveDate,
    pub amount: Cents,
    pub interest: Cents,
    pub principal: Cents,
    pub fee: Cents,
    pub original_principal: Cents,
    pub principal_before: Cents,
    pub interest_before: Cents,
    pub principal_after: Cents,
    pub interest_after: Cents,
    /// Late fees still unpaid right after the payment.
    pub fees_after: Cents,
    /// Installments due by the payment date still unpaid after it.
    pub overdue_after: Cents,
    /// Installment loans: the next scheduled due date still unpaid, and
    /// what is owed by then. Open loans have none.
    pub next_due: Option<(NaiveDate, Cents)>,
    pub notes: String,
    pub reversed: Option<String>,
    /// Installment loans: where this payment went.
    pub installments: Vec<InstallmentLine>,
    /// Installment loans: the whole schedule after this payment.
    pub schedule: Vec<ScheduleLine>,
    /// Installment loans: (paid in full, total installments).
    pub installments_paid: Option<(usize, usize)>,
    /// Installment loans: everything left to pay as scheduled.
    pub left_to_pay: Option<Cents>,
    /// Every payment on the loan up to and including this one, oldest
    /// first; reversed ones are marked void.
    pub history: Vec<HistoryLine>,
}

impl ReceiptData {
    pub fn owed_before(&self) -> Cents {
        self.principal_before + self.interest_before
    }
    pub fn owed_after(&self) -> Cents {
        self.principal_after + self.interest_after + self.fees_after
    }
    /// Everything still to pay after this payment.
    pub fn balance_after(&self) -> Cents {
        self.left_to_pay.unwrap_or_else(|| self.owed_after())
    }
}

fn ledger_key(p: &Payment) -> (NaiveDate, i64) {
    (p.paid_on, p.id)
}

/// What `applied` did to each installment, as `state` stands after it.
fn installment_lines(applied: &[Applied], state: &PlanState, on: NaiveDate) -> Vec<InstallmentLine> {
    applied
        .iter()
        .filter_map(|a| {
            let x = state.installments.iter().find(|x| x.seq == a.seq)?;
            Some(InstallmentLine {
                seq: a.seq,
                due: x.due,
                applied: *a,
                remaining: x.remaining(),
                paid: x.status(on) == InstallmentStatus::Paid,
            })
        })
        .collect()
}

/// Everything shown on the receipt, computed from the ledger as it stood
/// right after this payment.
pub fn data(snap: &Snapshot, payment_id: i64) -> Option<ReceiptData> {
    let payment = snap.payment(payment_id)?;
    let loan = snap.loan(payment.loan_id)?;
    let book = snap.book_loan(loan.id)?;
    let client = snap.client(loan.client_id)?;
    let upto = ledger_key(payment);

    // Payments that count, up to and including this one, in ledger order. A
    // reversed payment's own receipt shows it as it was when recorded.
    let mut live: Vec<&Payment> =
        snap.payments_for_loan(loan.id).filter(|p| (!p.is_reversed() || p.id == payment_id) && ledger_key(p) <= upto).collect();
    live.sort_by_key(|p| ledger_key(p));

    let mut d = ReceiptData {
        payment_id,
        business: snap.settings.receipt_title(),
        client: ClientLines::new(client, snap.settings.receipt_client),
        loan_id: loan.id,
        method: loan.terms.method,
        rate_line: loan.terms.rate_line(),
        start_date: loan.terms.start_date,
        paid_on: payment.paid_on,
        amount: payment.amount,
        interest: payment.interest,
        principal: payment.principal,
        fee: payment.fee,
        original_principal: loan.terms.principal,
        principal_before: 0,
        interest_before: 0,
        principal_after: 0,
        interest_after: 0,
        fees_after: 0,
        overdue_after: 0,
        next_due: None,
        notes: payment.notes.clone(),
        reversed: payment.reversal_reason.clone().filter(|_| payment.is_reversed()),
        installments: Vec::new(),
        schedule: Vec::new(),
        installments_paid: None,
        left_to_pay: None,
        history: Vec::new(),
    };
    let line = |p: &Payment, standing, installments, capital_left| HistoryLine {
        payment_id: p.id,
        paid_on: p.paid_on,
        amount: p.amount,
        principal: p.principal,
        interest: p.interest,
        fee: p.fee,
        current: p.id == payment_id,
        standing,
        installments,
        capital_left,
    };

    if let Some(plan) = &book.plan {
        // Each payment's lines come from the loan as it stood right after it.
        let mut pays = Vec::with_capacity(live.len());
        let mut after = None;
        for p in &live {
            pays.push(PlanPayment { paid_on: p.paid_on, amount: p.amount, instruction: Instruction::Fixed(p.applied.clone()) });
            let state = installments::run(&book.facts, plan, &pays, p.paid_on, NaiveDate::MAX).ok()?;
            let lines = installment_lines(&p.applied, &state, p.paid_on);
            let left: Cents = lines.iter().filter(|x| !x.paid).map(|x| x.remaining).sum();
            let standing = if state.paid_off() {
                PaymentStanding::PaidOff
            } else if left > 0 {
                PaymentStanding::Partial(left)
            } else {
                PaymentStanding::Settled
            };
            d.history.push(line(p, standing, lines, state.principal_outstanding()));
            after = Some(state);
        }
        let state = after?;
        let on = payment.paid_on;
        d.principal_after = state.principal_outstanding();
        d.principal_before = d.principal_after + payment.principal;
        // Owed now: installments due by the payment date (later ones are not due yet).
        let due_now: Vec<_> = state.installments.iter().filter(|x| x.due <= on).collect();
        d.interest_after = due_now.iter().map(|x| x.unpaid_interest()).sum();
        d.fees_after = state.installments.iter().map(|x| x.unpaid_fee()).sum();
        d.overdue_after = due_now.iter().map(|x| x.owed()).sum();
        d.interest_before = d.interest_after + payment.interest;
        d.installments = installment_lines(&payment.applied, &state, on);
        d.schedule = state
            .installments
            .iter()
            .map(|x| ScheduleLine {
                seq: x.seq,
                due: x.due,
                amount: x.total_due(),
                paid: x.paid(),
                remaining: x.remaining(),
                standing: match x.status(on) {
                    InstallmentStatus::Paid => Standing::Paid,
                    InstallmentStatus::Overdue => Standing::Overdue,
                    InstallmentStatus::PartlyPaid => Standing::PartlyPaid,
                    InstallmentStatus::Due | InstallmentStatus::Upcoming => Standing::Pending,
                },
                this_payment: payment.applied.iter().filter(|a| a.seq == x.seq).map(Applied::total).sum(),
            })
            .collect();
        let paid = state.installments.iter().filter(|x| x.remaining() <= 0).count();
        d.installments_paid = Some((paid, state.installments.len()));
        d.left_to_pay = Some(state.installments.iter().map(|x| x.remaining()).sum());
        d.next_due = state.installments.iter().find(|x| x.due > on && x.remaining() > 0).map(|x| (x.due, x.remaining() + d.overdue_after));
    } else {
        let mut facts = Vec::with_capacity(live.len());
        let mut after = None;
        for p in &live {
            facts.push(p.fact());
            let pos = owed_at_payment(&book.facts, &facts, p.paid_on);
            let standing = if pos.payoff() <= 0 {
                PaymentStanding::PaidOff
            } else if pos.interest_outstanding > 0 {
                PaymentStanding::Partial(pos.interest_outstanding)
            } else {
                PaymentStanding::Settled
            };
            d.history.push(line(p, standing, Vec::new(), pos.principal_outstanding));
            after = Some(pos);
        }
        let after = after?;
        d.principal_before = after.principal_outstanding + payment.principal;
        d.interest_before = after.interest_outstanding + payment.interest;
        d.principal_after = after.principal_outstanding;
        d.interest_after = after.interest_outstanding;
    }

    // Reversed payments are listed where they happened and counted nowhere.
    let voided = snap.payments_for_loan(loan.id).filter(|p| p.is_reversed() && p.id != payment_id && ledger_key(p) <= upto);
    d.history.extend(voided.map(|p| line(p, PaymentStanding::Void, Vec::new(), 0)));
    d.history.sort_by_key(|l| (l.paid_on, l.payment_id));
    Some(d)
}

/// What a client agreed to when taking a loan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoanReceipt {
    pub loan_id: i64,
    pub business: String,
    pub client: ClientLines,
    pub start_date: NaiveDate,
    pub principal: Cents,
    pub method: Method,
    pub rate_line: String,
    /// "10% a month = 2.3333% a week (7 of 30 days)" when they differ.
    pub conversion: Option<String>,
    pub frequency: Frequency,
    pub late_fee_rate: Rate,
    /// Interest each period while the capital is unchanged (interest only).
    pub interest_per_period: Cents,
    /// Interest only: the interest after half the capital is repaid, for
    /// the worked example.
    pub interest_after_half: Cents,
    /// Installment loans: the full schedule. Open loans have none.
    pub schedule: Vec<Installment>,
    pub schedule_edited: bool,
    pub notes: String,
}

impl LoanReceipt {
    pub fn total_interest(&self) -> Cents {
        self.schedule.iter().map(|x| x.interest).sum()
    }
}

pub fn loan_data(snap: &Snapshot, loan_id: i64) -> Option<LoanReceipt> {
    let loan = snap.loan(loan_id)?;
    let client = snap.client(loan.client_id)?;
    let terms = &loan.terms;
    Some(LoanReceipt {
        loan_id,
        business: snap.settings.receipt_title(),
        client: ClientLines::new(client, snap.settings.receipt_client),
        start_date: terms.start_date,
        principal: terms.principal,
        method: terms.method,
        rate_line: terms.rate_line(),
        conversion: terms.period_rate().conversion_note(),
        frequency: terms.frequency,
        late_fee_rate: terms.late_fee_rate,
        interest_per_period: terms.interest_per_period(),
        interest_after_half: terms.period_rate().interest(terms.principal - terms.principal / 2),
        schedule: if terms.method.has_installments() { loan.schedule.clone() } else { Vec::new() },
        schedule_edited: loan.schedule_edited,
        notes: loan.notes.clone(),
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The business logo as an inline image, or nothing.
fn logo_html(snap: &Snapshot) -> String {
    snap.logo
        .as_ref()
        .map(|png| {
            use base64::Engine as _;
            let b64 = base64::engine::general_purpose::STANDARD.encode(png);
            format!("<img class=\"logo\" src=\"data:image/png;base64,{b64}\" alt=\"\">")
        })
        .unwrap_or_default()
}

/// Styles shared by every receipt. Sizes are in points so the screen
/// preview and the printed page match; the sheet on screen is a Letter page.
const BASE_CSS: &str = r#"
  :root { --ink:#17191c; --muted:#5f6570; --rule:#dfe2e7; --soft:#f4f5f7; --accent:#365cf5; --pos:#137a60; --pos-bg:#e7f5f0; --warn:#9a5f0e; --warn-bg:#fbf1e1; --neg:#c2353d; --neg-bg:#fdecec; }
  * { box-sizing: border-box; }
  html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }
  body { margin:0; background:#e9ebef; color:var(--ink); font: 9.5pt/1.38 "Segoe UI", "Helvetica Neue", Arial, sans-serif; font-variant-numeric: tabular-nums; }
  .sheet { width: 8.5in; min-height: 11in; margin: 24px auto; padding: 0.5in; background:#fff; box-shadow: 0 1px 3px rgba(15,26,46,.12), 0 12px 32px -8px rgba(15,26,46,.18); }
  .top { display:flex; justify-content:space-between; align-items:flex-start; gap:16pt; padding-bottom:9pt; border-bottom:1.5pt solid var(--ink); }
  .brand { display:flex; align-items:center; gap:10pt; min-width:0; }
  .logo { display:block; max-width:84pt; max-height:48pt; object-fit:contain; }
  h1 { font-size:15pt; line-height:1.2; margin:0; }
  h2 { font-size:10pt; margin:0 0 4pt; }
  .kind { color:var(--muted); margin:1pt 0 0; }
  .doc { text-align:right; white-space:nowrap; }
  .doc b { display:block; font-size:11pt; }
  .muted { color:var(--muted); }
  .cols { display:grid; grid-template-columns: 1fr 1fr; gap: 0 22pt; }
  .block { padding-top:8pt; }
  .label { font-size:7.5pt; font-weight:600; letter-spacing:.06em; text-transform:uppercase; color:var(--muted); margin:0 0 3pt; }
  table { width:100%; border-collapse:collapse; }
  td, th { padding:1.6pt 0; vertical-align:top; }
  th { text-align:left; font-weight:600; color:var(--muted); font-size:7.5pt; letter-spacing:.04em; text-transform:uppercase; border-bottom:1pt solid var(--rule); padding-bottom:3pt; }
  td + td, th + th { padding-left:8pt; }
  .num { text-align:right; white-space:nowrap; }
  table.kv td:first-child { color:var(--muted); padding-right:8pt; white-space:nowrap; }
  table.kv.left td:first-child { width:30%; }
  table.kv td:last-child { text-align:right; }
  table.kv.left td:last-child { text-align:left; }
  table.grid tbody tr { border-bottom:0.5pt solid var(--rule); }
  table.grid { font-size:8.5pt; line-height:1.25; }
  table.grid td { padding-top:1.3pt; padding-bottom:1.3pt; }
  thead { display:table-header-group; }
  tr { break-inside:avoid; }
  tr.sub td { color:var(--muted); }
  tr.total td { font-weight:600; font-size:10.5pt; border-top:1pt solid var(--rule); padding-top:4pt; }
  tr.this td { background:#f3f5ff; font-weight:600; }
  tr.void td { color:var(--muted); }
  tr.void td.amt { text-decoration:line-through; }
  .tag { display:inline-block; font-size:7pt; font-weight:600; text-transform:uppercase; letter-spacing:.05em; color:var(--accent); margin-left:4pt; }
  .st { display:inline-block; white-space:nowrap; font-weight:600; font-size:7.5pt; line-height:1.3; padding:0 4pt; border-radius:3pt; }
  .st.ok { color:var(--pos); background:var(--pos-bg); }
  .st.part { color:var(--warn); background:var(--warn-bg); }
  .st.late { color:var(--neg); background:var(--neg-bg); }
  .st.pend, .st.void { color:var(--muted); background:var(--soft); }
  .check { font-family:"Segoe UI Symbol", "Segoe UI", "DejaVu Sans", Arial, sans-serif; }
  .neg { color:var(--neg); }
  .note { margin:6pt 0 0; padding:5pt 8pt; background:var(--soft); border-radius:4pt; font-size:8.5pt; }
  .done { color:var(--pos); background:var(--pos-bg); font-weight:600; }
  .end { display:flex; justify-content:space-between; gap:16pt; margin-top:12pt; padding-top:6pt; border-top:0.5pt solid var(--rule); color:var(--muted); font-size:8.5pt; }
  .actions { text-align:center; margin: 0 auto 24px; }
  button { font: 10.5pt "Segoe UI", sans-serif; background:var(--accent); color:#fff; border:0; border-radius:8px; padding:8px 18px; cursor:pointer; }
  @media print {
    body { background:#fff; }
    .sheet { width:auto; min-height:0; margin:0; padding:0; box-shadow:none; }
    .actions { display:none; }
  }
"#;

/// A complete receipt page around `body`, which prints itself when opened.
/// Every printed page carries the receipt's name and its page number.
fn page(title: &str, css: &str, body: &str) -> String {
    let lang = i18n::get().key();
    let print = esc(t("Print", "Imprimir"));
    let (page_word, of) = (t("Page", "Página"), t("of", "de"));
    // CSS strings can't hold a raw double quote.
    let footer = esc(title).replace('"', "'");
    format!(
        r#"<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  @page {{ size: letter; margin: 0.5in;
    @bottom-left {{ content: "{footer}"; font: 7.5pt "Segoe UI", Arial, sans-serif; color: #5f6570; }}
    @bottom-right {{ content: "{page_word} " counter(page) " {of} " counter(pages); font: 7.5pt "Segoe UI", Arial, sans-serif; color: #5f6570; }}
  }}
{BASE_CSS}{css}</style>
</head>
<body>
<div class="sheet">
{body}
</div>
<div class="actions"><button onclick="window.print()">{print}</button></div>
<script>window.addEventListener('load', function () {{ setTimeout(function () {{ window.print(); }}, 250); }});</script>
</body>
</html>
"#,
        title = esc(title),
    )
}

fn row(label: &str, value: String, class: &str) -> String {
    format!("<tr class=\"{class}\"><td>{}</td><td class=\"num\">{value}</td></tr>", esc(label))
}

/// A green check, drawn as a text symbol so it prints on any system.
const CHECK: &str = "<span class=\"check\">&#10003;</span>";

fn status(class: &str, text: &str, check: bool) -> String {
    let mark = if check { format!("{CHECK} ") } else { String::new() };
    format!("<span class=\"st {class}\">{mark}{}</span>", esc(text))
}

/// The header: logo, business name, what the document is and its number.
fn header(snap: &Snapshot, business: &str, kind: &str, number: &str, date_label: &str, date: &str) -> String {
    format!(
        r#"<div class="top">
  <div class="brand">{logo}<div><h1>{business}</h1><p class="kind">{kind} · {no} {number}</p></div></div>
  <div class="doc"><span class="muted">{date_label}</span><b>{date}</b></div>
</div>"#,
        logo = logo_html(snap),
        business = esc(business),
        kind = esc(kind),
        no = esc(t("No.", "No.")),
        date_label = esc(date_label),
        date = esc(date),
    )
}

/// The client block, or nothing when no detail is printed.
fn client_block(c: &ClientLines) -> String {
    let rows = c.rows();
    if rows.is_empty() {
        return String::new();
    }
    let rows: String = rows.iter().map(|(label, v)| format!("<tr><td>{}</td><td>{}</td></tr>", esc(label), esc(v))).collect();
    format!("<div class=\"block\"><p class=\"label\">{}</p><table class=\"kv left\">{rows}</table></div>", esc(t("Client", "Cliente")))
}

fn loan_block(loan_id: i64, method: Method, rate_line: &str, principal: String, start: String) -> String {
    let rows = [
        row(t("Loan", "Préstamo"), format!("#{loan_id}"), ""),
        row(t("Type", "Tipo"), esc(method.short_label()), ""),
        row(t("Interest rate", "Tasa de interés"), esc(rate_line), ""),
        row(t("Amount lent", "Monto prestado"), principal, ""),
        row(t("Loan date", "Fecha del préstamo"), start, ""),
    ]
    .join("");
    format!("<div class=\"block\"><p class=\"label\">{}</p><table class=\"kv\">{rows}</table></div>", esc(t("Loan", "Préstamo")))
}

/// Says the receipt runs onto another page (`message`) when its `rows`
/// are more than `fit`, rather than leaving anything out.
fn overflow_note(rows: usize, fit: usize, message: &str) -> String {
    if rows <= fit {
        return String::new();
    }
    format!("<p class=\"note\">{}</p>", esc(message))
}

fn standing_html(s: Standing, paid: Cents) -> String {
    match s {
        Standing::Paid => status("ok", t("Paid", "Pagada"), true),
        Standing::PartlyPaid => status("part", t("Partly paid", "Pago parcial"), false),
        Standing::Overdue if paid > 0 => status("late", t("Overdue · partly paid", "Atrasada · pago parcial"), false),
        Standing::Overdue => status("late", t("Overdue", "Atrasada"), false),
        Standing::Pending => status("pend", t("Pending", "Pendiente"), false),
    }
}

/// The installment schedule as it stands after the payment.
fn schedule_table(d: &ReceiptData, m: &dyn Fn(Cents) -> String, ds: DateStyle) -> String {
    let rows: String = d
        .schedule
        .iter()
        .map(|x| {
            let this = if x.this_payment > 0 { m(x.this_payment) } else { String::new() };
            format!(
                "<tr class=\"{cls}\"><td>{seq}</td><td>{due}</td><td class=\"num\">{amount}</td><td class=\"num\">{this}</td><td class=\"num\">{paid}</td><td class=\"num\">{left}</td><td>{st}</td></tr>",
                cls = if x.this_payment > 0 { "this" } else { "" },
                seq = x.seq,
                due = esc(&ds.format(x.due)),
                amount = m(x.amount),
                paid = m(x.paid),
                left = if x.remaining > 0 { m(x.remaining) } else { "—".into() },
                st = standing_html(x.standing, x.paid),
            )
        })
        .collect();
    let (paid, total) = d.installments_paid.unwrap_or_default();
    format!(
        r#"<div class="block"><h2>{title} <span class="muted">· {progress}</span></h2>
<table class="grid"><thead><tr><th>#</th><th>{due}</th><th class="num">{amount}</th><th class="num">{this}</th><th class="num">{paid_h}</th><th class="num">{left}</th><th>{status}</th></tr></thead><tbody>{rows}</tbody></table></div>"#,
        title = esc(t("Installments", "Cuotas")),
        progress = esc(&tr!("{paid} of {total} paid in full", "{paid} de {total} pagadas por completo")),
        due = esc(t("Due date", "Vence")),
        amount = esc(t("Amount", "Monto")),
        this = esc(t("This payment", "Este pago")),
        paid_h = esc(t("Paid", "Pagado")),
        left = esc(t("Left", "Falta")),
        status = esc(t("Status", "Estado")),
    )
}

/// Every payment on the loan up to this one, oldest first.
fn history_table(d: &ReceiptData, m: &dyn Fn(Cents) -> String, ds: DateStyle) -> String {
    let installment_loan = d.method.has_installments();
    let rows: String = d
        .history
        .iter()
        .map(|h| {
            let applied = if h.standing == PaymentStanding::Void {
                esc(t("Reversed", "Revertido"))
            } else if installment_loan {
                h.installments
                    .iter()
                    .map(|x| {
                        if x.paid {
                            format!("#{} {CHECK}", x.seq)
                        } else {
                            let left = m(x.remaining);
                            format!("#{} ({})", x.seq, esc(&tr!("{left} left", "faltan {left}")))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            } else {
                let parts = [(t("Fee", "Cargo"), h.fee), (t("Interest", "Interés"), h.interest), (t("Capital", "Capital"), h.principal)];
                parts.iter().filter(|(_, v)| *v > 0).map(|(l, v)| format!("{} {}", esc(l), m(*v))).collect::<Vec<_>>().join(" · ")
            };
            let st = match h.standing {
                PaymentStanding::PaidOff => status("ok", t("Loan paid off", "Préstamo saldado"), true),
                PaymentStanding::Settled if installment_loan => status("ok", t("Settled", "Saldado"), true),
                PaymentStanding::Settled => status("ok", t("Interest paid to date", "Interés al día"), true),
                PaymentStanding::Partial(left) => {
                    let left = m(left);
                    let text = if installment_loan {
                        tr!("Partly paid · {left} left", "Pago parcial · faltan {left}")
                    } else {
                        tr!("Partly paid · {left} interest left", "Pago parcial · faltan {left} de interés")
                    };
                    status("part", &text, false)
                }
                PaymentStanding::Void => status("void", t("Void · not counted", "Anulado · no cuenta"), false),
            };
            let cls = match (h.current, h.standing) {
                (true, _) => "this",
                (_, PaymentStanding::Void) => "void",
                _ => "",
            };
            let tag = if h.current { format!("<span class=\"tag\">{}</span>", esc(t("This receipt", "Este recibo"))) } else { String::new() };
            let left = if h.standing == PaymentStanding::Void { String::new() } else { m(h.capital_left) };
            format!(
                "<tr class=\"{cls}\"><td>{date}{tag}</td><td class=\"num amt\">{amount}</td><td>{applied}</td><td class=\"num\">{left}</td><td>{st}</td></tr>",
                date = esc(&ds.format(h.paid_on)),
                amount = m(h.amount),
            )
        })
        .collect();
    let counted = d.history.iter().filter(|h| h.standing != PaymentStanding::Void).count();
    let n = i18n::count(counted, "payment", "payments", "pago", "pagos");
    format!(
        r#"<div class="block"><h2>{title} <span class="muted">· {sub}</span></h2>
<table class="grid"><thead><tr><th>{date}</th><th class="num">{amount}</th><th>{applied}</th><th class="num">{capital}</th><th>{status}</th></tr></thead><tbody>{rows}</tbody></table></div>"#,
        title = esc(t("Payment history", "Historial de pagos")),
        sub = esc(&tr!("{n}, oldest first", "{n}, del más antiguo al más reciente")),
        date = esc(t("Date", "Fecha")),
        amount = esc(t("Amount", "Monto")),
        applied = esc(if installment_loan { t("Installments", "Cuotas") } else { t("Applied to", "Aplicado a") }),
        capital = esc(t("Capital left", "Capital restante")),
        status = esc(t("Status", "Estado")),
    )
}

/// The payment receipt as a complete HTML document in the current language.
pub fn render(d: &ReceiptData, snap: &Snapshot) -> String {
    let money = snap.settings.money();
    let ds = snap.settings.date_style;
    let m = |c: Cents| esc(&money.format(c));
    let number = format!("{:06}", d.payment_id);
    let installment_loan = d.method.has_installments();

    let mut paid = vec![row(t("Amount paid", "Monto pagado"), m(d.amount), "total")];
    if d.fee > 0 {
        paid.push(row(if installment_loan { t("Late fees", "Cargos por mora") } else { t("Fee", "Cargo") }, m(d.fee), "sub"));
    }
    paid.push(row(t("Interest", "Interés"), m(d.interest), "sub"));
    paid.push(row(t("Capital", "Abono a capital"), m(d.principal), "sub"));

    let mut bal = vec![
        row(t("Capital before this payment", "Capital antes de este pago"), m(d.principal_before), ""),
        row(t("Capital remaining", "Capital pendiente"), m(d.principal_after), ""),
    ];
    if d.interest_after > 0 {
        bal.push(row(t("Interest pending", "Interés pendiente"), m(d.interest_after), ""));
    }
    if d.fees_after > 0 {
        bal.push(row(t("Late fees pending", "Cargos por mora pendientes"), format!("<span class=\"neg\">{}</span>", m(d.fees_after)), ""));
    }
    if d.overdue_after > 0 && installment_loan {
        bal.push(row(t("Due and unpaid now", "Vencido sin pagar"), format!("<span class=\"neg\">{}</span>", m(d.overdue_after)), ""));
    }
    bal.push(match d.left_to_pay {
        Some(left) => row(t("Total left to pay", "Total por pagar"), m(left), "total"),
        None => row(t("Total owed now", "Total adeudado"), m(d.owed_after()), "total"),
    });
    let repaid = percent_label(d.original_principal - d.principal_after, d.original_principal).unwrap_or_else(|| "0.0%".into());
    let status_line = if d.balance_after() <= 0 {
        format!(
            "<p class=\"note done\">{CHECK} {}</p>",
            esc(t("This loan is fully paid. Thank you!", "Este préstamo quedó saldado. ¡Gracias!"))
        )
    } else {
        let next = d
            .next_due
            .map(|(date, amount)| {
                let (date, amount) = (ds.format(date), money.format(amount));
                tr!(" Next installment: {date} · {amount}.", " Próxima cuota: {date} · {amount}.")
            })
            .unwrap_or_default();
        format!("<p class=\"note\">{}</p>", esc(&tr!("{repaid} of the capital repaid.{next}", "{repaid} del capital pagado.{next}")))
    };

    let void = d
        .reversed
        .as_ref()
        .map(|r| format!("<div class=\"void\">{}<span>{}</span></div>", esc(t("VOID", "ANULADO")), esc(r)))
        .unwrap_or_default();
    let notes = if d.notes.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"muted\">{} {}</p>", esc(t("Note:", "Nota:")), esc(&d.notes))
    };
    let schedule = if installment_loan { schedule_table(d, &m, ds) } else { String::new() };
    let schedule_rows = if d.schedule.is_empty() { 0 } else { d.schedule.len() + 2 };
    let rows = schedule_rows + d.history.iter().map(|h| h.installments.len().max(1)).sum::<usize>();
    let title = tr!("Receipt {number}", "Recibo {number}");
    let css = r#"
  .void { border:2pt solid var(--neg); color:var(--neg); border-radius:6pt; text-align:center; font-weight:700; font-size:14pt; padding:4pt; margin-bottom:8pt; }
  .void span { display:block; font-size:8.5pt; font-weight:400; }
"#;
    let body = format!(
        r#"{void}
{header}
<div class="cols">{client}{loan}</div>
<div class="cols">
  <div class="block"><p class="label">{paid_l}</p><table class="kv">{paid}</table></div>
  <div class="block"><p class="label">{bal_l}</p><table class="kv">{bal}</table></div>
</div>
{status_line}
{overflow}
{schedule}
{history}
{notes}
<div class="end"><span>{thanks}</span><span>{loan_l} #{loan_id} · {receipt_l} {number}</span></div>"#,
        header = header(
            snap,
            &d.business,
            t("Payment receipt", "Recibo de pago"),
            &number,
            t("Date paid", "Fecha de pago"),
            &ds.format(d.paid_on)
        ),
        client = client_block(&d.client),
        loan = loan_block(d.loan_id, d.method, &d.rate_line, m(d.original_principal), esc(&ds.format(d.start_date))),
        paid_l = esc(t("This payment", "Este pago")),
        paid = paid.join(""),
        bal_l = esc(t("Balance after this payment", "Saldo después de este pago")),
        bal = bal.join(""),
        overflow = overflow_note(
            rows,
            PAYMENT_PAGE_ROWS,
            t(
                "This loan has a long history, so the receipt continues on the next page. Every payment is listed.",
                "Este préstamo tiene un historial largo, así que el recibo continúa en la página siguiente. Aparecen todos los pagos.",
            ),
        ),
        history = history_table(d, &m, ds),
        thanks = esc(t("Thank you for your payment.", "Gracias por su pago.")),
        loan_l = esc(t("Loan", "Préstamo")),
        loan_id = d.loan_id,
        receipt_l = esc(t("Receipt", "Recibo")),
    );
    page(&title, css, &body)
}

/// The loan receipt: the terms the client agreed to, the schedule of an
/// installment loan, how payments work and space for both signatures.
pub fn render_loan(d: &LoanReceipt, snap: &Snapshot) -> String {
    let money: MoneyFormat = snap.settings.money();
    let ds = snap.settings.date_style;
    let m = |c: Cents| esc(&money.format(c));
    let number = format!("{:06}", d.loan_id);
    let installment_loan = d.method.has_installments();

    let mut terms = vec![
        row(t("Amount lent", "Monto prestado"), m(d.principal), "total"),
        row(t("Loan type", "Tipo de préstamo"), esc(d.method.short_label()), ""),
        row(t("Interest rate", "Tasa de interés"), esc(&d.rate_line), ""),
        row(t("Payments", "Pagos"), esc(d.frequency.label()), ""),
    ];
    let (schedule_html, rules): (String, Vec<String>) = if installment_loan {
        let total_interest = d.total_interest();
        terms.push(row(t("Installments", "Cuotas"), d.schedule.len().to_string(), ""));
        terms.push(row(t("Total interest", "Interés total"), m(total_interest), ""));
        if d.late_fee_rate.0 > 0 {
            terms.push(row(t("Late fee", "Cargo por mora"), esc(&d.late_fee_rate.percent_label()), ""));
        }
        terms.push(row(t("Total to repay", "Total a pagar"), m(d.principal + total_interest), "total"));
        let mut left = d.principal;
        let rows: String = d
            .schedule
            .iter()
            .map(|x| {
                left -= x.principal;
                format!(
                    "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\"><b>{}</b></td><td class=\"num\">{}</td></tr>",
                    x.seq,
                    esc(&ds.format(x.due)),
                    m(x.principal),
                    m(x.interest),
                    m(x.total()),
                    m(left)
                )
            })
            .collect();
        let schedule_html = format!(
            "<div class=\"block\"><h2>{}</h2><table class=\"grid\"><thead><tr><th>#</th><th>{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th></tr></thead><tbody>{rows}</tbody></table></div>",
            esc(t("Payment schedule", "Calendario de pagos")),
            esc(t("Due date", "Vence")),
            esc(t("Capital", "Capital")),
            esc(t("Interest", "Interés")),
            esc(t("Payment", "Cuota")),
            esc(t("Capital left", "Restante")),
        );
        let fee = d.late_fee_rate.percent_label();
        let mut r = vec![d.method.explanation().to_string()];
        if let Some(c) = &d.conversion {
            r.push(tr!("The rate is converted on a 30-day month: {c}.", "La tasa se convierte sobre un mes de 30 días: {c}."));
        }
        if d.late_fee_rate.0 > 0 {
            r.push(tr!(
                "An installment not fully paid by its due date is charged a late fee of {fee} of what is still unpaid, once, the day after.",
                "Una cuota que no se pague completa a su vencimiento tiene un cargo por mora de {fee} de lo que falte, una sola vez, al día siguiente."
            ));
        }
        r.push(
            t(
                "Payments cover late fees first, then interest, then capital. The loan can be paid off early with the capital owed plus the interest due.",
                "Los pagos cubren primero los cargos por mora, luego el interés y después el capital. El préstamo se puede saldar antes con el capital adeudado más el interés pendiente.",
            )
            .to_string(),
        );
        if d.schedule_edited {
            r.push(t("This schedule was set by agreement.", "Este calendario se fijó de común acuerdo.").to_string());
        }
        (schedule_html, r)
    } else {
        // An open loan has no schedule: no due dates are printed.
        terms.push(row(t("Interest per payment", "Interés por pago"), m(d.interest_per_period), ""));
        let half = d.principal / 2;
        let (i, p, h, pay, next) =
            (m(d.interest_per_period), m(d.principal), m(half), m(d.interest_per_period + half), m(d.interest_after_half));
        let mut r = vec![d.method.explanation().to_string()];
        if let Some(c) = &d.conversion {
            r.push(tr!("The rate is converted on a 30-day month: {c}.", "La tasa se convierte sobre un mes de 30 días: {c}."));
        }
        r.extend([
            tr!("Paying {i} each time covers the interest; the capital stays at {p}.", "Si paga {i} cada vez, cubre el interés y el capital sigue en {p}."),
            tr!(
                "Anything paid above the interest lowers the capital. For example, paying {pay} is {i} of interest and {h} of capital, and the next interest drops to {next}.",
                "Todo lo que pague por encima del interés baja el capital. Por ejemplo, si paga {pay}, son {i} de interés y {h} de capital, y el próximo interés baja a {next}."
            ),
            t(
                "The loan can be paid off at any time with the capital owed plus the interest due. It closes when the capital reaches zero.",
                "El préstamo se puede saldar en cualquier momento con el capital adeudado más el interés pendiente. Se cierra cuando el capital llega a cero.",
            )
            .to_string(),
        ]);
        (String::new(), r)
    };
    let rules = rules.iter().map(|r| format!("<li>{}</li>", esc(r))).collect::<String>();
    let notes = if d.notes.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"muted\">{} {}</p>", esc(t("Notes:", "Notas:")), esc(&d.notes))
    };
    let title = tr!("Loan receipt {number}", "Recibo de préstamo {number}");
    let css = r#"
  .received { margin:10pt 0 0; padding:6pt 9pt; background:#f3f5ff; border-radius:4pt; font-size:10pt; }
  .rules { margin-top:10pt; font-size:7.5pt; line-height:1.35; color:var(--muted); }
  .rules h2 { font-size:8pt; color:var(--ink); margin-bottom:2pt; }
  .rules ul { margin:0; padding-left:12pt; }
  .rules li { margin:1pt 0; }
  .sign { display:flex; gap:28pt; margin-top:36pt; break-inside:avoid; }
  .sign div { flex:1; border-top:1pt solid var(--ink); padding-top:4pt; text-align:center; font-size:8.5pt; color:var(--muted); }
"#;
    // The client's name appears only where Settings allows it.
    let (amount, date) = (m(d.principal), esc(&ds.format(d.start_date)));
    let (received, client_sign) = match &d.client.name {
        Some(name) => {
            let sign = esc(&tr!("Client: {name}", "Cliente: {name}"));
            let name = esc(name);
            (tr!("{name} received {amount} on {date}.", "{name} recibió {amount} el {date}."), sign)
        }
        None => (tr!("The client received {amount} on {date}.", "El cliente recibió {amount} el {date}."), esc(t("Client", "Cliente"))),
    };
    let body = format!(
        r#"{header}
<div class="cols">{client}<div class="block"><p class="label">{terms_l}</p><table class="kv">{terms}</table></div></div>
<p class="received">{received}</p>
{overflow}
{schedule_html}
<div class="rules"><h2>{rules_h}</h2><ul>{rules}</ul></div>
{notes}
<div class="sign"><div>{client_sign}</div><div>{lender_sign}</div></div>"#,
        header = header(
            snap,
            &d.business,
            t("Loan receipt", "Recibo de préstamo"),
            &number,
            t("Loan date", "Fecha del préstamo"),
            &ds.format(d.start_date)
        ),
        client = client_block(&d.client),
        terms_l = esc(&{
            let id = d.loan_id;
            tr!("Loan #{id}", "Préstamo #{id}")
        }),
        terms = terms.join(""),
        overflow = overflow_note(
            d.schedule.len(),
            LOAN_PAGE_ROWS,
            t(
                "The schedule continues on the next page; the signatures are at the end.",
                "El calendario continúa en la página siguiente; las firmas están al final.",
            ),
        ),
        rules_h = esc(t("How payments work", "Cómo funcionan los pagos")),
        lender_sign = esc(&{
            let business = &d.business;
            tr!("For {business}", "Por {business}")
        }),
    );
    page(&title, css, &body)
}

/// Writes the receipt for `payment_id` into `dir` and returns its path.
pub fn write(snap: &Snapshot, payment_id: i64, dir: &Path) -> Result<PathBuf> {
    let d = data(snap, payment_id).ok_or_else(|| anyhow!("payment {payment_id} not found"))?;
    save(dir, &format!("receipt-{payment_id:06}.html"), &render(&d, snap))
}

/// Writes the loan receipt for `loan_id` into `dir` and returns its path.
pub fn write_loan(snap: &Snapshot, loan_id: i64, dir: &Path) -> Result<PathBuf> {
    let d = loan_data(snap, loan_id).ok_or_else(|| anyhow!("loan {loan_id} not found"))?;
    save(dir, &format!("loan-{loan_id:06}.html"), &render_loan(&d, snap))
}

fn save(dir: &Path, name: &str, html: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("Could not create {}", dir.display()))?;
    let path = dir.join(name);
    std::fs::write(&path, html).with_context(|| format!("Could not write {}", path.display()))?;
    Ok(path)
}
