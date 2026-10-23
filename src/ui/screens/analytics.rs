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
use crate::i18n::t;
use crate::tr;
use crate::ui::theme::{Palette, palette, size, space, tabular};
use crate::utils::dates::{month_year, short_month, short_month_year};

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
            t("Portfolio Analytics", "Análisis de la cartera"),
            Some(t("Every figure is computed from your recorded capital, loans and payments.", "Cada cifra se calcula con tu capital, préstamos y pagos registrados.").into_any_element()),
            vec![
                Button::new("an-withdraw")
                    .label(t("Withdraw capital", "Retirar capital"))
                    .on_click(cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Withdrawal, w, cx)))
                    .into_any_element(),
                Button::new("an-add")
                    .primary()
                    .icon(IconName::Plus)
                    .label(t("Add capital", "Agregar capital"))
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
            .child(cell(t("Portfolio value", "Valor de la cartera"), money.format(s.portfolio_value), t("Cash + capital lent + interest due", "Efectivo + capital prestado + interés vencido").into(), p.text))
            .child(vrule(&p))
            .child(cell(t("Portfolio gain", "Ganancia de la cartera"), money.format_signed(gain), t("Value minus capital invested", "Valor menos capital invertido").into(), gain_color))
            .child(vrule(&p))
            .child(cell(
                t("Return on invested capital", "Rendimiento sobre el capital"),
                roic.clone().map(|r| if gain > 0 { format!("+{r}") } else { r }).unwrap_or_else(|| "—".into()),
                t("Total, not annualized", "Total, no anualizado").into(),
                gain_color,
            ))
            .child(vrule(&p))
            .child(cell(t("Income received", "Ganancia cobrada"), money.format(s.realized_income()), t("Interest and fees received", "Intereses y cargos cobrados").into(), p.text));
        let capital = card(&p)
            .flex_row()
            .child(cell(t("Capital invested", "Capital invertido"), money.format(invested), {
                let (i, o) = (money.format(s.contributions), money.format(s.withdrawals));
                tr!("{i} in · {o} out", "{i} entró · {o} salió")
            }, p.text))
            .child(vrule(&p))
            .child(cell(t("Available cash", "Efectivo disponible"), money.format(s.available_cash), t("Not lent out", "Sin prestar").into(), p.text))
            .child(vrule(&p))
            .child(cell(t("Interest due, not collected", "Interés vencido sin cobrar"), money.format(s.interest_receivable), t("Past its due date", "Ya pasó su fecha").into(), p.text))
            .child(vrule(&p))
            .child(cell(t("Expected interest per month", "Interés esperado por mes"), money.format(s.expected_monthly_interest), t("At today's capital", "Con el capital de hoy").into(), p.text))
            .child(vrule(&p))
            .child(cell(
                t("Defaulted balance", "Saldo incobrable"),
                money.format(s.defaulted_principal + s.defaulted_interest),
                t("Excluded from value", "Fuera del valor").into(),
                if s.defaulted_principal > 0 { p.negative } else { p.text },
            ));

        // Value chart.
        let range = self.settings().chart_range;
        let points = self.chart_points(range);
        let options: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let control = segmented(&p, "an-range", &options, range, cx, |app, r, w, cx| app.update_settings(|s| s.chart_range = r, w, cx));
        let chart_body = if points.is_empty() {
            empty_state(
                &p,
                IconName::ChartLine,
                t("No history yet", "Aún no hay historial"),
                t("Record capital and loans to see portfolio value over time.", "Registra capital y préstamos para ver el valor de la cartera en el tiempo."),
                None,
            ).into_any_element()
        } else {
            div().px(px(space::XL)).pb(px(space::L)).child(self.growth_chart(&p, &points, super::overview::ChartSlot::Analytics, 300., cx)).into_any_element()
        };
        let value_card = card(&p).child(card_header(&p, t("Portfolio value", "Valor de la cartera"), Some(control.into_any_element()))).child(chart_body);

        // Monthly interest, last 12 months.
        let flows = self.snap.book.monthly_cash_flow(today);
        let last12: Vec<_> = flows.iter().rev().take(12).rev().collect();
        let labels: Vec<SharedString> = last12.iter().map(|f| short_month(f.month).into()).collect();
        let values: Vec<i64> = last12.iter().map(|f| f.interest_received + f.fees_received).collect();
        let interest_card = card(&p)
            .flex_1()
            .flex_basis(px(0.))
            .min_w_0()
            .child(card_header(
                &p,
                t("Interest received by month", "Interés cobrado por mes"),
                Some(div().text_size(px(size::META)).text_color(p.muted).child(t("Including fees", "Incluye cargos")).into_any_element()),
            ))
            .child(if last12.is_empty() {
                empty_state(
                    &p,
                    IconName::ChartColumn,
                    t("No income yet", "Aún no hay ganancias"),
                    t("Monthly interest appears once payments come in.", "El interés mensual aparece cuando lleguen los pagos."),
                    None,
                ).into_any_element()
            } else {
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::L))
                    .child(column_chart(&p, &labels, &values, true, &money, 260.))
                    .child(div().h(px(1.)).bg(p.border))
                    .child(kv(&p, t("Last 12 months", "Últimos 12 meses"), money.format(values.iter().sum::<i64>())))
                    .child(kv(&p, t("Monthly average", "Promedio mensual"), money.format(values.iter().sum::<i64>() / values.len().max(1) as i64)))
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
                .child(div().flex_1().flex_basis(px(0.)).min_w(px(96.)).child(if narrow { short_month_year(f.month) } else { month_year(f.month) }))
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
            .child(card_header(&p, t("Monthly cash flow", "Flujo de efectivo mensual"), None))
            .child(if flows.is_empty() {
                empty_state(
                    &p,
                    IconName::Table,
                    t("No cash movements yet", "Aún no hay movimientos"),
                    t("Capital, loans and payments are summarised here month by month.", "Aquí se resumen capital, préstamos y pagos mes a mes."),
                    None,
                ).into_any_element()
            } else {
                v_flex()
                    .child(
                        table_header(&p)
                            .child(div().flex_1().flex_basis(px(0.)).min_w(px(96.)).child(th(&p, t("Month", "Mes"))))
                            .child(fcol().child(th(&p, t("Capital in", "Capital entró"))))
                            .child(fcol().child(th(&p, t("Capital out", "Capital salió"))))
                            .child(fcol().child(th(&p, t("Lent", "Prestado"))))
                            .child(fcol().child(th(&p, t("Capital back", "Capital cobrado"))))
                            .child(fcol().child(th(&p, t("Interest", "Interés"))))
                            .child(fcol().child(th(&p, t("Net cash flow", "Flujo neto"))))
                            .child(fcol().child(th(&p, t("Ending cash", "Efectivo final")))),
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
            .child(card_header(&p, t("Capital ledger", "Movimientos de capital"), None))
            .child(if self.snap.capital.is_empty() {
                empty_state(
                    &p,
                    IconName::Landmark,
                    t("No capital recorded", "No hay capital registrado"),
                    t(
                        "Record the money you set aside for lending. It is your invested capital, not profit.",
                        "Registra el dinero que apartaste para prestar. Es tu capital invertido, no ganancia.",
                    ),
                    Some(
                        Button::new("ledger-add")
                            .primary()
                            .label(t("Add capital", "Agregar capital"))
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
            .child(card_header(p, t("Capital lent out", "Capital en la calle"), None))
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
                                    .child(label(p, t("Share of capital lent", "Porción del capital prestada")))
                                    .child(figure(p, percent_label(deployed, pool).unwrap_or_else(|| "—".into()), size::FIGURE)),
                            )
                            .child(div().text_color(p.muted).font_features(tabular()).child({
                                let (a, b) = (money.format(deployed), money.format(pool));
                                tr!("{a} of {b}", "{a} de {b}")
                            })),
                    )
                    .child(crate::ui::components::progress_bar(p, ratio, p.accent))
                    .child(kv(p, t("Available to lend", "Disponible para prestar"), money.format(s.available_cash)))
                    .child(div().h(px(1.)).bg(p.border))
                    .child(div().pt(px(4.)).font_weight(FontWeight::MEDIUM).child(tr!("Loans by status · {total}", "Préstamos por estado · {total}")))
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
