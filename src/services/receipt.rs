//! Printable receipts: one for each payment and one for a loan when the
//! client receives it. A receipt is a self-contained HTML page written to the
//! receipts folder and opened in the default browser, which shows the print
//! dialog. It works offline and prints on letter paper or a narrow receipt
//! printer. The name at the top is the receipt name from Settings.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow};
use chrono::NaiveDate;

use super::Snapshot;
use crate::domain::installments::{self, Applied, Installment, InstallmentStatus, Instruction, PlanPayment};
use crate::domain::ledger::{delinquency, owed_at_payment};
use crate::domain::loans::{Frequency, Method};
use crate::domain::money::{Cents, Rate, percent_label};
use crate::i18n::{self, t};
use crate::tr;

/// How many upcoming due dates an interest-only loan receipt lists.
const SCHEDULE_ROWS: u32 = 6;

/// What one payment did to one installment, and how it stands after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallmentLine {
    pub seq: u32,
    pub due: NaiveDate,
    pub applied: Applied,
    /// Still owed on it right after the payment.
    pub remaining: Cents,
    pub paid: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptData {
    pub payment_id: i64,
    pub business: String,
    pub client_name: String,
    pub client_phone: Option<String>,
    pub loan_id: i64,
    /// "10% a month · paid weekly".
    pub rate_line: String,
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
    pub next_due: Option<(NaiveDate, Cents)>,
    pub notes: String,
    pub reversed: Option<String>,
    /// Installment loans: where the payment went.
    pub installments: Vec<InstallmentLine>,
    /// Installment loans: (paid in full, total installments).
    pub installments_paid: Option<(usize, usize)>,
    /// Installment loans: everything left to pay as scheduled.
    pub left_to_pay: Option<Cents>,
}

impl ReceiptData {
    pub fn owed_before(&self) -> Cents {
        self.principal_before + self.interest_before
    }
    pub fn owed_after(&self) -> Cents {
        self.principal_after + self.interest_after + self.fees_after
    }
}

/// Everything shown on the receipt, computed from the ledger as it stood
/// right after this payment.
pub fn data(snap: &Snapshot, payment_id: i64) -> Option<ReceiptData> {
    let payment = snap.payments.iter().find(|p| p.id == payment_id)?;
    let loan = snap.loan(payment.loan_id)?;
    let book = snap.book_loan(loan.id)?;
    let client = snap.client(loan.client_id)?;

    // Live payments up to and including this one, in ledger order.
    let mut history: Vec<_> = snap.payments.iter().filter(|p| p.loan_id == loan.id && !p.is_reversed()).collect();
    if payment.is_reversed() {
        // A reversed payment is shown as it was when recorded.
        history.push(payment);
        history.sort_by_key(|p| (p.paid_on, p.id));
    }
    let upto = history.iter().position(|p| p.id == payment_id)?;
    let mut d = ReceiptData {
        payment_id,
        business: snap.settings.receipt_title(),
        client_name: client.full_name.clone(),
        client_phone: client.phone.clone(),
        loan_id: loan.id,
        rate_line: loan.terms.rate_line(),
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
        installments_paid: None,
        left_to_pay: None,
    };

    if let Some(plan) = &book.plan {
        let pays: Vec<PlanPayment> = history[..=upto]
            .iter()
            .map(|p| PlanPayment { paid_on: p.paid_on, amount: p.amount, instruction: Instruction::Fixed(p.applied.clone()) })
            .collect();
        let state = installments::run(&book.facts, plan, &pays, payment.paid_on, NaiveDate::MAX).ok()?;
        let on = payment.paid_on;
        d.principal_after = state.principal_outstanding();
        d.principal_before = d.principal_after + payment.principal;
        // Owed now: installments due by the payment date (later ones are not due yet).
        let due_now: Vec<_> = state.installments.iter().filter(|x| x.due <= on).collect();
        d.interest_after = due_now.iter().map(|x| x.unpaid_interest()).sum();
        d.fees_after = state.installments.iter().map(|x| x.unpaid_fee()).sum();
        d.overdue_after = due_now.iter().map(|x| x.owed()).sum();
        d.interest_before = d.interest_after + payment.interest;
        d.installments = payment
            .applied
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
            .collect();
        let paid = state.installments.iter().filter(|x| x.remaining() <= 0).count();
        d.installments_paid = Some((paid, state.installments.len()));
        d.left_to_pay = Some(state.installments.iter().map(|x| x.remaining()).sum());
        d.next_due = state.installments.iter().find(|x| x.due > on && x.remaining() > 0).map(|x| (x.due, x.remaining() + d.overdue_after));
        return Some(d);
    }

    let facts: Vec<_> = history[..=upto].iter().map(|p| p.fact()).collect();
    let after = owed_at_payment(&book.facts, &facts, payment.paid_on);
    let next = delinquency(&book.facts, &facts, payment.paid_on);
    d.next_due = (after.payoff() > 0).then_some(next.next_due.map(|n| (n, next.next_due_amount))).flatten();
    d.principal_before = after.principal_outstanding + payment.principal;
    d.interest_before = after.interest_outstanding + payment.interest;
    d.principal_after = after.principal_outstanding;
    d.interest_after = after.interest_outstanding;
    Some(d)
}

