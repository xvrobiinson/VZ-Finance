use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ncolumn;
use crate::app::{Route, VzApp};
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
        let interest: i64 = live.iter().map(|x| x.interest + x.fee).sum();
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

        let header = page_header(
            &p,
            "Payments",
            Some(
                h_flex()
                    .gap(px(4.))
                    .font_features(tabular())
                    .child(format!("{} payment{} · {} received, of which", live.len(), if live.len() == 1 { "" } else { "s" }, money.format(received)))
                    .child(div().text_color(p.positive).child(money.format(interest)))
                    .child("is income")
                    .into_any_element(),
            ),
            vec![
                Button::new("pay-new")
                    .primary()
                    .icon(IconName::Plus)
                    .label("Record payment")
                    .tooltip("Ctrl+P")
                    .on_click(cx.listener(|this, _, w, cx| this.open_payment_form(None, w, cx)))
                    .into_any_element(),
            ],
        );

        let body = if self.snap.payments.is_empty() {
            empty_state(
                &p,
                IconName::Receipt,
                "No payments yet",
                "Record what a client pays you. VZ Finance splits it into interest (income) and principal (capital returned).",
                (!self.snap.loans.is_empty()).then(|| {
                    Button::new("pay-empty")
                        .primary()
                        .label("Record payment")
                        .on_click(cx.listener(|this, _, w, cx| this.open_payment_form(None, w, cx)))
                        .into_any_element()
                }),
            )
            .into_any_element()
        } else if rows.is_empty() {
            empty_state(&p, IconName::SearchX, "No matching payments", &format!("Nothing matches “{q}”."), None).into_any_element()
        } else {
            let items = rows.iter().map(|x| {
                let loan = self.snap.loan(x.loan_id);
                let cid = loan.map(|l| l.client_id).unwrap_or(0);
                let name = self.snap.client_name(cid).to_string();
                let lid = x.loan_id;
                let rev = x.is_reversed();
                let dim = |d: Div| if rev { d.text_color(p.muted).line_through() } else { d };
                table_row(&p, ("payrow", x.id as u64), true)
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
                                    .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(if rev {
                                        format!("Loan #{lid} · reversed: {}", x.reversal_reason.clone().unwrap_or_default())
                                    } else if x.notes.is_empty() {
                                        format!("Loan #{lid}")
                                    } else {
                                        format!("Loan #{lid} · {}", x.notes)
                                    })),
                            ),
                    )
                    .child(dim(ncolumn(120.).text_color(p.muted)).child(money.format(x.principal)))
                    .child(dim(ncolumn(110.).text_color(p.muted)).child(money.format(x.interest)))
                    .when(!narrow, |d| d.child(dim(ncolumn(90.).text_color(p.muted)).child(if x.fee > 0 { money.format(x.fee) } else { "—".into() })))
                    .child(dim(ncolumn(130.)).child(money.format(x.amount)))
                    .when(!narrow, |d| d.child(div().w(px(90.)).flex().justify_end().when(rev, |d| d.child(pill(&p, "Reversed", Tone::Neutral)))))
                    .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Loan(lid), w, cx)))
            });
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().w(px(120.)).child(th(&p, "Date")))
                        .child(div().flex_1().min_w(px(180.)).child(th(&p, "Client")))
                        .child(ncolumn(120.).child(th(&p, "Principal")))
                        .child(ncolumn(110.).child(th(&p, "Interest")))
                        .when(!narrow, |d| d.child(ncolumn(90.).child(th(&p, "Fee"))))
                        .child(ncolumn(130.).child(th(&p, "Amount")))
                        .when(!narrow, |d| d.child(div().w(px(90.)))),
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
