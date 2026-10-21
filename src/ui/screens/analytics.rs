use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::{ncolumn, segmented};
use crate::app::VzApp;
use crate::charts::bars::column_chart;
use crate::domain::ledger::DisplayStatus;
use crate::domain::money::percent_label;
use crate::domain::portfolio::CapitalKind;
use crate::services::settings::ChartRange;
use crate::ui::components::{card, card_header, empty_state, figure, kv, label, page_header, table_header, th, vrule};
use crate::ui::theme::{Palette, palette, size, space, tabular};

impl VzApp {
    pub(crate) fn render_analytics(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let narrow = self.content_width(window) < 1000.;
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let today = self.today;
        let s = self.snap.book.summary(today);
        let invested = s.net_invested();
        let gain = s.gain();
        let roic = percent_label(gain, invested);

        let header = page_header(
            &p,
            "Portfolio Analytics",
            Some("Every figure is computed from your recorded capital, loans and payments.".into_any_element()),
            vec![
                Button::new("an-withdraw")
                    .label("Withdraw capital")
                    .on_click(cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Withdrawal, w, cx)))
                    .into_any_element(),
                Button::new("an-add")
                    .primary()
                    .icon(IconName::Plus)
                    .label("Add capital")
                    .on_click(cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Contribution, w, cx)))
                    .into_any_element(),
            ],
        );

        let cell = |l: &str, v: String, sub: String, color: Hsla| {
            v_flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .px(px(space::XL))
                .py(px(space::L + 2.))
                .gap(px(6.))
                .child(label(&p, l.to_string()).truncate())
                .child(figure(&p, v, size::FIGURE).text_color(color))
                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sub))
        };
        let gain_color = match gain.signum() {
            1 => p.positive,
            -1 => p.negative,
            _ => p.text,
        };
        let returns = card(&p)
            .flex_row()
            .child(cell("Portfolio value", money.format(s.portfolio_value), "Cash + principal out + receivable".into(), p.text))
            .child(vrule(&p))
            .child(cell("Portfolio gain", money.format_signed(gain), "Value minus capital invested".into(), gain_color))
            .child(vrule(&p))
            .child(cell(
                "Return on invested capital",
                roic.clone().map(|r| if gain > 0 { format!("+{r}") } else { r }).unwrap_or_else(|| "—".into()),
                "Simple, not annualized".into(),
                gain_color,
            ))
            .child(vrule(&p))
            .child(cell("Realized income", money.format(s.realized_income()), "Interest and fees received".into(), p.text));
        let capital = card(&p)
            .flex_row()
            .child(cell("Capital invested", money.format(invested), format!("{} in · {} out", money.format(s.contributions), money.format(s.withdrawals)), p.text))
            .child(vrule(&p))
            .child(cell("Available cash", money.format(s.available_cash), "Not lent out".into(), p.text))
            .child(vrule(&p))
            .child(cell("Interest receivable", money.format(s.interest_receivable), "Accrued, unpaid, performing".into(), p.text))
            .child(vrule(&p))
            .child(cell("Expected interest remaining", money.format(s.expected_interest_remaining), "On schedule, active loans".into(), p.text))
            .child(vrule(&p))
            .child(cell(
                "Defaulted balance",
                money.format(s.defaulted_principal + s.defaulted_interest),
                "Excluded from value".into(),
                if s.defaulted_principal > 0 { p.negative } else { p.text },
            ));

        // Value chart.
        let range = self.settings().chart_range;
        let points = self.chart_points(range);
        let options: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let control = segmented(&p, "an-range", &options, range, cx, |app, r, w, cx| app.update_settings(|s| s.chart_range = r, w, cx));
        let chart_body = if points.is_empty() {
            empty_state(&p, IconName::ChartLine, "No history yet", "Record capital and loans to see portfolio value over time.", None).into_any_element()
        } else {
            div().px(px(space::XL)).pb(px(space::L)).child(self.growth_chart(&p, &points, super::overview::ChartSlot::Analytics, 300., cx)).into_any_element()
        };
        let value_card = card(&p).child(card_header(&p, "Portfolio value", Some(control.into_any_element()))).child(chart_body);

        // Monthly interest, last 12 months.
        let flows = self.snap.book.monthly_cash_flow(today);
        let last12: Vec<_> = flows.iter().rev().take(12).rev().collect();
        let labels: Vec<SharedString> = last12.iter().map(|f| f.month.format("%b").to_string().into()).collect();
        let values: Vec<i64> = last12.iter().map(|f| f.interest_received + f.fees_received).collect();
        let interest_card = card(&p)
            .flex_1()
            .flex_basis(px(0.))
            .min_w_0()
            .child(card_header(&p, "Interest received by month", Some(div().text_size(px(size::META)).text_color(p.muted).child("Including fees").into_any_element())))
            .child(if last12.is_empty() {
                empty_state(&p, IconName::ChartColumn, "No income yet", "Monthly interest appears once payments come in.", None).into_any_element()
            } else {
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::L))
                    .child(column_chart(&p, &labels, &values, true, &money, 260.))
                    .child(div().h(px(1.)).bg(p.border))
                    .child(kv(&p, "Last 12 months", money.format(values.iter().sum::<i64>())))
                    .child(kv(&p, "Monthly average", money.format(values.iter().sum::<i64>() / values.len().max(1) as i64)))
                    .into_any_element()
            });

        let deploy_card = self.deployment_card(&p);

        // Monthly cash flow table, newest first.
        let flow_rows = flows.iter().rev().map(|f| {
            let net = f.net();
            h_flex()
                .h(px(40.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .child(div().flex_1().flex_basis(px(0.)).min_w(px(96.)).child(f.month.format(if narrow { "%b %Y" } else { "%B %Y" }).to_string()))
                .child(fcol().text_color(p.muted).child(dash(&money, f.contributions)))
                .child(fcol().text_color(p.muted).child(dash_neg(&money, f.withdrawals)))
                .child(fcol().text_color(p.muted).child(dash_neg(&money, f.disbursed)))
                .child(fcol().text_color(p.muted).child(dash(&money, f.principal_returned)))
                .child(fcol().text_color(if f.interest_received + f.fees_received > 0 { p.positive } else { p.muted }).child(dash(&money, f.interest_received + f.fees_received)))
                .child(fcol().child(money.format_signed(net)))
                .child(fcol().child(money.format(f.ending_cash)))
        });
        let flow_card = card(&p)
            .overflow_hidden()
            .child(card_header(&p, "Monthly cash flow", None))
            .child(if flows.is_empty() {
                empty_state(&p, IconName::Table, "No cash movements yet", "Capital, loans and payments are summarised here month by month.", None).into_any_element()
            } else {
                v_flex()
                    .child(
                        table_header(&p)
                            .child(div().flex_1().flex_basis(px(0.)).min_w(px(96.)).child(th(&p, "Month")))
                            .child(fcol().child(th(&p, "Capital in")))
                            .child(fcol().child(th(&p, "Capital out")))
                            .child(fcol().child(th(&p, "Lent")))
                            .child(fcol().child(th(&p, "Principal back")))
                            .child(fcol().child(th(&p, "Income")))
                            .child(fcol().child(th(&p, "Net cash flow")))
                            .child(fcol().child(th(&p, "Ending cash"))),
                    )
                    .children(flow_rows)
                    .into_any_element()
            });

        // Capital ledger.
        let cap_rows = self.snap.capital.iter().rev().map(|c| {
            let sign = match c.kind {
                CapitalKind::Contribution => money.format(c.amount),
                CapitalKind::Withdrawal => money.format(-c.amount),
            };
            h_flex()
                .h(px(44.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .child(div().w(px(130.)).font_features(tabular()).child(ds.format(c.occurred_on)))
                .child(div().w(px(150.)).text_color(p.muted).child(c.kind.label()))
                .child(div().flex_1().min_w_0().truncate().text_color(p.muted).child(c.notes.clone()))
                .child(ncolumn(140.).font_features(tabular()).child(sign))
        });
        let ledger_card = card(&p)
            .overflow_hidden()
            .child(card_header(&p, "Capital ledger", None))
            .child(if self.snap.capital.is_empty() {
                empty_state(
                    &p,
                    IconName::Landmark,
                    "No capital recorded",
                    "Record the money you set aside for lending. Contributions are not profit; they are your invested capital.",
                    Some(
                        Button::new("ledger-add")
                            .primary()
                            .label("Add capital")
                            .on_click(cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Contribution, w, cx)))
                            .into_any_element(),
                    ),
                )
                .into_any_element()
            } else {
                v_flex().children(cap_rows).into_any_element()
            });

        v_flex()
            .child(header)
            .child(
                v_flex()
                    .gap(px(space::XL))
                    .child(returns)
                    .child(capital)
                    .child(value_card)
                    .child(if narrow { v_flex().gap(px(space::XL)).child(interest_card).child(deploy_card).into_any_element() } else { h_flex().items_stretch().gap(px(space::XL)).child(interest_card).child(deploy_card).into_any_element() })
                    .child(flow_card)
                    .child(ledger_card),
            )
            .into_any_element()
    }

    fn deployment_card(&self, p: &Palette) -> Div {
        let money = self.settings().money();
        let s = self.snap.book.summary(self.today);
        let deployed = s.outstanding_principal;
        let pool = deployed + s.available_cash.max(0);
        let ratio = if pool > 0 { deployed as f32 / pool as f32 } else { 0.0 };
        let mut counts = [(DisplayStatus::Active, 0usize), (DisplayStatus::DueSoon, 0), (DisplayStatus::Overdue, 0), (DisplayStatus::PaidOff, 0), (DisplayStatus::Defaulted, 0)];
        for l in &self.snap.book.loans {
            let st = l.display_status(self.today);
            if let Some(c) = counts.iter_mut().find(|(k, _)| *k == st) {
                c.1 += 1;
            }
        }
        let total: usize = counts.iter().map(|c| c.1).sum();
        let colors = |st: DisplayStatus| match st {
            DisplayStatus::Active => p.accent,
            DisplayStatus::DueSoon => p.warning,
            DisplayStatus::Overdue => p.negative,
            DisplayStatus::PaidOff => p.positive,
            _ => p.chart_reference,
        };
        let bar = h_flex().h(px(10.)).w_full().rounded(px(3.)).overflow_hidden().gap(px(2.)).bg(p.neutral_tint).children(
            counts
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(st, n)| div().h_full().flex_basis(px(0.)).flex_grow(*n as f32).bg(colors(*st))),
        );
        let legend = counts.iter().map(|(st, n)| {
            h_flex()
                .min_h(px(30.))
                .gap(px(10.))
                .child(div().size(px(8.)).rounded(px(2.)).bg(colors(*st)))
                .child(div().flex_1().text_color(p.muted).child(st.label()))
                .child(div().font_features(tabular()).child(n.to_string()))
        });
        card(p)
            .flex_1()
            .flex_basis(px(0.))
            .min_w_0()
            .child(card_header(p, "Capital deployment", None))
            .child(
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::M))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_end()
                            .child(
                                v_flex()
                                    .gap(px(2.))
                                    .child(label(p, "Principal deployed"))
                                    .child(figure(p, percent_label(deployed, pool).unwrap_or_else(|| "—".into()), size::FIGURE)),
                            )
                            .child(div().text_color(p.muted).font_features(tabular()).child(format!("{} of {}", money.format(deployed), money.format(pool)))),
                    )
                    .child(crate::ui::components::progress_bar(p, ratio, p.accent))
                    .child(kv(p, "Available to lend", money.format(s.available_cash)))
                    .child(div().h(px(1.)).bg(p.border))
                    .child(div().pt(px(4.)).font_weight(FontWeight::MEDIUM).child(format!("Loans by status · {total}")))
                    .child(bar)
                    .child(v_flex().children(legend)),
            )
    }
}

/// An equal-width, right-aligned cash-flow column.
fn fcol() -> Div {
    div().flex_1().flex_basis(px(0.)).min_w_0().text_right().whitespace_nowrap()
}

fn dash(m: &crate::domain::money::MoneyFormat, v: i64) -> String {
    if v == 0 { "—".into() } else { m.format(v) }
}

fn dash_neg(m: &crate::domain::money::MoneyFormat, v: i64) -> String {
    if v == 0 { "—".into() } else { m.format(-v) }
}
