use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{crumb, ncolumn, segmented, status_detail_pill};
use crate::app::{LoanFilter, Route, VzApp};
use crate::domain::ledger::{ContractStatus, DisplayStatus};
use crate::domain::loans::InterestMethod;
use crate::domain::money::percent_label;
use crate::ui::components::{Tone, avatar, card, card_header, empty_state, figure, kv, label, page_header, pill, progress_bar, table_header, table_row, th, vrule};
use crate::ui::forms::reason::ReasonKind;
use crate::ui::theme::{palette, size, space, tabular};

impl VzApp {
    pub(crate) fn render_loans(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let narrow = self.content_width(window) < 1000.;
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let today = self.today;
        let s = self.snap.book.summary(today);

        let matches = |f: LoanFilter, st: DisplayStatus| match f {
            LoanFilter::Open => matches!(st, DisplayStatus::Active | DisplayStatus::DueSoon | DisplayStatus::Overdue),
            LoanFilter::Attention => matches!(st, DisplayStatus::DueSoon | DisplayStatus::Overdue),
            LoanFilter::PaidOff => st == DisplayStatus::PaidOff,
            LoanFilter::Defaulted => matches!(st, DisplayStatus::Defaulted | DisplayStatus::Restructured),
            LoanFilter::All => true,
        };
        let statuses: Vec<_> = self.snap.book.loans.iter().map(|l| (l, l.display_status(today))).collect();
        let count = |f: LoanFilter| statuses.iter().filter(|(_, st)| matches(f, *st)).count();
        let labels = [
            (LoanFilter::Open, format!("Active {}", count(LoanFilter::Open))),
            (LoanFilter::Attention, format!("Needs attention {}", count(LoanFilter::Attention))),
            (LoanFilter::PaidOff, format!("Paid off {}", count(LoanFilter::PaidOff))),
            (LoanFilter::Defaulted, format!("Defaulted {}", count(LoanFilter::Defaulted))),
            (LoanFilter::All, format!("All {}", statuses.len())),
        ];
        let options: Vec<(LoanFilter, &str)> = labels.iter().map(|(f, l)| (*f, l.as_str())).collect();
        let filter_el = segmented(&p, "loanfilter", &options, self.loan_filter, cx, |app, f, _, cx| {
            app.loan_filter = f;
            cx.notify();
        });

        let mut visible: Vec<_> = statuses.iter().filter(|(_, st)| matches(self.loan_filter, *st)).collect();
        visible.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| {
            let qa = a.0.delinquency(today);
            let qb = b.0.delinquency(today);
            qa.next_due.cmp(&qb.next_due)
        }));

        let header = page_header(
            &p,
            "Loans",
            Some(format!("{} active · {} outstanding principal", s.active_loans, money.format(s.outstanding_principal)).into_any_element()),
            vec![
                Button::new("loans-new")
                    .primary()
                    .icon(IconName::Plus)
                    .label("New loan")
                    .tooltip("Ctrl+L")
                    .on_click(cx.listener(|this, _, w, cx| this.open_loan_form(None, w, cx)))
                    .into_any_element(),
            ],
        );

        let rows = visible.iter().map(|(l, st)| {
            let lid = l.id;
            let pos = l.position(today);
            let q = l.delinquency(today);
            let name = self.snap.client_name(l.client_id).to_string();
            let loan = self.snap.loan(lid);
            let desc = loan.map(|x| format!("#{} · {} · {}", lid, x.terms.method.label(), x.terms.rate.percent_label())).unwrap_or_default();
            let ratio = if l.facts.principal > 0 { pos.principal_paid as f32 / l.facts.principal as f32 } else { 0.0 };
            let next = match (q.next_due, st) {
                (_, DisplayStatus::PaidOff) => "—".to_string(),
                (Some(d), _) => ds.format_short(d, today),
                (None, _) => "—".to_string(),
            };
            table_row(&p, ("loan", lid as u64), true)
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(200.))
                        .gap(px(space::M))
                        .child(avatar(&p, &name, l.client_id, 32.))
                        .child(
                            v_flex()
                                .min_w_0()
                                .child(div().truncate().child(name))
                                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(desc)),
                        ),
                )
                .when(!narrow, |d| d.child(ncolumn(130.).font_features(tabular()).text_color(p.muted).child(money.format(l.facts.principal))))
                .child(ncolumn(130.).font_features(tabular()).child(money.format(pos.payoff())))
                .child(
                    v_flex()
                        .w(px(130.))
                        .items_end()
                        .child(div().child(next))
                        .when(q.next_due_amount > 0, |d| {
                            d.child(div().text_size(px(size::META)).text_color(p.muted).font_features(tabular()).child(money.format(q.next_due_amount)))
                        }),
                )
                .when(!narrow, |d| {
                    d.child(
                        v_flex()
                            .w(px(120.))
                            .gap(px(4.))
                            .child(progress_bar(&p, ratio, p.accent))
                            .child(div().text_size(px(size::META)).text_color(p.muted).font_features(tabular()).child(format!(
                                "{} repaid",
                                percent_label(pos.principal_paid, l.facts.principal).unwrap_or_default()
                            ))),
                    )
                })
                .child(div().w(px(130.)).flex().justify_end().child(status_detail_pill(&p, *st, &q, today)))
                .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Loan(lid), w, cx)))
        });

        let body = if self.snap.loans.is_empty() {
            empty_state(
                &p,
                IconName::HandCoins,
                "No loans yet",
                "Create a loan to generate its repayment schedule. You'll see a preview before anything is saved.",
                Some(
                    Button::new("loans-empty-new")
                        .primary()
                        .label("New loan")
                        .on_click(cx.listener(|this, _, w, cx| this.open_loan_form(None, w, cx)))
                        .into_any_element(),
                ),
            )
            .into_any_element()
        } else if visible.is_empty() {
            empty_state(&p, IconName::CircleCheck, "Nothing here", "No loans match this filter.", None).into_any_element()
        } else {
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().flex_1().min_w(px(200.)).child(th(&p, "Client and loan")))
                        .when(!narrow, |d| d.child(ncolumn(130.).child(th(&p, "Principal"))))
                        .child(ncolumn(130.).child(th(&p, "Owed today")))
                        .child(ncolumn(130.).child(th(&p, "Next due")))
                        .when(!narrow, |d| d.child(div().w(px(120.)).child(th(&p, "Progress"))))
                        .child(ncolumn(130.).child(th(&p, "Status"))),
                )
                .children(rows)
                .into_any_element()
        };

        v_flex()
            .child(header)
            .child(div().pb(px(space::L)).child(filter_el))
            .child(card(&p).overflow_hidden().child(body))
            .into_any_element()
    }

    pub(crate) fn render_loan(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let narrow = self.content_width(window) < 1050.;
        let (Some(loan), Some(book)) = (self.snap.loan(id).cloned(), self.snap.book_loan(id).cloned()) else {
            return div().child("Loan not found").into_any_element();
        };
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let today = self.today;
        let pos = book.position(today);
        let q = book.delinquency(today);
        let status = book.display_status(today);
        let client_name = self.snap.client_name(loan.client_id).to_string();
        let client_id = loan.client_id;
        let t = &loan.terms;
        let active = loan.status == ContractStatus::Active;

        let mut actions = Vec::new();
        match loan.status {
            ContractStatus::Active => {
                actions.push(
                    Button::new("loan-default")
                        .ghost()
                        .label("Mark defaulted")
                        .on_click(cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::MarkDefaulted(id), w, cx)))
                        .into_any_element(),
                );
                actions.push(
                    Button::new("loan-pay")
                        .primary()
                        .icon(IconName::Plus)
                        .label("Record payment")
                        .on_click(cx.listener(move |this, _, w, cx| this.open_payment_form(Some(id), w, cx)))
                        .into_any_element(),
                );
            }
            ContractStatus::Defaulted => actions.push(
                Button::new("loan-reinstate")
                    .label("Reinstate loan")
                    .on_click(cx.listener(move |this, _, w, cx| this.reinstate_loan(id, w, cx)))
                    .into_any_element(),
            ),
            _ => {}
        }

        let frequency = if t.method == InterestMethod::Simple {
            format!("{} {}, single payment", t.term_count, t.frequency.term_unit(t.term_count))
        } else {
            format!("{} {} {}", t.term_count, t.frequency.label().to_lowercase(), t.frequency.term_unit(t.term_count))
        };
        let subtitle = h_flex()
            .gap(px(6.))
            .child(
                div()
                    .id("loan-client")
                    .text_color(p.accent_text)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(client_name.clone())
                    .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Client(client_id), w, cx))),
            )
            .child(format!("·  {}  ·  {} a year  ·  {}", t.method.label(), t.rate.percent_label(), frequency));
        let header = h_flex()
            .justify_between()
            .items_end()
            .pb(px(space::XL))
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .gap(px(space::M))
                            .text_size(px(size::TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Loan #{id}"))
                            .child(status_detail_pill(&p, status, &q, today)),
                    )
                    .child(div().text_color(p.muted).child(subtitle)),
            )
            .child(h_flex().gap(px(space::S)).children(actions));

        let cell = |l: &str, v: String, sub: String, lead: bool, color: Hsla| {
            v_flex()
                .flex_1()
                .flex_basis(px(0.))
                .min_w_0()
                .when(lead, |d| d.flex_grow(1.3))
                .px(px(if narrow { space::L } else { space::XL }))
                .py(px(space::L + 2.))
                .gap(px(6.))
                .child(label(&p, l.to_string()).truncate())
                .child(figure(&p, v, if lead { 26. } else if narrow { 19. } else { size::FIGURE }).text_color(color))
                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sub))
        };
        let payoff_label = match loan.status {
            ContractStatus::PaidOff => "Paid off",
            ContractStatus::Defaulted => "Balance at default",
            _ => "Payoff today",
        };
        let strip = card(&p)
            .flex_row()
            .child(cell(payoff_label, money.format(pos.payoff()), "Principal plus accrued interest".into(), true, p.text))
            .child(vrule(&p))
            .child(cell("Remaining principal", money.format(pos.principal_outstanding), format!("of {} lent", money.format(t.principal)), false, p.text))
            .child(vrule(&p))
            .child(cell("Outstanding interest", money.format(pos.interest_outstanding), "Accrued, not yet paid".into(), false, p.text))
            .child(vrule(&p))
            .child(cell("Interest paid", money.format(pos.interest_paid), format!("of {} accrued to date", money.format(pos.interest_accrued_total)), false, p.text))
            .child(vrule(&p))
            .child(cell("Principal repaid", money.format(pos.principal_paid), "Capital returned".into(), false, p.text));

        let ratio = pos.principal_paid as f32 / t.principal.max(1) as f32;
        let next_line = match (q.next_due, active) {
            (Some(d), true) => format!("{} on {}", money.format(q.next_due_amount), ds.format(d)),
            _ => "—".to_string(),
        };
        let details = card(&p)
            .child(card_header(&p, "Repayment progress", None))
            .child(
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::M))
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .child(progress_bar(&p, ratio, if loan.status == ContractStatus::PaidOff { p.positive } else { p.accent }))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .text_size(px(size::META))
                                    .text_color(p.muted)
                                    .font_features(tabular())
                                    .child(format!("{} of principal repaid", percent_label(pos.principal_paid, t.principal).unwrap_or_default()))
                                    .child(format!("{} left", money.format(pos.principal_outstanding))),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(kv(&p, "Original principal", money.format(t.principal)))
                            .child(kv(&p, "Annual interest rate", t.rate.percent_label()))
                            .child(kv(&p, "Interest method", t.method.label()))
                            .child(kv(&p, "Interest accrued to date", money.format(pos.interest_accrued_total)))
                            .child(kv(&p, "Next scheduled payment", next_line))
                            .when(q.overdue_amount > 0, |d| {
                                d.child(kv(
                                    &p,
                                    "Past due",
                                    div().text_color(p.negative).child(format!("{} · {} days", money.format(q.overdue_amount), q.days_overdue)),
                                ))
                            })
                            .child(kv(&p, "Start date", ds.format(t.start_date)))
                            .child(kv(&p, "Maturity date", ds.format(loan.maturity_date))),
                    )
                    .when(!loan.notes.is_empty(), |d| {
                        d.child(v_flex().gap(px(4.)).pt(px(space::S)).child(label(&p, "Notes")).child(div().child(loan.notes.clone())))
                    }),
            );

        // Schedule with coverage status per installment.
        let paid_ex_fees = pos.paid_total - pos.fees_paid;
        let mut cumulative = 0;
        let schedule_rows = book.schedule.iter().map(|r| {
            let before = cumulative;
            cumulative += r.total();
            let (text, tone) = if loan.status == ContractStatus::PaidOff {
                if paid_ex_fees >= cumulative { ("Paid", Tone::Positive) } else { ("Settled", Tone::Positive) }
            } else if paid_ex_fees >= cumulative {
                ("Paid", Tone::Positive)
            } else if paid_ex_fees > before {
                if r.due_date < today { ("Partly paid", Tone::Negative) } else { ("Partly paid", Tone::Warning) }
            } else if r.due_date < today {
                ("Overdue", Tone::Negative)
            } else if (r.due_date - today).num_days() <= 7 {
                ("Due soon", Tone::Warning)
            } else {
                ("Upcoming", Tone::Neutral)
            };
            h_flex()
                .h(px(40.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .child(div().w(px(28.)).text_color(p.faint).child(r.seq.to_string()))
                .child(div().flex_1().child(ds.format(r.due_date)))
                .child(ncolumn(120.).text_color(p.muted).child(money.format(r.principal)))
                .child(ncolumn(110.).text_color(p.muted).child(money.format(r.interest)))
                .child(ncolumn(120.).child(money.format(r.total())))
                .child(div().w(px(100.)).flex().justify_end().child(pill(&p, text, tone)))
        });
        let schedule_card = card(&p)
            .overflow_hidden()
            .child(card_header(
                &p,
                "Schedule",
                Some(div().text_size(px(size::META)).text_color(p.muted).child("Contractual, if paid on each due date").into_any_element()),
            ))
            .child(
                table_header(&p)
                    .child(div().w(px(28.)).child(th(&p, "#")))
                    .child(div().flex_1().child(th(&p, "Due date")))
                    .child(ncolumn(120.).child(th(&p, "Principal")))
                    .child(ncolumn(110.).child(th(&p, "Interest")))
                    .child(ncolumn(120.).child(th(&p, "Payment")))
                    .child(ncolumn(100.).child(th(&p, "Status"))),
            )
            .child(div().id("schedule-scroll").max_h(px(440.)).overflow_y_scroll().children(schedule_rows));

        // Transactions: newest first. Only the latest live payment can be reversed.
        let payments: Vec<_> = self.snap.payments_for_loan(id).cloned().collect();
        let latest_live = payments.iter().filter(|x| !x.is_reversed()).map(|x| x.id).max_by_key(|pid| {
            payments.iter().position(|x| x.id == *pid).unwrap_or(0)
        });
        let can_reverse = matches!(loan.status, ContractStatus::Active | ContractStatus::PaidOff);
        let tx_rows = payments.iter().rev().map(|x| {
            let reversed = x.is_reversed();
            let pid = x.id;
            h_flex()
                .min_h(px(52.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .child(
                    v_flex()
                        .w(px(150.))
                        .child(div().text_color(if reversed { p.muted } else { p.text }).child(ds.format(x.paid_on)))
                        .child(div().text_size(px(size::META)).text_color(p.faint).child(format!("Payment #{pid}"))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(size::META + 0.5))
                        .text_color(p.muted)
                        .truncate()
                        .child(if reversed {
                            format!("Reversed — {}", x.reversal_reason.clone().unwrap_or_default())
                        } else if x.notes.is_empty() {
                            String::new()
                        } else {
                            x.notes.clone()
                        }),
                )
                .child(ncolumn(110.).text_color(p.muted).when(reversed, |d| d.line_through()).child(money.format(x.interest)))
                .child(ncolumn(120.).text_color(p.muted).when(reversed, |d| d.line_through()).child(money.format(x.principal)))
                .child(ncolumn(120.).text_color(if reversed { p.muted } else { p.text }).when(reversed, |d| d.line_through()).child(money.format(x.amount)))
                .child(div().w(px(96.)).flex().justify_end().child(if reversed {
                    pill(&p, "Reversed", Tone::Neutral).into_any_element()
                } else if can_reverse && Some(pid) == latest_live {
                    Button::new(("reverse", pid as u64))
                        .ghost()
                        .small()
                        .label("Reverse")
                        .on_click(cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::ReversePayment(pid), w, cx)))
                        .into_any_element()
                } else {
                    div().into_any_element()
                }))
        });
        let tx_card = card(&p)
            .overflow_hidden()
            .child(card_header(&p, "Transactions", None))
            .child(if payments.is_empty() {
                empty_state(
                    &p,
                    IconName::Receipt,
                    "No payments yet",
                    "Each payment shows how much went to interest and how much returned your capital.",
                    active.then(|| {
                        Button::new("loan-empty-pay")
                            .primary()
                            .label("Record payment")
                            .on_click(cx.listener(move |this, _, w, cx| this.open_payment_form(Some(id), w, cx)))
                            .into_any_element()
                    }),
                )
                .into_any_element()
            } else {
                v_flex()
                    .child(
                        table_header(&p)
                            .child(div().w(px(150.)).child(th(&p, "Date")))
                            .child(div().flex_1().child(th(&p, "Note")))
                            .child(ncolumn(110.).child(th(&p, "Interest")))
                            .child(ncolumn(120.).child(th(&p, "Principal")))
                            .child(ncolumn(120.).child(th(&p, "Amount")))
                            .child(div().w(px(96.))),
                    )
                    .children(tx_rows)
                    .into_any_element()
            });

        v_flex()
            .child(crumb(&p, "Loans", Route::Loans, cx))
            .child(header)
            .child(
                v_flex()
                    .gap(px(space::XL))
                    .child(strip)
                    .child(if narrow {
                        v_flex().gap(px(space::XL)).child(details).child(schedule_card).into_any_element()
                    } else {
                        h_flex()
                            .items_start()
                            .gap(px(space::XL))
                            .child(v_flex().w(px(380.)).flex_none().child(details))
                            .child(v_flex().flex_1().min_w_0().child(schedule_card))
                            .into_any_element()
                    })
                    .child(tx_card),
            )
            .into_any_element()
    }
}
