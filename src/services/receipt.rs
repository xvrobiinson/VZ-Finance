//! Printable payment receipts. A receipt is a self-contained HTML page written
//! to the receipts folder and opened in the default browser, which shows the
//! print dialog. It works offline and prints on letter paper or a narrow
//! receipt printer.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow};
use chrono::NaiveDate;

use super::Snapshot;
use crate::domain::ledger::{delinquency, owed_at_payment};
use crate::domain::loans::Frequency;
use crate::domain::money::{Cents, Rate, percent_label};
use crate::i18n::{self, t};
use crate::tr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptData {
    pub payment_id: i64,
    pub business: String,
    pub client_name: String,
    pub client_phone: Option<String>,
    pub loan_id: i64,
    pub rate: Rate,
    pub frequency: Frequency,
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
    pub next_due: Option<(NaiveDate, Cents)>,
    pub notes: String,
    pub reversed: Option<String>,
}

impl ReceiptData {
    pub fn owed_before(&self) -> Cents {
        self.principal_before + self.interest_before
    }
    pub fn owed_after(&self) -> Cents {
        self.principal_after + self.interest_after
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
    let facts: Vec<_> = history[..=upto].iter().map(|p| p.fact()).collect();
    let after = owed_at_payment(&book.facts, &facts, payment.paid_on);
    let next = delinquency(&book.facts, &facts, payment.paid_on);
    let next_due = (after.payoff() > 0).then_some(next.next_due.map(|d| (d, next.next_due_amount))).flatten();

    Some(ReceiptData {
        payment_id,
        business: snap.settings.business_name.clone(),
        client_name: client.full_name.clone(),
        client_phone: client.phone.clone(),
        loan_id: loan.id,
        rate: loan.terms.rate,
        frequency: loan.terms.frequency,
        paid_on: payment.paid_on,
        amount: payment.amount,
        interest: payment.interest,
        principal: payment.principal,
        fee: payment.fee,
        original_principal: loan.terms.principal,
        principal_before: after.principal_outstanding + payment.principal,
        interest_before: after.interest_outstanding + payment.interest,
        principal_after: after.principal_outstanding,
        interest_after: after.interest_outstanding,
        next_due,
        notes: payment.notes.clone(),
        reversed: payment.reversal_reason.clone().filter(|_| payment.is_reversed()),
    })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The receipt as a complete HTML document in the current language.
pub fn render(d: &ReceiptData, snap: &Snapshot) -> String {
    let money = snap.settings.money();
    let ds = snap.settings.date_style;
    let m = |c: Cents| esc(&money.format(c));
    let repaid = d.original_principal - d.principal_after;
    let repaid_pct = percent_label(repaid, d.original_principal).unwrap_or_else(|| "0.0%".into());
    let remaining_pct = percent_label(d.principal_after, d.original_principal).unwrap_or_else(|| "0.0%".into());
    let bar = if d.original_principal > 0 { (repaid as f64 / d.original_principal as f64 * 100.0).clamp(0.0, 100.0) } else { 0.0 };
    let share = percent_label(d.amount - d.fee, d.owed_before()).unwrap_or_default();
    let number = format!("{:06}", d.payment_id);
    let rate = format!("{} {}", d.rate.percent_label(), d.frequency.per());
    let row = |label: &str, value: String, class: &str| format!("<tr class=\"{class}\"><td>{}</td><td>{value}</td></tr>", esc(label));

    let mut lines = Vec::new();
    lines.push(row(t("Amount paid", "Monto pagado"), m(d.amount), "total"));
    lines.push(row(t("Interest", "Interés"), m(d.interest), "sub"));
    lines.push(row(t("Capital", "Abono a capital"), m(d.principal), "sub"));
    if d.fee > 0 {
        lines.push(row(t("Fee", "Cargo"), m(d.fee), "sub"));
    }
    let paid_table = lines.join("");

    let mut bal = Vec::new();
    bal.push(row(t("Capital before this payment", "Capital antes de este pago"), m(d.principal_before), ""));
    bal.push(row(t("Capital remaining", "Capital pendiente"), m(d.principal_after), ""));
    if d.interest_after > 0 {
        bal.push(row(t("Interest pending", "Interés pendiente"), m(d.interest_after), ""));
    }
    bal.push(row(t("Total owed now", "Total adeudado"), m(d.owed_after()), "total"));
    let balance_table = bal.join("");

    let next = match d.next_due {
        Some((date, amount)) if d.owed_after() > 0 => format!(
            "<p class=\"next\">{} <b>{}</b> · {} <b>{}</b></p>",
            esc(t("Next payment:", "Próximo pago:")),
            esc(&ds.format(date)),
            esc(t("interest", "interés")),
            m(amount)
        ),
        _ => format!(
            "<p class=\"next done\">{}</p>",
            esc(t("This loan is fully paid. Thank you!", "Este préstamo quedó saldado. ¡Gracias!"))
        ),
    };
    let void = d.reversed.as_ref().map(|r| format!("<div class=\"void\">{}<span>{}</span></div>", esc(t("VOID", "ANULADO")), esc(r)));
    let phone = d.client_phone.as_ref().map(|p| format!("<div class=\"muted\">{}</div>", esc(p))).unwrap_or_default();
    let notes = if d.notes.trim().is_empty() {
        String::new()
    } else {
        format!("<p class=\"muted\">{} {}</p>", esc(t("Note:", "Nota:")), esc(&d.notes))
    };
    let lang = i18n::get().key();
    let title = tr!("Receipt {number}", "Recibo {number}");
    let share_line = tr!("This payment covered {share} of the balance owed.", "Este pago cubrió el {share} de lo adeudado.");

    format!(
        r#"<!doctype html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
  :root {{ --ink:#17191c; --muted:#666c77; --rule:#e9ebef; --accent:#365cf5; --pos:#137a60; }}
  * {{ box-sizing: border-box; }}
  body {{ margin:0; background:#f7f8fa; color:var(--ink); font: 14px/1.45 "Segoe UI", system-ui, sans-serif; font-variant-numeric: tabular-nums; }}
  .sheet {{ max-width: 380px; margin: 24px auto; background:#fff; border:1px solid var(--rule); border-radius:12px; padding:24px; }}
  h1 {{ font-size:20px; margin:0 0 2px; letter-spacing:-0.01em; }}
  .kind {{ color:var(--muted); margin:0 0 16px; }}
  .meta {{ display:flex; justify-content:space-between; gap:12px; padding:12px 0; border-top:1px solid var(--rule); border-bottom:1px solid var(--rule); margin-bottom:12px; }}
  .meta b {{ display:block; }}
  .muted {{ color:var(--muted); font-size:13px; }}
  table {{ width:100%; border-collapse:collapse; margin:8px 0 12px; }}
  td {{ padding:4px 0; }}
  td:last-child {{ text-align:right; white-space:nowrap; }}
  tr.sub td {{ color:var(--muted); padding-left:12px; }}
  tr.total td {{ font-weight:600; font-size:15px; padding-top:6px; }}
  .section {{ border-top:1px solid var(--rule); padding-top:10px; }}
  .bar {{ height:8px; border-radius:4px; background:#eef0f4; overflow:hidden; margin:8px 0 6px; }}
  .bar i {{ display:block; height:100%; background:var(--accent); width:{bar:.1}%; }}
  .pcts {{ display:flex; justify-content:space-between; font-size:13px; }}
  .pcts b {{ font-size:15px; }}
  .next {{ margin:14px 0 4px; padding:10px 12px; background:#f3f5ff; border-radius:8px; }}
  .next.done {{ background:#e7f5f0; color:var(--pos); font-weight:600; }}
  .void {{ border:2px solid #c2353d; color:#c2353d; border-radius:8px; text-align:center; font-weight:700; font-size:18px; padding:6px; margin-bottom:12px; }}
  .void span {{ display:block; font-size:12px; font-weight:400; }}
  .thanks {{ text-align:center; color:var(--muted); margin-top:16px; font-size:13px; }}
  .actions {{ text-align:center; margin: 0 auto 24px; }}
  button {{ font: inherit; background:var(--accent); color:#fff; border:0; border-radius:8px; padding:8px 18px; cursor:pointer; }}
  @media print {{
    body {{ background:#fff; }}
    .sheet {{ border:0; margin:0 auto; padding:0; max-width: 100%; }}
    .actions {{ display:none; }}
    @page {{ margin: 10mm; }}
  }}
</style>
</head>
<body>
<div class="sheet">
  {void}
  <h1>{business}</h1>
  <p class="kind">{kind} · {no} {number}</p>
  <div class="meta">
    <div><span class="muted">{client_l}</span><b>{client}</b>{phone}</div>
    <div style="text-align:right"><span class="muted">{date_l}</span><b>{date}</b><span class="muted">{loan_l} #{loan_id} · {rate}</span></div>
  </div>
  <table>{paid_table}</table>
  <div class="section">
    <table>{balance_table}</table>
  </div>
  <div class="section">
    <div class="bar"><i></i></div>
    <div class="pcts">
      <div>{repaid_l}<br><b>{repaid_pct}</b></div>
      <div style="text-align:right">{remaining_l}<br><b>{remaining_pct}</b></div>
    </div>
    <p class="muted">{share_line}</p>
  </div>
  {next}
  {notes}
  <p class="thanks">{thanks}</p>
</div>
<div class="actions"><button onclick="window.print()">{print}</button></div>
<script>window.addEventListener('load', function () {{ setTimeout(function () {{ window.print(); }}, 250); }});</script>
</body>
</html>
"#,
        void = void.unwrap_or_default(),
        business = esc(&d.business),
        kind = esc(t("Payment receipt", "Recibo de pago")),
        no = esc(t("No.", "No.")),
        client_l = esc(t("Client", "Cliente")),
        client = esc(&d.client_name),
        date_l = esc(t("Date", "Fecha")),
        date = esc(&ds.format(d.paid_on)),
        loan_l = esc(t("Loan", "Préstamo")),
        loan_id = d.loan_id,
        rate = esc(&rate),
        repaid_l = esc(t("Capital paid", "Capital pagado")),
        remaining_l = esc(t("Capital remaining", "Capital pendiente")),
        share_line = esc(&share_line),
        thanks = esc(t("Thank you for your payment.", "Gracias por su pago.")),
        print = esc(t("Print", "Imprimir")),
    )
}

/// Writes the receipt for `payment_id` into `dir` and returns its path.
pub fn write(snap: &Snapshot, payment_id: i64, dir: &Path) -> Result<PathBuf> {
    let d = data(snap, payment_id).ok_or_else(|| anyhow!("payment {payment_id} not found"))?;
    std::fs::create_dir_all(dir).with_context(|| format!("Could not create {}", dir.display()))?;
    let path = dir.join(format!("receipt-{:06}.html", payment_id));
    std::fs::write(&path, render(&d, snap)).with_context(|| format!("Could not write {}", path.display()))?;
    Ok(path)
}
