use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ncolumn;
use crate::app::{Route, VzApp};
use crate::i18n::{count, t};
use crate::tr;
use crate::ui::components::{Tone, avatar, card, empty_state, icon, page_header, pill, table_header, table_row, th};
use crate::ui::theme::{palette, size, space, tabular};

impl VzApp {
    pub(crate) fn render_payments(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let narrow = self.content_width(window) < 1000.;
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let q = self.payment_search.read(cx).value().to_lowercase();
        let q = q.trim().to_string();
        let live: Vec<_> = self.snap.payments.iter().filter(|x| !x.is_reversed()).collect();
        let received: i64 = live.iter().map(|x| x.amount).sum();
        let income: i64 = live.iter().map(|x| x.interest + x.fee).sum();
        let rows: Vec<_> = self
            .snap
            .payments
            .iter()
            .rev()
            .filter(|x| {
                if q.is_empty() {
                    return true;
                }
                let client = self.snap.loan(x.loan_id).map(|l| self.snap.client_name(l.client_id)).unwrap_or_default().to_lowercase();
                client.contains(&q) || x.notes.to_lowercase().contains(&q) || format!("#{}", x.loan_id).contains(&q)
            })
            .collect();

        let n = count(live.len(), "payment", "payments", "pago", "pagos");
        let received = money.format(received);
        let header = page_header(
            &p,
            t("Payments", "Pagos"),
            Some(
                h_flex()
                    .gap(px(4.))
                    .font_features(tabular())
                    .child(tr!("{n} · {received} received, of which", "{n} · {received} recibido, de los cuales"))
                    .child(div().text_color(p.positive).child(money.format(income)))
                    .child(t("is interest", "son interés"))
                    .into_any_element(),
            ),
            vec![
                Button::new("pay-new")
                    .primary()
                    .icon(IconName::Plus)
                    .label(t("Record payment", "Registrar pago"))
                    .tooltip("Ctrl+P")
                    .on_click(cx.listener(|this, _, w, cx| this.open_payment_form(None, w, cx)))
                    .into_any_element(),
            ],
        );

        let body = if self.snap.payments.is_empty() {
            empty_state(
                &p,
                IconName::Receipt,
                t("No payments yet", "Aún no hay pagos"),
                t(
                    "Record what a client pays you. Each payment is split into interest (your income) and capital (your money coming back).",
                    "Registra lo que te paga un cliente. Cada pago se divide en interés (tu ganancia) y capital (tu dinero que regresa).",
                ),
                (!self.snap.loans.is_empty()).then(|| {
                    Button::new("pay-empty")
                        .primary()
                        .label(t("Record payment", "Registrar pago"))
                        .on_click(cx.listener(|this, _, w, cx| this.open_payment_form(None, w, cx)))
                        .into_any_element()
                }),
            )
            .into_any_element()
        } else if rows.is_empty() {
            empty_state(&p, IconName::SearchX, t("No matching payments", "Ningún pago coincide"), &tr!("Nothing matches “{q}”.", "Nada coincide con “{q}”."), None)
                .into_any_element()
        } else {
            let items = rows.iter().map(|x| {
                let loan = self.snap.loan(x.loan_id);
                let cid = loan.map(|l| l.client_id).unwrap_or(0);
                let name = self.snap.client_name(cid).to_string();
                let lid = x.loan_id;
                let pid = x.id;
                let rev = x.is_reversed();
                let dim = |d: Div| if rev { d.text_color(p.muted).line_through() } else { d };
                let sub = if rev {
                    let why = x.reversal_reason.clone().unwrap_or_default();
                    tr!("Loan #{lid} · reversed: {why}", "Préstamo #{lid} · anulado: {why}")
                } else if x.notes.is_empty() {
                    tr!("Loan #{lid}", "Préstamo #{lid}")
                } else {
                    let note = &x.notes;
                    tr!("Loan #{lid} · {note}", "Préstamo #{lid} · {note}")
                };
                table_row(&p, ("payrow", pid as u64), true)
                    .font_features(tabular())
                    .child(div().w(px(120.)).text_color(if rev { p.muted } else { p.text }).child(ds.format(x.paid_on)))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(180.))
                            .gap(px(space::M))
                            .child(avatar(&p, &name, cid, 28.))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .child(div().truncate().child(name))
                                    .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sub)),
                            ),
                    )
                    .child(dim(ncolumn(110.).text_color(p.muted)).child(money.format(x.interest)))
                    .child(dim(ncolumn(120.).text_color(p.muted)).child(money.format(x.principal)))
                    .when(!narrow, |d| d.child(dim(ncolumn(90.).text_color(p.muted)).child(if x.fee > 0 { money.format(x.fee) } else { "—".into() })))
                    .child(dim(ncolumn(130.)).child(money.format(x.amount)))
                    .child(
                        h_flex()
                            .w(px(if narrow { 110. } else { 190. }))
                            .justify_end()
                            .gap(px(6.))
                            .when(rev && !narrow, |d| d.child(pill(&p, t("Reversed", "Anulado"), Tone::Neutral)))
                            .child(
                                Button::new(("pay-receipt", pid as u64))
                                    .ghost()
                                    .small()
                                    .icon(IconName::Printer)
                                    .label(t("Receipt", "Recibo"))
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        cx.stop_propagation();
                                        this.open_receipt(pid, w, cx)
                                    })),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Loan(lid), w, cx)))
            });
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().w(px(120.)).child(th(&p, t("Date", "Fecha"))))
                        .child(div().flex_1().min_w(px(180.)).child(th(&p, t("Client", "Cliente"))))
                        .child(ncolumn(110.).child(th(&p, t("Interest", "Interés"))))
                        .child(ncolumn(120.).child(th(&p, t("Capital", "Capital"))))
                        .when(!narrow, |d| d.child(ncolumn(90.).child(th(&p, t("Fee", "Cargo")))))
                        .child(ncolumn(130.).child(th(&p, t("Amount", "Monto"))))
                        .child(div().w(px(if narrow { 110. } else { 190. }))),
                )
                .children(items)
                .into_any_element()
        };

        v_flex()
            .child(header)
            .child(
                div()
                    .pb(px(space::L))
                    .child(div().w(px(380.)).child(Input::new(&self.payment_search).prefix(icon(IconName::Search, p.faint, 14.)).cleanable(true))),
            )
            .child(card(&p).overflow_hidden().child(body))
            .into_any_element()
    }
}
