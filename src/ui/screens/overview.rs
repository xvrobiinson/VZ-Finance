use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{header_link, segmented, status_detail_pill};
use crate::app::{Route, VzApp};
use crate::charts::line::{LineChart, Series};
use crate::domain::ledger::DisplayStatus;
use crate::domain::money::{Cents, percent_label};
use crate::domain::portfolio::{CapitalKind, HistoryPoint, Summary};
use crate::services::settings::ChartRange;
use crate::ui::components::{avatar, card, card_header, empty_state, figure, icon, kbd, vrule};
use crate::ui::theme::{Palette, palette, size, space, tabular};

/// Which screen's portfolio chart is being drawn; each keeps its own hover.
#[derive(Clone, Copy)]
pub(crate) enum ChartSlot {
    Overview,
    Analytics,
}

impl VzApp {
    pub(crate) fn render_overview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let width = self.content_width(window);
        // Six figures in one row need about 1,250px; below that use 3 + 3.
        let kpi_compact = width < 1250.;
        let compact = width < 1150.;
        let s = self.snap.book.summary(self.today);
        let money = self.settings().money();

        let finding: AnyElement = if s.overdue_loans > 0 {
            h_flex()
                .gap(px(4.))
                .child(format!("{} loan{} overdue ·", s.overdue_loans, if s.overdue_loans == 1 { "" } else { "s" }))
                .child(div().text_color(p.negative).font_features(tabular()).child(format!("{} past due", money.format(s.overdue_amount))))
                .into_any_element()
        } else if s.due_soon_loans > 0 {
            format!("{} payment{} due in the next 7 days", s.due_soon_loans, if s.due_soon_loans == 1 { "" } else { "s" }).into_any_element()
        } else if s.active_loans > 0 {
            format!("All {} active loan{} are current", s.active_loans, if s.active_loans == 1 { "" } else { "s" }).into_any_element()
        } else {
            "Your lending portfolio at a glance".into_any_element()
        };

        let actions = vec![
            Button::new("ov-new-loan")
                .label("New loan")
                .tooltip("Ctrl+L")
                .on_click(cx.listener(|this, _, w, cx| this.open_loan_form(None, w, cx)))
                .into_any_element(),
            Button::new("ov-pay")
                .primary()
                .icon(IconName::Plus)
                .label("Record payment")
                .tooltip("Ctrl+P")
                .on_click(cx.listener(|this, _, w, cx| this.open_payment_form(None, w, cx)))
                .into_any_element(),
        ];