/// What a client agreed to when taking a loan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoanReceipt {
    pub loan_id: i64,
    pub business: String,
    pub client_name: String,
    pub client_phone: Option<String>,
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
    /// Interest only: the next due dates with their interest.
    pub upcoming: Vec<(u32, NaiveDate, Cents)>,
    /// Installment loans: the full schedule.
    pub schedule: Vec<Installment>,
    pub schedule_edited: bool,
    pub notes: String,
}

impl LoanReceipt {
    /// Everything owed on the first due date, to pay an interest-only loan
    /// off then.
    pub fn payoff_at_first_due(&self) -> Cents {
        self.principal + self.interest_per_period
    }
    pub fn total_interest(&self) -> Cents {
        self.schedule.iter().map(|x| x.interest).sum()
    }
}

pub fn loan_data(snap: &Snapshot, loan_id: i64) -> Option<LoanReceipt> {
    let loan = snap.loan(loan_id)?;
    let client = snap.client(loan.client_id)?;
    let terms = &loan.terms;
    let interest = terms.interest_per_period();
    Some(LoanReceipt {
        loan_id,
        business: snap.settings.receipt_title(),
        client_name: client.full_name.clone(),
        client_phone: client.phone.clone(),
        start_date: terms.start_date,
        principal: terms.principal,
        method: terms.method,
        rate_line: terms.rate_line(),
        conversion: terms.period_rate().conversion_note(),
        frequency: terms.frequency,
        late_fee_rate: terms.late_fee_rate,
        interest_per_period: interest,
        upcoming: if terms.method.has_installments() {
            Vec::new()
        } else {
            (1..=SCHEDULE_ROWS).map(|k| (k, terms.frequency.due_date(terms.start_date, k), interest)).collect()
        },
        schedule: loan.schedule.clone(),
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

/// Styles shared by every receipt.
const BASE_CSS: &str = r#"
  :root { --ink:#17191c; --muted:#666c77; --rule:#e9ebef; --accent:#365cf5; --pos:#137a60; --neg:#c2353d; }
  * { box-sizing: border-box; }
  body { margin:0; background:#f7f8fa; color:var(--ink); font: 14px/1.45 "Segoe UI", system-ui, sans-serif; font-variant-numeric: tabular-nums; }
  .sheet { max-width: 380px; margin: 24px auto; background:#fff; border:1px solid var(--rule); border-radius:12px; padding:24px; }
  .brand { display:flex; align-items:center; gap:14px; margin:0 0 16px; }
  .logo { display:block; max-width:96px; max-height:64px; object-fit:contain; }
  h1 { font-size:20px; margin:0 0 2px; letter-spacing:-0.01em; }
  h2 { font-size:14px; margin:4px 0 2px; }
  .kind { color:var(--muted); margin:0; }
  .meta { display:flex; justify-content:space-between; gap:12px; padding:12px 0; border-top:1px solid var(--rule); border-bottom:1px solid var(--rule); margin-bottom:12px; }
  .meta b { display:block; }
  .muted { color:var(--muted); font-size:13px; }
  .neg { color:var(--neg); }
  table { width:100%; border-collapse:collapse; margin:8px 0 12px; }
  td, th { padding:4px 0; }
  th { text-align:left; font-weight:500; color:var(--muted); font-size:12px; border-bottom:1px solid var(--rule); }
  td:last-child, th:last-child { text-align:right; white-space:nowrap; }
  td.num, th.num { text-align:right; white-space:nowrap; }
  tr.sub td { color:var(--muted); padding-left:12px; }
  tr.total td { font-weight:600; font-size:15px; padding-top:6px; }
  .section { border-top:1px solid var(--rule); padding-top:10px; }
  .thanks { text-align:center; color:var(--muted); margin-top:16px; font-size:13px; }
  .actions { text-align:center; margin: 0 auto 24px; }
  button { font: inherit; background:var(--accent); color:#fff; border:0; border-radius:8px; padding:8px 18px; cursor:pointer; }
  @media print {
    body { background:#fff; }
    .sheet { border:0; margin:0 auto; padding:0; max-width: 100%; }
    .actions { display:none; }
    @page { margin: 10mm; }
  }
"#;

/// A complete receipt page around `body`, which prints itself when opened.
fn page(title: &str, css: &str, body: &str) -> String {
    let lang = i18n::get().key();
    let print = esc(t("Print", "Imprimir"));
    format!(
        r#"<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>{BASE_CSS}{css}</style>
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
    format!("<tr class=\"{class}\"><td>{}</td><td>{value}</td></tr>", esc(label))
}

/// The payment receipt as a complete HTML document in the current language.
pub fn render(d: &ReceiptData, snap: &Snapshot) -> String {
    let money = snap.settings.money();
    let ds = snap.settings.date_style;
    let m = |c: Cents| esc(&money.format(c));
    let repaid = d.original_principal - d.principal_after;
    let repaid_pct = percent_label(repaid, d.original_principal).unwrap_or_else(|| "0.0%".into());
    let remaining_pct = percent_label(d.principal_after, d.original_principal).unwrap_or_else(|| "0.0%".into());
    let bar = if d.original_principal > 0 { (repaid as f64 / d.original_principal as f64 * 100.0).clamp(0.0, 100.0) } else { 0.0 };
    let number = format!("{:06}", d.payment_id);

    let mut lines = vec![row(t("Amount paid", "Monto pagado"), m(d.amount), "total")];
    if d.fee > 0 {
        lines.push(row(if d.installments.is_empty() { t("Fee", "Cargo") } else { t("Late fees", "Cargos por mora") }, m(d.fee), "sub"));
    }
    lines.push(row(t("Interest", "Interés"), m(d.interest), "sub"));
    lines.push(row(t("Capital", "Abono a capital"), m(d.principal), "sub"));
    let paid_table = lines.join("");

    // Installment loans: where the payment went, installment by installment.
    let per_installment = if d.installments.is_empty() {
        String::new()
    } else {
        let rows: String = d
            .installments
            .iter()
            .map(|x| {
                let status = if x.paid { esc(t("paid", "pagada")) } else { format!("{} {}", esc(t("owes", "debe")), m(x.remaining)) };
                format!(
                    "<tr><td>#{} · {}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>{status}</td></tr>",
                    x.seq,
                    esc(&ds.format(x.due)),
                    m(x.applied.principal),
                    m(x.applied.interest),
                    m(x.applied.fee),
                )
            })
            .collect();
        format!(
            "<div class=\"section\"><h2>{}</h2><table><tr><th>{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th></th></tr>{rows}</table></div>",
            esc(t("Applied to", "Aplicado a")),
            esc(t("Installment", "Cuota")),
            esc(t("Capital", "Capital")),
            esc(t("Interest", "Interés")),
            esc(t("Late fee", "Mora")),
        )
    };

    let mut bal = vec![
        row(t("Capital before this payment", "Capital antes de este pago"), m(d.principal_before), ""),
        row(t("Capital remaining", "Capital pendiente"), m(d.principal_after), ""),
    ];
    if d.interest_after > 0 {
        bal.push(row(t("Interest pending", "Interés pendiente"), m(d.interest_after), ""));
    }
    if d.fees_after > 0 {
        bal.push(format!(
            "<tr><td>{}</td><td class=\"neg\">{}</td></tr>",
            esc(t("Late fees pending", "Cargos por mora pendientes")),
            m(d.fees_after)
        ));
    }
    if d.overdue_after > 0 && !d.installments.is_empty() {
        bal.push(format!(
            "<tr><td>{}</td><td class=\"neg\">{}</td></tr>",
            esc(t("Due and unpaid now", "Vencido sin pagar")),
            m(d.overdue_after)
        ));
    }
    match d.left_to_pay {
        Some(left) => bal.push(row(t("Total left to pay", "Total por pagar"), m(left), "total")),
        None => bal.push(row(t("Total owed now", "Total adeudado"), m(d.owed_after()), "total")),
    }
    let balance_table = bal.join("");

    let next = match d.next_due {
        Some((date, amount)) if d.left_to_pay.unwrap_or_else(|| d.owed_after()) > 0 => format!(
            "<p class=\"next\">{} <b>{}</b> · <b>{}</b></p>",
            esc(t("Next payment:", "Próximo pago:")),
            esc(&ds.format(date)),
            m(amount)
        ),
        _ if d.left_to_pay.unwrap_or_else(|| d.owed_after()) > 0 => String::new(),
        _ => format!(
            "<p class=\"next done\">{}</p>",
            esc(t("This loan is fully paid. Thank you!", "Este préstamo quedó saldado. ¡Gracias!"))
        ),
    };
    let progress_line = match d.installments_paid {
        Some((paid, total)) => tr!("{paid} of {total} installments paid in full.", "{paid} de {total} cuotas pagadas por completo."),
        None => {
            let share = percent_label(d.amount - d.fee, d.owed_before()).unwrap_or_default();
            tr!("This payment covered {share} of the balance owed.", "Este pago cubrió el {share} de lo adeudado.")
        }
    };
    let void = d
        .reversed
        .as_ref()
        .map(|r| format!("<div class=\"void\">{}<span>{}</span></div>", esc(t("VOID", "ANULADO")), esc(r)))
        .unwrap_or_default();
    let phone = d.client_phone.as_ref().map(|p| format!("<div class=\"muted\">{}</div>", esc(p))).unwrap_or_default();
    let notes = if d.notes.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"muted\">{} {}</p>", esc(t("Note:", "Nota:")), esc(&d.notes))
    };
    let title = tr!("Receipt {number}", "Recibo {number}");
    let css = format!(
        r#"
  .bar {{ height:8px; border-radius:4px; background:#eef0f4; overflow:hidden; margin:8px 0 6px; }}
  .bar i {{ display:block; height:100%; background:var(--accent); width:{bar:.1}%; }}
  .pcts {{ display:flex; justify-content:space-between; font-size:13px; }}
  .pcts b {{ font-size:15px; }}
  .next {{ margin:14px 0 4px; padding:10px 12px; background:#f3f5ff; border-radius:8px; }}
  .next.done {{ background:#e7f5f0; color:var(--pos); font-weight:600; }}
  .void {{ border:2px solid var(--neg); color:var(--neg); border-radius:8px; text-align:center; font-weight:700; font-size:18px; padding:6px; margin-bottom:12px; }}
  .void span {{ display:block; font-size:12px; font-weight:400; }}
  .sheet {{ max-width: {width}px; }}
"#,
        width = if d.installments.is_empty() { 380 } else { 460 },
    );
    let body = format!(
        r#"  {void}
  <div class="brand">{logo}<div><h1>{business}</h1><p class="kind">{kind} · {no} {number}</p></div></div>
  <div class="meta">
    <div><span class="muted">{client_l}</span><b>{client}</b>{phone}</div>
    <div style="text-align:right"><span class="muted">{date_l}</span><b>{date}</b><span class="muted">{loan_l} #{loan_id} · {rate}</span></div>
  </div>
  <table>{paid_table}</table>
  {per_installment}
  <div class="section">
    <table>{balance_table}</table>
  </div>
  <div class="section">
    <div class="bar"><i></i></div>
    <div class="pcts">
      <div>{repaid_l}<br><b>{repaid_pct}</b></div>
      <div style="text-align:right">{remaining_l}<br><b>{remaining_pct}</b></div>
    </div>
    <p class="muted">{progress_line}</p>
  </div>
  {next}
  {notes}
  <p class="thanks">{thanks}</p>"#,
        logo = logo_html(snap),
        business = esc(&d.business),
        kind = esc(t("Payment receipt", "Recibo de pago")),
        no = esc(t("No.", "No.")),
        client_l = esc(t("Client", "Cliente")),
        client = esc(&d.client_name),
        date_l = esc(t("Date", "Fecha")),
        date = esc(&ds.format(d.paid_on)),
        loan_l = esc(t("Loan", "Préstamo")),
        loan_id = d.loan_id,
        rate = esc(&d.rate_line),
        repaid_l = esc(t("Capital paid", "Capital pagado")),
        remaining_l = esc(t("Capital remaining", "Capital pendiente")),
        progress_line = esc(&progress_line),
        thanks = esc(t("Thank you for your payment.", "Gracias por su pago.")),
    );
    page(&title, &css, &body)
}

/// The loan receipt: the terms the client agreed to, the payment dates and
/// space for both signatures.
pub fn render_loan(d: &LoanReceipt, snap: &Snapshot) -> String {
    let money = snap.settings.money();
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
        let n = d.schedule.len();
        terms.push(row(t("Installments", "Cuotas"), n.to_string(), ""));
        terms.push(row(t("Total interest", "Interés total"), m(total_interest), ""));
        terms.push(row(t("Total to repay", "Total a pagar"), m(d.principal + total_interest), "total"));
        if d.late_fee_rate.0 > 0 {
            terms.push(row(t("Late fee", "Cargo por mora"), esc(&d.late_fee_rate.percent_label()), ""));
        }
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
            "<table><tr><th>#</th><th>{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th><th class=\"num\">{}</th></tr>{rows}</table>",
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
        let first_due = d.upcoming.first().map(|(_, due, _)| *due).unwrap_or(d.start_date);
        terms.push(row(t("Interest per payment", "Interés por pago"), m(d.interest_per_period), ""));
        terms.push(row(t("First payment due", "Primer pago"), esc(&ds.format(first_due)), ""));
        terms.push(row(t("To pay it all on that date", "Para saldar en esa fecha"), m(d.payoff_at_first_due()), ""));
        let rows: String = d
            .upcoming
            .iter()
            .map(|(k, due, interest)| format!("<tr><td>{k}</td><td>{}</td><td>{}</td></tr>", esc(&ds.format(*due)), m(*interest)))
            .collect();
        let schedule_html = format!(
            "<p class=\"muted\">{}</p><table><tr><th>#</th><th>{}</th><th>{}</th></tr>{rows}</table>",
            esc(t(
                "Interest due on each date while the capital stays the same. Paying capital lowers the amounts that follow.",
                "Interés a pagar en cada fecha mientras el capital no cambie. Abonar a capital baja los montos siguientes.",
            )),
            esc(t("Due date", "Vence")),
            esc(t("Interest", "Interés")),
        );
        let half = d.principal / 2;
        let rate = snap.loan(d.loan_id).map(|l| l.terms.period_rate());
        let next_after_half = rate.map(|r| r.interest(d.principal - half)).unwrap_or_default();
        let (i, p, h, pay, next) = (m(d.interest_per_period), m(d.principal), m(half), m(d.interest_per_period + half), m(next_after_half));
        let rules = vec![
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
        ];
        (schedule_html, rules)
    };
    let rules = rules.iter().map(|r| format!("<li>{}</li>", esc(r))).collect::<String>();
    let phone = d.client_phone.as_ref().map(|p| format!("<div class=\"muted\">{}</div>", esc(p))).unwrap_or_default();
    let notes = if d.notes.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"muted\">{} {}</p>", esc(t("Notes:", "Notas:")), esc(&d.notes))
    };
    let title = tr!("Loan receipt {number}", "Recibo de préstamo {number}");
    let css = format!(
        r#"
  .sheet {{ max-width: {width}px; }}
  ul {{ margin:6px 0 12px; padding-left:18px; }}
  li {{ margin:4px 0; }}
  .received {{ margin:12px 0; padding:10px 12px; background:#f3f5ff; border-radius:8px; }}
  .sign {{ display:flex; gap:24px; margin-top:44px; }}
  .sign div {{ flex:1; border-top:1px solid var(--ink); padding-top:6px; text-align:center; font-size:12px; color:var(--muted); }}
"#,
        width = if installment_loan { 520 } else { 440 },
    );
    let received = {
        let (name, amount, date) = (esc(&d.client_name), m(d.principal), esc(&ds.format(d.start_date)));
        tr!("{name} received {amount} on {date}.", "{name} recibió {amount} el {date}.")
    };
    let body = format!(
        r#"  <div class="brand">{logo}<div><h1>{business}</h1><p class="kind">{kind} · {no} {number}</p></div></div>
  <div class="meta">
    <div><span class="muted">{client_l}</span><b>{client}</b>{phone}</div>
    <div style="text-align:right"><span class="muted">{date_l}</span><b>{date}</b><span class="muted">{loan_l} #{loan_id}</span></div>
  </div>
  <p class="received">{received}</p>
  <table>{terms}</table>
  <div class="section">
    <h2>{schedule_h}</h2>
    {schedule_html}
  </div>
  <div class="section">
    <h2>{rules_h}</h2>
    <ul>{rules}</ul>
  </div>
  {notes}
  <div class="sign"><div>{client_sign}</div><div>{lender_sign}</div></div>"#,
        logo = logo_html(snap),
        business = esc(&d.business),
        kind = esc(t("Loan receipt", "Recibo de préstamo")),
        no = esc(t("No.", "No.")),
        client_l = esc(t("Client", "Cliente")),
        client = esc(&d.client_name),
        date_l = esc(t("Loan date", "Fecha del préstamo")),
        date = esc(&ds.format(d.start_date)),
        loan_l = esc(t("Loan", "Préstamo")),
        loan_id = d.loan_id,
        terms = terms.join(""),
        schedule_h =
            esc(if installment_loan { t("Payment schedule", "Calendario de pagos") } else { t("Payment dates", "Fechas de pago") }),
        rules_h = esc(t("How payments work", "Cómo funcionan los pagos")),
        client_sign = esc(&{
            let name = &d.client_name;
            tr!("Client: {name}", "Cliente: {name}")
        }),
        lender_sign = esc(&{
            let business = &d.business;
            tr!("For {business}", "Por {business}")
        }),
    );
    page(&title, &css, &body)
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
