//! The installment table and late fees on an installment loan's page.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ncolumn;
use crate::app::VzApp;
use crate::domain::installments::{InstallmentStatus, PlanState};
use crate::i18n::t;
use crate::tr;
use crate::ui::components::{Tone, card, card_header, pill, sel, table_header, th};
use crate::ui::forms::reason::ReasonKind;
use crate::ui::theme::{Palette, size, space, tabular};

impl VzApp {
    /// Every installment with what it owes, what was paid and its status;
    /// overdue and partly paid ones are highlighted.
    pub(crate) fn installments_card(&self, p: &Palette, state: &PlanState, today: chrono::NaiveDate) -> AnyElement {
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let total_interest = state.interest_total();
        let total = state.principal + total_interest + state.fees_assessed();
        let rows = state.installments.iter().map(|x| {
            let st = x.status(today);
            let (tone, tint) = match st {
                InstallmentStatus::Paid => (Tone::Positive, None),
                InstallmentStatus::Overdue => (Tone::Negative, Some(p.negative_tint)),
                InstallmentStatus::PartlyPaid => (Tone::Warning, Some(p.warning_tint)),
                InstallmentStatus::Due => (Tone::Accent, None),
                InstallmentStatus::Upcoming => (Tone::Neutral, None),
            };
            let fee_text = if x.fee > 0 {
                money.format(x.fee)
            } else if x.fee_waived && x.fee_assessed_on.is_some() {
                t("waived", "perdonado").to_string()
            } else {
                "—".to_string()
            };
            h_flex()
                .h(px(40.))
                .px(px(space::XL))
                .gap(px(space::M))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .when_some(tint, |d, c| d.bg(c))
                .when(st == InstallmentStatus::Paid, |d| d.text_color(p.muted))
                .child(div().w(px(24.)).text_color(p.faint).child(sel(x.seq.to_string())))
                .child(div().flex_1().min_w(px(90.)).truncate().child(sel(ds.format(x.due))))
                .child(ncolumn(100.).child(sel(money.format(x.principal))))
                .child(ncolumn(90.).child(sel(money.format(x.interest))))
                .child(ncolumn(80.).text_color(if x.fee > 0 { p.negative } else { p.faint }).child(sel(fee_text)))
                .child(ncolumn(100.).font_weight(FontWeight::MEDIUM).child(sel(money.format(x.total_due()))))
                .child(ncolumn(100.).text_color(p.muted).child(sel(money.format(x.paid()))))
                .child(ncolumn(100.).child(sel(money.format(x.remaining().max(0)))))
                .child(div().w(px(96.)).flex().justify_end().child(pill(p, st.label(), tone)))
        });
        let (i, tot) = (money.format(total_interest), money.format(total));
        card(p)
            .overflow_hidden()
            .child(card_header(
                p,
                t("Installments", "Cuotas"),
                Some(
                    div()
                        .text_size(px(size::META))
                        .text_color(p.muted)
                        .child(sel(tr!("Interest {i} · total {tot}", "Interés {i} · total {tot}")))
                        .into_any_element(),
                ),
            ))
            .child(
                table_header(p)
                    .gap(px(space::M))
                    .child(div().w(px(24.)).child(th(p, "#")))
                    .child(div().flex_1().min_w(px(90.)).child(th(p, t("Due date", "Vence"))))
                    .child(ncolumn(100.).child(th(p, t("Capital", "Capital"))))
                    .child(ncolumn(90.).child(th(p, t("Interest", "Interés"))))
                    .child(ncolumn(80.).child(th(p, t("Late fee", "Mora"))))
                    .child(ncolumn(100.).child(th(p, t("Total due", "Total"))))
                    .child(ncolumn(100.).child(th(p, t("Paid", "Pagado"))))
                    .child(ncolumn(100.).child(th(p, t("Outstanding", "Pendiente"))))
                    .child(ncolumn(96.).child(th(p, t("Status", "Estado")))),
            )
            .child(div().id("installments-scroll").max_h(px(460.)).overflow_y_scroll().children(rows))
            .into_any_element()
    }

    /// Late fees assessed on this loan, with their base and whether they were
    /// paid or waived.
    pub(crate) fn late_fees_card(&self, p: &Palette, loan_id: i64, cx: &mut Context<Self>) -> Option<AnyElement> {
        let fees: Vec<_> = self.snap.late_fees.iter().filter(|f| f.loan_id == loan_id).cloned().collect();
        if fees.is_empty() {
            return None;
        }
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let state = self.snap.book_loan(loan_id).and_then(|b| b.plan_state(self.today));
        let rows = fees.iter().map(|f| {
            let seq = f.seq;
            let paid = state.as_ref().and_then(|s| s.installments.iter().find(|x| x.seq == seq)).map(|x| x.paid_fee).unwrap_or(0);
            let (status, tone) = if f.is_waived() {
                (t("Waived", "Perdonado"), Tone::Neutral)
            } else if paid >= f.amount {
                (t("Paid", "Pagado"), Tone::Positive)
            } else if paid > 0 {
                (t("Partly paid", "Pago parcial"), Tone::Warning)
            } else {
                (t("Unpaid", "Sin pagar"), Tone::Negative)
            };
            let (rate, base) = (f.rate.percent_label(), money.format(f.base_amount));
            let detail = tr!("{rate} of {base} unpaid", "{rate} de {base} pendiente");
            let detail = if f.is_waived() && !f.waive_reason.is_empty() { format!("{detail} · {}", f.waive_reason) } else { detail };
            let action = if f.is_waived() {
                Button::new(("fee-restore", seq as u64))
                    .ghost()
                    .small()
                    .label(t("Restore", "Restaurar"))
                    .on_click(cx.listener(move |this, _, w, cx| this.restore_late_fee(loan_id, seq, w, cx)))
            } else {
                Button::new(("fee-waive", seq as u64))
                    .ghost()
                    .small()
                    .label(t("Waive", "Perdonar"))
                    .on_click(cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::WaiveFee(loan_id, seq), w, cx)))
            };
            h_flex()
                .min_h(px(44.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .py(px(space::S))
                .child(v_flex().flex_1().min_w_0().child(sel(tr!("Installment {seq}", "Cuota {seq}"))).child(
                    div().text_size(px(size::META)).text_color(p.muted).child(sel({
                        let when = ds.format(f.assessed_on);
                        format!("{when} · {detail}")
                    })),
                ))
                .child(ncolumn(110.).when(f.is_waived(), |d| d.line_through().text_color(p.muted)).child(sel(money.format(f.amount))))
                .child(div().w(px(100.)).flex().justify_end().child(pill(p, status, tone)))
                .child(div().w(px(90.)).flex().justify_end().child(action))
        });
        Some(
            card(p)
                .overflow_hidden()
                .child(card_header(
                    p,
                    t("Late fees", "Cargos por mora"),
                    Some(
                        div()
                            .text_size(px(size::META))
                            .text_color(p.muted)
                            .child(t("Charged once per overdue installment", "Una vez por cuota atrasada"))
                            .into_any_element(),
                    ),
                ))
                .child(v_flex().children(rows))
                .into_any_element(),
        )
    }

    fn restore_late_fee(&mut self, loan_id: i64, seq: u32, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.set_late_fee_waived(loan_id, seq, false, "", self.today) {
            Ok(()) => {
                self.reload(cx);
                self.notify_success(t("Late fee restored", "Cargo por mora restaurado"), window, cx);
            }
            Err(e) => {
                let m = self.error_message(&e);
                self.notify_error(m, window, cx);
            }
        }
    }
}