        v_flex()
            .gap(px(space::XL))
            .child(crate::ui::components::page_header(&p, "Overview", Some(finding), actions).pb_0())
            .children(self.render_setup(&p, cx))
            .child(self.kpi_strip(&p, &s, kpi_compact))
            .child(if compact {
                v_flex().gap(px(space::XL)).child(self.growth_card(&p, cx)).child(self.allocation_card(&p, &s, true)).into_any_element()
            } else {
                h_flex()
                    .items_stretch()
                    .gap(px(space::XL))
                    .child(self.growth_card(&p, cx))
                    .child(self.allocation_card(&p, &s, false))
                    .into_any_element()
            })
            .child(if compact {
                // Time-sensitive block first when stacked.
                v_flex().gap(px(space::XL)).child(self.attention_card(&p, cx)).child(self.recent_payments_card(&p, cx)).into_any_element()
            } else {
                h_flex()
                    .items_stretch()
                    .gap(px(space::XL))
                    .child(self.recent_payments_card(&p, cx))
                    .child(self.attention_card(&p, cx))
                    .into_any_element()
            })
            .into_any_element()
    }

    /// First-run checklist, shown until the first loan exists.
    fn render_setup(&self, p: &Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.snap.loans.is_empty() {
            return None;
        }
        let has_capital = !self.snap.capital.is_empty();
        let has_client = !self.snap.clients.is_empty();
        let done = [has_capital, has_client, false].iter().filter(|d| **d).count();
        let step = |n: usize, done: bool, title: &str, body: &str, action: AnyElement| {
            h_flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .items_start()
                .gap(px(space::M))
                .p(px(space::L))
                .rounded(px(10.))
                .bg(p.raised)
                .child(
                    h_flex()
                        .flex_none()
                        .size(px(24.))
                        .rounded_full()
                        .justify_center()
                        .bg(if done { p.positive_tint } else { p.surface })
                        .border_1()
                        .border_color(if done { p.positive_tint } else { p.border })
                        .text_size(px(size::META))
                        .text_color(if done { p.positive } else { p.muted })
                        .child(if done { icon(IconName::Check, p.positive, 14.).into_any_element() } else { n.to_string().into_any_element() }),
                )
                .child(
                    v_flex()
                        .gap(px(4.))
                        .flex_1()
                        .min_w_0()
                        .child(div().font_weight(FontWeight::MEDIUM).text_color(p.text).child(title.to_string()))
                        .child(div().text_color(p.muted).text_size(px(size::META + 0.5)).child(body.to_string()))
                        .child(h_flex().pt(px(6.)).child(action)),
                )
        };
        Some(
            card(p)
                .p(px(space::XL))
                .gap(px(space::L))
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            v_flex()
                                .gap(px(4.))
                                .child(div().text_size(px(size::SECTION)).font_weight(FontWeight::SEMIBOLD).child("Set up your lending book"))
                                .child(div().text_color(p.muted).child("Three steps, and every figure on this screen comes from what you record.")),
                        )
                        .child(div().text_color(p.muted).font_features(tabular()).child(format!("{done} of 3 done"))),
                )
                .child(
                    h_flex()
                        .items_stretch()
                        .gap(px(space::M))
                        .child(step(
                            1,
                            has_capital,
                            "Record your starting capital",
                            "The money you have set aside to lend.",
                            Button::new("setup-capital")
                                .label("Add capital")
                                .on_click(cx.listener(|this, _, w, cx| this.open_capital_form(CapitalKind::Contribution, w, cx)))
                                .into_any_element(),
                        ))
                        .child(step(
                            2,
                            has_client,
                            "Add a client",
                            "Name and optional contact details.",
                            Button::new("setup-client")
                                .label("New client")
                                .on_click(cx.listener(|this, _, w, cx| this.open_client_form(None, w, cx)))
                                .into_any_element(),
                        ))
                        .child(step(
                            3,
                            false,
                            "Create the first loan",
                            "Preview the schedule before you confirm.",
                            Button::new("setup-loan")
                                .primary()
                                .label("New loan")
                                .on_click(cx.listener(|this, _, w, cx| this.open_loan_form(None, w, cx)))
                                .into_any_element(),
                        )),
                )
                .into_any_element(),
        )
    }

    fn kpi_strip(&self, p: &Palette, s: &Summary, compact: bool) -> impl IntoElement {
        let money = self.settings().money();
        let cell = |icon_name: IconName, label: &str, value: String, value_color: Hsla, sub: AnyElement, lead: bool| {
            v_flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .when(lead && !compact, |d| d.flex_grow(1.5))
                .px(px(space::XL))
                .py(px(space::L + 2.))
                .gap(px(8.))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .child(icon(icon_name, p.faint, 14.))
                        .child(div().text_size(px(size::META + 0.5)).text_color(p.muted).min_w_0().truncate().child(label.to_string())),
                )
                .child(figure(p, value, if lead { size::LEAD } else { size::FIGURE }).text_color(value_color))
                .child(div().text_size(px(size::META)).text_color(p.muted).min_w_0().truncate().child(sub))
        };
        let gain = s.gain();
        let invested = s.net_invested();
        let gain_sub: AnyElement = if invested > 0 {
            let pct = percent_label(gain, invested).unwrap_or_default();
            let c = match gain.signum() {
                1 => p.positive,
                -1 => p.negative,
                _ => p.muted,
            };
            h_flex()
                .gap(px(4.))
                .child(div().text_color(c).font_features(tabular()).child(format!(
                    "{} ({}{})",
                    money.format_signed(gain),
                    if gain > 0 { "+" } else { "" },
                    pct
                )))
                .child(div().font_features(tabular()).child(format!("on {} invested", money.format(invested))))
                .into_any_element()
        } else {
            "No capital recorded yet".into_any_element()
        };
        let fees_sub = if s.fees_collected > 0 {
            format!("plus {} in fees", money.format(s.fees_collected))
        } else {
            "Received, all time".to_string()
        };
        let pv = s.portfolio_value;
        let cash_sub = match percent_label(s.available_cash, pv) {
            Some(pct) if pv > 0 => format!("{pct} of portfolio"),
            _ => "Ready to lend".to_string(),
        };
        let overdue_sub = if s.overdue_loans > 0 {
            format!("{} loan{} behind", s.overdue_loans, if s.overdue_loans == 1 { "" } else { "s" })
        } else {
            "Nothing past due".to_string()
        };
        let active_sub = if s.due_soon_loans > 0 {
            format!("{} due this week", s.due_soon_loans)
        } else if s.active_loans > 0 {
            "None due this week".to_string()
        } else {
            "No loans outstanding".to_string()
        };

        let mut cells = vec![
            cell(IconName::Wallet, "Total portfolio value", money.format(pv), p.text, gain_sub, true),
            cell(
                IconName::HandCoins,
                "Outstanding principal",
                money.format(s.outstanding_principal),
                p.text,
                format!("across {} active loan{}", s.active_loans, if s.active_loans == 1 { "" } else { "s" }).into_any_element(),
                false,
            ),
            cell(IconName::TrendingUp, "Interest earned", money.format(s.interest_collected), p.text, fees_sub.into_any_element(), false),
            cell(IconName::Landmark, "Available capital", money.format(s.available_cash), p.text, cash_sub.into_any_element(), false),
            cell(IconName::FileText, "Active loans", s.active_loans.to_string(), p.text, active_sub.into_any_element(), false),
            cell(
                IconName::CircleAlert,
                "Overdue amount",
                money.format(s.overdue_amount),
                if s.overdue_amount > 0 { p.negative } else { p.text },
                overdue_sub.into_any_element(),
                false,
            ),
        ];
        // One row of six on wide windows; two rows of three when narrow.
        let per_row = if compact { 3 } else { 6 };
        let mut rows = Vec::new();
        while !cells.is_empty() {
            let rest = cells.split_off(per_row.min(cells.len()));
            let mut row = h_flex().items_stretch();
            for (i, c) in cells.into_iter().enumerate() {
                if i > 0 {
                    row = row.child(vrule(p));
                }
                row = row.child(c);
            }
            rows.push(row);
            cells = rest;
        }
        let mut strip = card(p);
        for (i, r) in rows.into_iter().enumerate() {
            if i > 0 {
                strip = strip.child(div().h(px(1.)).mx(px(space::XL)).bg(p.border));
            }
            strip = strip.child(r);
        }
        strip
    }

    pub(crate) fn chart_points(&self, range: ChartRange) -> Vec<HistoryPoint> {
        let Some(first) = self.snap.book.first_activity() else { return Vec::new() };
        let from = match range.months() {
            Some(m) => self.today.checked_sub_months(chrono::Months::new(m)).unwrap_or(self.today).max(first),
            None => first,
        };
        self.snap.book.history(from.min(self.today), self.today)
    }

    pub(crate) fn growth_chart(&self, p: &Palette, points: &[HistoryPoint], slot: ChartSlot, height: f32, cx: &mut Context<Self>) -> AnyElement {
        let (hover, id) = match slot {
            ChartSlot::Overview => (self.chart_hover, "growth"),
            ChartSlot::Analytics => (self.analytics_hover, "an-growth"),
        };
        let ds = self.settings().date_style;
        let span_days = match (points.first(), points.last()) {
            (Some(a), Some(b)) => (b.date - a.date).num_days(),
            _ => 0,
        };
        let labels = points
            .iter()
            .map(|pt| {
                if span_days > 400 {
                    pt.date.format("%b %Y").to_string()
                } else {
                    ds.format_short(pt.date, self.today)
                }
                .into()
            })
            .collect();
        let hover_labels = points.iter().map(|pt| ds.format(pt.date).into()).collect();
        let mut series = vec![Series {
            name: "Portfolio value".into(),
            values: points.iter().map(|x| x.portfolio_value).collect(),
            color: p.accent,
            width: 2.0,
            dashed: false,
            fill: true,
        }];
        if self.settings().show_interest_line {
            series.push(Series {
                name: "Capital invested".into(),
                values: points.iter().map(|x| x.net_invested).collect(),
                color: p.chart_reference,
                width: 1.5,
                dashed: true,
                fill: false,
            });
        }
        let chart = LineChart { labels, hover_labels, series, height, money: self.settings().money() };
        let weak = cx.entity().downgrade();
        let handler = Rc::new(move |h: Option<usize>, _: &mut Window, cx: &mut App| {
            let _ = weak.update(cx, |app, cx| {
                match slot {
                    ChartSlot::Overview => app.chart_hover = h,
                    ChartSlot::Analytics => app.analytics_hover = h,
                }
                cx.notify();
            });
        });
        chart.render(SharedString::from(id.to_string()), p, hover, handler)
    }

    fn growth_card(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let money = self.settings().money();
        let range = self.settings().chart_range;
        let points = self.chart_points(range);
        let options: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let control = segmented(p, "range", &options, range, cx, |app, r, w, cx| app.update_settings(|s| s.chart_range = r, w, cx));

        let (gain_change, interest_change) = match (points.first(), points.last()) {
            (Some(a), Some(b)) => (
                (b.portfolio_value - b.net_invested) - (a.portfolio_value - a.net_invested),
                b.interest_collected - a.interest_collected,
            ),
            _ => (0, 0),
        };
        let period = match range {
            ChartRange::All => "since you started".to_string(),
            r => format!("in the last {}", match r {
                ChartRange::OneMonth => "month",
                ChartRange::ThreeMonths => "3 months",
                ChartRange::SixMonths => "6 months",
                _ => "year",
            }),
        };
        let gain_color = match gain_change.signum() {
            1 => p.positive,
            -1 => p.negative,
            _ => p.muted,
        };
        let legend = h_flex()
            .gap(px(space::L))
            .text_size(px(size::META))
            .text_color(p.muted)
            .child(h_flex().gap(px(6.)).child(div().w(px(12.)).h(px(2.)).bg(p.accent)).child("Portfolio value"))
            .when(self.settings().show_interest_line, |d| {
                d.child(
                    h_flex()
                        .gap(px(6.))
                        .child(h_flex().gap(px(2.)).child(div().w(px(4.)).h(px(1.5)).bg(p.chart_reference)).child(div().w(px(4.)).h(px(1.5)).bg(p.chart_reference)))
                        .child("Capital invested"),
                )
            });

        let body = if points.is_empty() {
            empty_state(
                p,
                IconName::ChartLine,
                "No history yet",
                "Portfolio value is charted from your capital, loans and payments as soon as you record them.",
                None,
            )
            .into_any_element()
        } else {
            v_flex()
                .px(px(space::XL))
                .pb(px(space::L))
                .gap(px(space::M))
                .child(
                    h_flex()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .text_color(p.muted)
                                .child("Gain")
                                .child(div().text_color(gain_color).font_features(tabular()).child(money.format_signed(gain_change)))
                                .child(format!("{period} ·"))
                                .child(div().font_features(tabular()).child(format!("{} interest received", money.format(interest_change)))),
                        )
                        .child(legend),
                )
                .child(self.growth_chart(p, &points, ChartSlot::Overview, 248., cx))
                .into_any_element()
        };
        card(p).flex_1().min_w_0().child(card_header(p, "Portfolio growth", Some(control.into_any_element()))).child(body).into_any_element()
    }

    fn allocation_card(&self, p: &Palette, s: &Summary, full_width: bool) -> AnyElement {
        let money = self.settings().money();
        let total = s.portfolio_value.max(0);
        let mut cash_c = p.accent;
        cash_c.a = 0.35;
        let parts: [(&str, Cents, Hsla); 3] = [
            ("Lent out (principal)", s.outstanding_principal, p.accent),
            ("Interest receivable", s.interest_receivable, p.chart_secondary),
            ("Available cash", s.available_cash.max(0), cash_c),
        ];
        let bar = h_flex().h(px(10.)).w_full().rounded(px(3.)).overflow_hidden().gap(px(2.)).bg(p.neutral_tint).children(
            parts.iter().filter(|(_, v, _)| *v > 0 && total > 0).map(|(_, v, c)| div().h_full().flex_basis(px(0.)).flex_grow(*v as f32 / total as f32).bg(*c)),
        );
        let rows = parts.iter().map(|(label, v, c)| {
            h_flex()
                .min_h(px(36.))
                .gap(px(10.))
                .child(div().size(px(8.)).rounded(px(2.)).bg(*c))
                .child(div().flex_1().text_color(p.muted).child(label.to_string()))
                .child(
                    div()
                        .w(px(52.))
                        .text_right()
                        .text_size(px(size::META))
                        .text_color(p.faint)
                        .font_features(tabular())
                        .child(percent_label(*v, total).filter(|_| total > 0).unwrap_or_else(|| "—".into())),
                )
                .child(div().w(px(110.)).text_right().font_features(tabular()).text_color(p.text).child(money.format(*v)))
        });
        card(p)
            .when(!full_width, |d| d.w(px(360.)).flex_none())
            .child(card_header(p, "Where your money is", None))
            .child(
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::L))
                    .child(bar)
                    .child(v_flex().children(rows))
                    .child(div().h(px(1.)).bg(p.border))
                    .child(crate::ui::components::kv(p, "Expected interest still to come", money.format(s.expected_interest_remaining)))
                    .when(s.defaulted_principal > 0, |d| {
                        d.child(
                            h_flex()
                                .justify_between()
                                .child(div().text_color(p.negative).child("Defaulted, excluded"))
                                .child(div().text_color(p.negative).font_features(tabular()).child(money.format(s.defaulted_principal + s.defaulted_interest))),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(size::META))
                            .line_height(px(17.))
                            .text_color(p.faint)
                            .child("Portfolio value = available cash + principal lent out + interest accrued but not yet paid. Defaulted balances are left out."),
                    ),
            )
            .into_any_element()
    }

    fn recent_payments_card(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let recent: Vec<_> = self.snap.payments.iter().rev().filter(|x| !x.is_reversed()).take(5).collect();
        let rows = recent.iter().map(|pay| {
            let loan = self.snap.loan(pay.loan_id);
            let client_id = loan.map(|l| l.client_id).unwrap_or(0);
            let name = self.snap.client_name(client_id).to_string();
            let loan_id = pay.loan_id;
            let split = if pay.fee > 0 {
                format!("Interest {} · Principal {} · Fee {}", money.format(pay.interest), money.format(pay.principal), money.format(pay.fee))
            } else {
                format!("Interest {} · Principal {}", money.format(pay.interest), money.format(pay.principal))
            };
            h_flex()
                .id(("recent", pay.id as u64))
                .h(px(60.))
                .px(px(space::XL))
                .gap(px(space::M))
                .cursor_pointer()
                .hover(|s| s.bg(p.hover))
                .child(avatar(p, &name, client_id, 32.))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_color(p.text).truncate().child(name))
                        .child(div().text_size(px(size::META)).text_color(p.muted).child(format!("{} · Loan #{}", ds.format_short(pay.paid_on, self.today), pay.loan_id))),
                )
                .child(
                    v_flex()
                        .items_end()
                        .child(div().font_features(tabular()).text_color(p.positive).child(format!("+{}", money.format(pay.amount))))
                        .child(div().text_size(px(size::META)).font_features(tabular()).text_color(p.muted).child(split)),
                )
                .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Loan(loan_id), w, cx)))
        });
        let body = if recent.is_empty() {
            empty_state(p, IconName::Receipt, "No payments yet", "Payments you record show here with their split between interest and principal.", None)
                .into_any_element()
        } else {
            v_flex().pb(px(space::S)).children(rows).into_any_element()
        };
        card(p)
            .flex_1().flex_basis(px(0.)).min_w_0()
            .child(card_header(p, "Recent payments", Some(header_link(p, "recent-all", "View all", Route::Payments, cx))))
            .child(body)
            .into_any_element()
    }

    fn attention_card(&self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let mut items: Vec<_> = self
            .snap
            .book
            .loans
            .iter()
            .filter_map(|l| {
                let st = l.display_status(self.today);
                matches!(st, DisplayStatus::Overdue | DisplayStatus::DueSoon).then(|| (l, st, l.delinquency(self.today)))
            })
            .collect();
        items.sort_by(|a, b| a.1.cmp(&b.1).then(b.2.days_overdue.cmp(&a.2.days_overdue)).then(a.2.next_due.cmp(&b.2.next_due)));
        let count = items.len();
        let rows = items.iter().take(5).map(|(l, st, q)| {
            let name = self.snap.client_name(l.client_id).to_string();
            let loan_id = l.id;
            let pos = l.position(self.today);
            let due_text = format!("Loan #{}", l.id);
            let amount_line = match (st, q.next_due) {
                (DisplayStatus::Overdue, Some(d)) => format!("{} past due since {}", money.format(q.overdue_amount), ds.format_short(d, self.today)),
                (DisplayStatus::Overdue, None) => format!("{} past due", money.format(q.overdue_amount)),
                (_, Some(d)) => format!("{} due {}", money.format(q.next_due_amount), ds.format_short(d, self.today)),
                _ => format!("{} due", money.format(q.next_due_amount)),
            };
            h_flex()
                .id(("attn", loan_id as u64))
                .h(px(60.))
                .px(px(space::XL))
                .gap(px(space::M))
                .cursor_pointer()
                .hover(|s| s.bg(p.hover))
                .child(avatar(p, &name, l.client_id, 32.))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_color(p.text).truncate().child(name))
                        .child(div().text_size(px(size::META)).text_color(p.muted).font_features(tabular()).truncate().child(format!("{due_text} · {amount_line}"))),
                )
                .child(
                    v_flex()
                        .items_end()
                        .gap(px(3.))
                        .flex_none()
                        .child(div().font_features(tabular()).text_color(p.text).child(money.format(pos.payoff())))
                        .child(status_detail_pill(p, *st, q, self.today).h(px(20.))),
                )
                .child(
                    Button::new(("attn-pay", loan_id as u64))
                        .ghost()
                        .small()
                        .label("Record")
                        .on_click(cx.listener(move |this, _, w, cx| {
                            cx.stop_propagation();
                            this.open_payment_form(Some(loan_id), w, cx)
                        })),
                )
                .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Loan(loan_id), w, cx)))
        });
        let body = if count == 0 {
            empty_state(
                p,
                IconName::CircleCheck,
                "Everything is current",
                if self.snap.book.loans.is_empty() { "Loans that are overdue or due within 7 days will appear here." } else { "No loan is overdue or due within the next 7 days." },
                None,
            )
            .into_any_element()
        } else {
            v_flex().pb(px(space::S)).children(rows).into_any_element()
        };
        let title = if count > 0 { format!("Needs attention · {count}") } else { "Needs attention".to_string() };
        card(p)
            .flex_1().flex_basis(px(0.)).min_w_0()
            .child(card_header(p, title, Some(header_link(p, "attn-all", "View loans", Route::Loans, cx))))
            .child(body)
            .child(div().flex_1())
            .child(
                h_flex()
                    .px(px(space::XL))
                    .py(px(space::M))
                    .gap(px(6.))
                    .border_t_1()
                    .border_color(p.rule)
                    .text_size(px(size::META))
                    .text_color(p.faint)
                    .child("Record a payment from anywhere with")
                    .child(kbd(p, "Ctrl P")),
            )
            .into_any_element()
    }
}
