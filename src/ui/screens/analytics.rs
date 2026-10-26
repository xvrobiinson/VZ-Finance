use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{ncolumn, row_icon_button};
use crate::app::VzApp;
use crate::charts::bars::column_chart;
use crate::domain::ledger::DisplayStatus;
use crate::domain::money::percent_label;
use crate::domain::portfolio::{Account, CapitalKind, EntryKind};
use crate::i18n::t;
use crate::services::settings::ChartRange;
use crate::tr;
use crate::ui::components::{empty_state, kv, page_header, sel, table_header, th};
use crate::ui::forms::reason::ReasonKind;
use crate::ui::kit::{Button, Card, Dropdown, DropdownItem, Segmented, Stat, StatStrip};
use crate::ui::theme::{Palette, Typeset as _, Width, palette, space, tabular, text};
use crate::utils::dates::{month_year, short_month, short_month_year};

impl VzApp {
    /// Returns, capital, charts, cash flow and the money ledger. Cards sit
    /// side by side from `Wide` up; tables drop their minor columns below it.
    pub(crate) fn render_analytics(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let w = Width::of(self.content_width(window));
        let gap = px(w.gap());
        let compact = w == Width::Compact;
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
            Some(
                t(
                    "Every figure is computed from your recorded capital, loans and payments.",
                    "Cada cifra se calcula con tu capital, préstamos y pagos registrados.",
                )
                .into_any_element(),
            ),
            self.analytics_actions(w, cx),
        );

        let cell = |l: &str, v: String, sub: String, color: Hsla| Stat::new(l.to_string(), v).tone(color).caption(sel(sub));
        let gain_color = match gain.signum() {
            1 => p.positive,
            -1 => p.negative,
            _ => p.text,
        };
        let return_cells = vec![
            cell(
                t("Portfolio value", "Valor de la cartera"),
                money.format(s.portfolio_value),
                t("Cash + capital lent + interest due", "Efectivo + capital prestado + interés vencido").into(),
                p.text,
            ),
            cell(
                t("Portfolio gain", "Ganancia de la cartera"),
                money.format_signed(gain),
                t("Value minus capital invested", "Valor menos capital invertido").into(),
                gain_color,
            ),
            cell(
                t("Return on invested capital", "Rendimiento sobre el capital"),
                roic.clone().map(|r| if gain > 0 { format!("+{r}") } else { r }).unwrap_or_else(|| "—".into()),
                t("Total, not annualized", "Total, no anualizado").into(),
                gain_color,
            ),
            cell(
                t("Income received", "Ganancia cobrada"),
                money.format(s.realized_income()),
                t("Interest and fees received", "Intereses y cargos cobrados").into(),
                p.text,
            ),
        ];
        // Full money figures are never truncated to fit a row: four need a
        // wide column, six need an ultra one.
        let returns = StatStrip::new(if w >= Width::Wide { 4 } else { 2 }).dense(compact).stats(return_cells);
        let capital_cells = vec![
            cell(
                t("Capital invested", "Capital invertido"),
                money.format(invested),
                {
                    let (i, o) = (money.format(s.contributions), money.format(s.withdrawals));
                    tr!("{i} in · {o} out", "{i} entró · {o} salió")
                },
                p.text,
            ),
            cell(
                t("Available capital", "Capital disponible"),
                money.format(s.available_capital),
                t("Capital not lent out", "Capital sin prestar").into(),
                p.text,
            ),
            cell(
                t("Available interest", "Interés disponible"),
                money.format(s.available_interest),
                if s.expenses() > 0 {
                    let spent = money.format(s.expenses());
                    tr!("{spent} spent in expenses", "{spent} usado en gastos")
                } else {
                    t("Interest and fees on hand", "Interés y cargos en mano").into()
                },
                p.text,
            ),
            cell(
                t("Interest due, not collected", "Interés vencido sin cobrar"),
                money.format(s.interest_receivable),
                t("Past its due date", "Ya pasó su fecha").into(),
                p.text,
            ),
            cell(
                t("Expected interest per month", "Interés esperado por mes"),
                money.format(s.expected_monthly_interest),
                t("At today's capital", "Con el capital de hoy").into(),
                p.text,
            ),
            cell(
                t("Defaulted balance", "Saldo incobrable"),
                money.format(s.defaulted_principal + s.defaulted_interest),
                t("Excluded from value", "Fuera del valor").into(),
                if s.defaulted_principal > 0 { p.negative } else { p.text },
            ),
        ];
        let capital = StatStrip::new(match w {
            Width::Compact => 2,
            Width::Medium | Width::Wide => 3,
            Width::Ultra => 6,
        })
        .dense(compact)
        .stats(capital_cells);

        // Value chart.
        let range = self.settings().chart_range;
        let points = self.chart_points(range);
        let options: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let control = Segmented::pick("an-range", options.iter().copied(), range, cx, |app, r, w, cx| {
            app.update_settings(|s| s.chart_range = r, w, cx)
        });
        let chart_body = if points.is_empty() {
            empty_state(
                &p,
                IconName::ChartLine,
                t("No history yet", "Aún no hay historial"),
                t(
                    "Record capital and loans to see portfolio value over time.",
                    "Registra capital y préstamos para ver el valor de la cartera en el tiempo.",
                ),
                None,
            )
            .into_any_element()
        } else {
            div()
                .px(px(space::XL))
                .pb(px(space::L))
                .child(self.growth_chart(
                    &p,
                    &points,
                    super::overview::ChartSlot::Analytics,
                    if w >= Width::Wide { 300. } else { 232. },
                    cx,
                ))
                .into_any_element()
        };
        let value_card = Card::new().title(t("Portfolio value", "Valor de la cartera")).action(control).child(chart_body);

        // Monthly interest, last 12 months.
        let flows = self.snap.book.monthly_cash_flow(today);
        let last12: Vec<_> = flows.iter().rev().take(12).rev().collect();
        let labels: Vec<SharedString> = last12.iter().map(|f| short_month(f.month).into()).collect();
        let values: Vec<i64> = last12.iter().map(|f| f.interest_received + f.fees_received).collect();
        let interest_card = Card::new()
            .flex_1()
            .flex_basis(px(0.))
            .min_w_0()
            .title(t("Interest received by month", "Interés cobrado por mes"))
            .action(div().flex_none().typeset(text::CAPTION).text_color(p.muted).child(t("Including fees", "Incluye cargos")))
            .child(if last12.is_empty() {
                empty_state(
                    &p,
                    IconName::ChartColumn,
                    t("No income yet", "Aún no hay ganancias"),
                    t("Monthly interest appears once payments come in.", "El interés mensual aparece cuando lleguen los pagos."),
                    None,
                )
                .into_any_element()
            } else {
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::L))
                    .child(column_chart(&p, &labels, &values, true, &money, 260.))
                    .child(div().h(px(1.)).bg(p.border))
                    .child(kv(&p, t("Last 12 months", "Últimos 12 meses"), sel(money.format(values.iter().sum::<i64>()))))
                    .child(kv(
                        &p,
                        t("Monthly average", "Promedio mensual"),
                        sel(money.format(values.iter().sum::<i64>() / values.len().max(1) as i64)),
                    ))
                    .into_any_element()
            });

        let deploy_card = self.deployment_card(&p);

        // Monthly cash flow table, newest first. Capital moves and expenses
        // (also in the ledger below) go first as the column narrows, then
        // lending; interest, net flow and ending cash always stay.
        let show_moves = w >= Width::Wide;
        let show_lending = w >= Width::Medium;
        let flow_rows = flows.iter().rev().map(|f| {
            let net = f.net();
            h_flex()
                .h(px(40.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .child(div().flex_1().flex_basis(px(0.)).min_w(px(96.)).child(sel(if show_moves {
                    month_year(f.month)
                } else {
                    short_month_year(f.month)
                })))
                .when(show_moves, |d| {
                    d.child(fcol().text_color(p.muted).child(sel(dash(&money, f.contributions))))
                        .child(fcol().text_color(p.muted).child(sel(dash_neg(&money, f.withdrawals))))
                })
                .when(show_lending, |d| {
                    d.child(fcol().text_color(p.muted).child(sel(dash_neg(&money, f.disbursed))))
                        .child(fcol().text_color(p.muted).child(sel(dash(&money, f.principal_returned))))
                })
                .child(
                    fcol()
                        .text_color(if f.interest_received + f.fees_received > 0 { p.positive } else { p.muted })
                        .child(sel(dash(&money, f.interest_received + f.fees_received))),
                )
                .when(show_moves, |d| d.child(fcol().text_color(p.muted).child(sel(dash_neg(&money, f.expenses)))))
                .child(fcol().child(sel(money.format_signed(net))))
                .child(fcol().child(sel(money.format(f.ending_cash))))
        });
        let flow_card =
            Card::new().overflow_hidden().title(t("Monthly cash flow", "Flujo de efectivo mensual")).child(if flows.is_empty() {
                empty_state(
                    &p,
                    IconName::Table,
                    t("No cash movements yet", "Aún no hay movimientos"),
                    t(
                        "Capital, loans and payments are summarised here month by month.",
                        "Aquí se resumen capital, préstamos y pagos mes a mes.",
                    ),
                    None,
                )
                .into_any_element()
            } else {
                v_flex()
                    .child(
                        table_header(&p)
                            .child(div().flex_1().flex_basis(px(0.)).min_w(px(96.)).child(th(&p, t("Month", "Mes"))))
                            .when(show_moves, |d| {
                                d.child(fcol().child(th(&p, t("Capital in", "Capital entró"))))
                                    .child(fcol().child(th(&p, t("Capital out", "Capital salió"))))
                            })
                            .when(show_lending, |d| {
                                d.child(fcol().child(th(&p, t("Lent", "Prestado"))))
                                    .child(fcol().child(th(&p, t("Capital back", "Capital cobrado"))))
                            })
                            .child(fcol().child(th(&p, t("Interest", "Interés"))))
                            .when(show_moves, |d| d.child(fcol().child(th(&p, t("Expenses", "Gastos")))))
                            .child(fcol().child(th(&p, t("Net cash flow", "Flujo neto"))))
                            .child(fcol().child(th(&p, t("Ending cash", "Efectivo final")))),
                    )
                    .children(flow_rows)
                    .into_any_element()
            });

        // Every movement of cash outside loans and payments, newest first.
        enum Mv {
            Capital(i64),
            Entry(i64),
        }
        let mut movements: Vec<(chrono::NaiveDate, i64, Mv)> = self
            .snap
            .capital
            .iter()
            .map(|c| (c.occurred_on, c.id, Mv::Capital(c.id)))
            .chain(self.snap.entries.iter().map(|e| (e.occurred_on, e.id, Mv::Entry(e.id))))
            .collect();
        movements.sort_by_key(|(d, id, _)| std::cmp::Reverse((*d, *id)));
        let move_rows: Vec<AnyElement> = movements
            .iter()
            .filter_map(|(date, _, mv)| {
                // (type, account detail, note, amount shown, its color, row key, delete)
                let (kind, detail, note, amount_text, color, key, delete) = match mv {
                    Mv::Capital(id) => {
                        let c = self.snap.capital_tx(*id)?;
                        let (amount, color) = match c.kind {
                            CapitalKind::Contribution => (money.format_signed(c.amount), p.positive),
                            CapitalKind::Withdrawal => (money.format_signed(-c.amount), p.text),
                        };
                        let detail = Account::Capital.label().to_string();
                        (
                            c.kind.label().to_string(),
                            detail,
                            c.notes.clone(),
                            amount,
                            color,
                            format!("c{id}"),
                            ReasonKind::DeleteCapital(*id),
                        )
                    }
                    Mv::Entry(id) => {
                        let e = self.snap.entry(*id)?;
                        let (detail, amount, color) = match e.kind {
                            EntryKind::Transfer => {
                                (format!("{} → {}", e.account.label(), e.account.other().label()), money.format(e.amount), p.muted)
                            }
                            EntryKind::Expense => {
                                let from = e.account.label().to_lowercase();
                                (tr!("From {from}", "Del {from}"), money.format_signed(-e.amount), p.text)
                            }
                            EntryKind::Adjustment => (
                                e.account.label().to_string(),
                                money.format_signed(e.amount),
                                if e.amount > 0 { p.positive } else { p.text },
                            ),
                        };
                        (
                            e.kind.label().to_string(),
                            detail,
                            e.description.clone(),
                            amount,
                            color,
                            format!("e{id}"),
                            ReasonKind::DeleteEntry(*id),
                        )
                    }
                };
                let edit_mv = match mv {
                    Mv::Capital(id) => (true, *id),
                    Mv::Entry(id) => (false, *id),
                };
                // Compact: the date leads the detail line and the note column goes.
                let detail = if compact { format!("{} · {detail}", ds.format(*date)) } else { detail };
                Some(
                    h_flex()
                        .min_h(px(48.))
                        .px(px(space::XL))
                        .gap(px(space::L))
                        .border_b_1()
                        .border_color(p.rule)
                        .when(!compact, |d| d.child(div().w(px(120.)).flex_none().font_features(tabular()).child(sel(ds.format(*date)))))
                        .child(
                            v_flex()
                                .when(compact, |d| d.flex_1().min_w_0())
                                .when(!compact, |d| d.w(px(220.)).flex_none())
                                .child(div().truncate().child(sel(kind)))
                                .child(
                                    div().typeset(text::CAPTION).font_features(tabular()).text_color(p.muted).truncate().child(sel(detail)),
                                ),
                        )
                        .when(!compact, |d| d.child(div().flex_1().min_w_0().truncate().text_color(p.muted).child(sel(note))))
                        .child(ncolumn(140.).whitespace_nowrap().font_features(tabular()).text_color(color).child(sel(amount_text)))
                        .child(
                            h_flex()
                                .w(px(64.))
                                .flex_none()
                                .justify_end()
                                .child(row_icon_button(
                                    SharedString::from(format!("mv-edit-{key}")),
                                    IconName::Pencil,
                                    t("Edit", "Editar"),
                                    cx,
                                    move |this, w, cx| match edit_mv {
                                        (true, id) => this.edit_capital(id, w, cx),
                                        (false, id) => this.open_entry_form(EntryKind::Expense, Some(id), w, cx),
                                    },
                                ))
                                .child(row_icon_button(
                                    SharedString::from(format!("mv-del-{key}")),
                                    IconName::Trash,
                                    t("Delete", "Eliminar"),
                                    cx,
                                    move |this, w, cx| this.open_reason_form(delete, w, cx),
                                )),
                        )
                        .into_any_element(),
                )
            })
            .collect();
        let ledger_actions = h_flex()
            .flex_none()
            .gap(px(space::S))
            .child(
                Button::new("ledger-move")
                    .small()
                    .icon(IconName::ArrowRightLeft)
                    .label(t("Move money", "Mover dinero"))
                    .on_click(cx.listener(|this, _, w, cx| this.open_entry_form(EntryKind::Transfer, None, w, cx))),
            )
            .child(
                Button::new("ledger-adjust")
                    .small()
                    .icon(IconName::SlidersHorizontal)
                    .label(t("Adjust balances", "Ajustar saldos"))
                    .on_click(cx.listener(|this, _, w, cx| this.open_adjust_form(w, cx))),
            );
        let ledger_card = Card::new().overflow_hidden().title(t("Money movements", "Movimientos de dinero")).action(ledger_actions).child(
            if movements.is_empty() {
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
                v_flex().children(move_rows).into_any_element()
            },
        );

        let changes: Vec<_> = self.snap.changes.iter().collect();
        let history = (!changes.is_empty()).then(|| self.history_card(&p, &changes, 40));

        let side_by_side = w >= Width::Wide;
        v_flex()
            .child(header)
            .child(
                v_flex()
                    .gap(gap)
                    .child(returns)
                    .child(capital)
                    .child(value_card)
                    .child(
                        div()
                            .flex()
                            .gap(gap)
                            .when(side_by_side, |d| d.flex_row().items_stretch())
                            .when(!side_by_side, |d| d.flex_col())
                            .child(interest_card)
                            .child(deploy_card),
                    )
                    .child(flow_card)
                    .child(ledger_card)
                    .children(history),
            )
            .into_any_element()
    }

    /// Page actions. Wide windows show all three; narrower ones fold the
    /// secondary ones into a "More" menu so the primary never wraps.
    fn analytics_actions(&self, w: Width, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let add = Button::new("an-add")
            .primary()
            .icon(IconName::Plus)
            .label(t("Add capital", "Agregar capital"))
            .on_click(cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Contribution, w, cx)))
            .into_any_element();
        let expense = cx.listener(|this, _, w, cx| this.open_entry_form(EntryKind::Expense, None, w, cx));
        let withdraw = cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Withdrawal, w, cx));
        if w >= Width::Wide {
            return vec![
                Button::new("an-expense")
                    .icon(IconName::ReceiptText)
                    .label(t("New expense", "Nuevo gasto"))
                    .on_click(expense)
                    .into_any_element(),
                Button::new("an-withdraw").label(t("Withdraw capital", "Retirar capital")).on_click(withdraw).into_any_element(),
                add,
            ];
        }
        let more = Dropdown::new("an-more")
            .icon(IconName::Ellipsis)
            .tooltip(t("More actions", "Más acciones"))
            .align_end()
            .item(DropdownItem::new(t("New expense", "Nuevo gasto"), expense).icon(IconName::ReceiptText))
            .item(DropdownItem::new(t("Withdraw capital", "Retirar capital"), withdraw).icon(IconName::Landmark));
        vec![more.into_any_element(), add]
    }

    fn deployment_card(&self, p: &Palette) -> Card {
        let money = self.settings().money();
        let s = self.snap.book.summary(self.today);
        let deployed = s.outstanding_principal;
        let pool = deployed + s.available_cash.max(0);
        let ratio = if pool > 0 { deployed as f32 / pool as f32 } else { 0.0 };
        let mut counts = [
            (DisplayStatus::Active, 0usize),
            (DisplayStatus::DueSoon, 0),
            (DisplayStatus::Overdue, 0),
            (DisplayStatus::PaidOff, 0),
            (DisplayStatus::Defaulted, 0),
        ];
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
            counts.iter().filter(|(_, n)| *n > 0).map(|(st, n)| div().h_full().flex_basis(px(0.)).flex_grow(*n as f32).bg(colors(*st))),
        );
        let legend = counts.iter().map(|(st, n)| {
            h_flex()
                .min_h(px(30.))
                .gap(px(10.))
                .child(div().size(px(8.)).rounded(px(2.)).bg(colors(*st)))
                .child(div().flex_1().text_color(p.muted).child(sel(st.label())))
                .child(div().font_features(tabular()).child(sel(n.to_string())))
        });
        Card::new().flex_1().flex_basis(px(0.)).min_w_0().title(t("Capital lent out", "Capital en la calle")).child(
            v_flex()
                .px(px(space::XL))
                .pb(px(space::XL))
                .gap(px(space::M))
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_between()
                        .items_end()
                        .gap(px(space::S))
                        .child(Stat::new(
                            t("Share of capital lent", "Porción del capital prestada"),
                            percent_label(deployed, pool).unwrap_or_else(|| "—".into()),
                        ))
                        .child(div().text_color(p.muted).font_features(tabular()).whitespace_nowrap().child(sel({
                            let (a, b) = (money.format(deployed), money.format(pool));
                            tr!("{a} of {b}", "{a} de {b}")
                        }))),
                )
                .child(crate::ui::components::progress_bar(p, ratio, p.accent))
                .child(kv(p, t("Available to lend", "Disponible para prestar"), sel(money.format(s.available_cash))))
                .child(div().h(px(1.)).bg(p.border))
                .child(
                    div()
                        .pt(px(4.))
                        .typeset(text::BODY_STRONG)
                        .font_features(tabular())
                        .child(tr!("Loans by status · {total}", "Préstamos por estado · {total}")),
                )
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
