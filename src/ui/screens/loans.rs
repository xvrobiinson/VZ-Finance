use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{crumb, ncolumn, segmented, status_detail_pill};
use crate::app::{LoanFilter, Route, VzApp};
use crate::domain::ledger::{ContractStatus, DisplayStatus, PeriodState, owed_at_payment, period_states};
use crate::domain::loans::period_interest;
use crate::domain::money::percent_label;
use crate::i18n::{count, t};
use crate::tr;
use crate::ui::components::{
    Tone, avatar, card, card_header, empty_state, figure, kv, label, page_header, pill, progress_bar, table_header, table_row, th, vrule,
};
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
        let n = |f: LoanFilter| statuses.iter().filter(|(_, st)| matches(f, *st)).count();
        let labels = [
            (LoanFilter::Open, format!("{} {}", t("Active", "Activos"), n(LoanFilter::Open))),
            (LoanFilter::Attention, format!("{} {}", t("Needs attention", "Requieren atención"), n(LoanFilter::Attention))),
            (LoanFilter::PaidOff, format!("{} {}", t("Paid off", "Saldados"), n(LoanFilter::PaidOff))),
            (LoanFilter::Defaulted, format!("{} {}", t("Defaulted", "Incobrables"), n(LoanFilter::Defaulted))),
            (LoanFilter::All, format!("{} {}", t("All", "Todos"), statuses.len())),
        ];
        let options: Vec<(LoanFilter, &str)> = labels.iter().map(|(f, l)| (*f, l.as_str())).collect();
        let filter_el = segmented(&p, "loanfilter", &options, self.loan_filter, cx, |app, f, _, cx| {
            app.loan_filter = f;
            cx.notify();
        });

        let mut visible: Vec<_> = statuses.iter().filter(|(_, st)| matches(self.loan_filter, *st)).collect();
        visible.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.delinquency(today).next_due.cmp(&b.0.delinquency(today).next_due)));

        let subtitle = {
            let active = count(s.active_loans, "active loan", "active loans", "préstamo activo", "préstamos activos");
            let out = money.format(s.outstanding_principal);
            tr!("{active} · {out} of capital lent out", "{active} · {out} de capital prestado")
        };
        let header = page_header(
            &p,
            t("Loans", "Préstamos"),
            Some(subtitle.into_any_element()),
            vec![
                Button::new("loans-new")
                    .primary()
                    .icon(IconName::Plus)
                    .label(t("New loan", "Nuevo préstamo"))
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
            let desc = format!("#{lid} · {} {}", l.facts.rate.percent_label(), l.facts.frequency.per());
            let ratio = if l.facts.principal > 0 { pos.principal_paid as f32 / l.facts.principal as f32 } else { 0.0 };
            let next = match (q.next_due, st) {
                (_, DisplayStatus::PaidOff | DisplayStatus::Defaulted) => "—".to_string(),
                (Some(d), _) => ds.format_short(d, today),
                (None, _) => "—".to_string(),
            };
            table_row(&p, ("loan", lid as u64), true)
                .child(
                    h_flex().flex_1().min_w(px(200.)).gap(px(space::M)).child(avatar(&p, &name, l.client_id, 32.)).child(
                        v_flex()
                            .min_w_0()
                            .child(div().truncate().child(name))
                            .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(desc)),
                    ),
                )
                .when(!narrow, |d| {
                    d.child(ncolumn(130.).font_features(tabular()).text_color(p.muted).child(money.format(l.facts.principal)))
                })
                .child(ncolumn(130.).font_features(tabular()).child(money.format(pos.payoff())))
                .child(v_flex().w(px(130.)).items_end().child(div().child(next)).when(
                    q.next_due_amount > 0 && l.facts.status == ContractStatus::Active,
                    |d| {
                        d.child(
                            div()
                                .text_size(px(size::META))
                                .text_color(p.muted)
                                .font_features(tabular())
                                .child(money.format(q.next_due_amount)),
                        )
                    },
                ))
                .when(!narrow, |d| {
                    d.child(v_flex().w(px(120.)).gap(px(4.)).child(progress_bar(&p, ratio, p.accent)).child(
                        div().text_size(px(size::META)).text_color(p.muted).font_features(tabular()).child({
                            let pct = percent_label(pos.principal_paid, l.facts.principal).unwrap_or_default();
                            tr!("{pct} repaid", "{pct} pagado")
                        }),
                    ))
                })
                .child(div().w(px(130.)).flex().justify_end().child(status_detail_pill(&p, *st, &q, today)))
                .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Loan(lid), w, cx)))
        });

        let body = if self.snap.loans.is_empty() {
            empty_state(
                &p,
                IconName::HandCoins,
                t("No loans yet", "Aún no hay préstamos"),
                t(
                    "Create a loan to start tracking its interest and payments. You'll see the interest per period before anything is saved.",
                    "Crea un préstamo para seguir sus intereses y pagos. Verás el interés por período antes de guardar.",
                ),
                Some(
                    Button::new("loans-empty-new")
                        .primary()
                        .label(t("New loan", "Nuevo préstamo"))
                        .on_click(cx.listener(|this, _, w, cx| this.open_loan_form(None, w, cx)))
                        .into_any_element(),
                ),
            )
            .into_any_element()
        } else if visible.is_empty() {
            empty_state(
                &p,
                IconName::CircleCheck,
                t("Nothing here", "Nada por aquí"),
                t("No loans match this filter.", "Ningún préstamo coincide con este filtro."),
                None,
            )
            .into_any_element()
        } else {
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().flex_1().min_w(px(200.)).child(th(&p, t("Client and loan", "Cliente y préstamo"))))
                        .when(!narrow, |d| d.child(ncolumn(130.).child(th(&p, t("Lent", "Prestado")))))
                        .child(ncolumn(130.).child(th(&p, t("Owes today", "Debe hoy"))))
                        .child(ncolumn(130.).child(th(&p, t("Next payment", "Próximo pago"))))
                        .when(!narrow, |d| d.child(div().w(px(120.)).child(th(&p, t("Capital repaid", "Capital pagado")))))
                        .child(ncolumn(130.).child(th(&p, t("Status", "Estado")))),
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
            return div().child(t("Loan not found", "Préstamo no encontrado")).into_any_element();
        };
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let today = self.today;
        let pos = book.position(today);
        let owed_now = owed_at_payment(&book.facts, &book.payments, today);
        let q = book.delinquency(today);
        let status = book.display_status(today);
        let client_name = self.snap.client_name(loan.client_id).to_string();
        let client_id = loan.client_id;
        let terms = &loan.terms;
        let active = loan.status == ContractStatus::Active;

        let mut actions = Vec::new();
        match loan.status {
            ContractStatus::Active => {
                actions.push(
                    Button::new("loan-default")
                        .ghost()
                        .label(t("Mark defaulted", "Marcar incobrable"))
                        .on_click(cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::MarkDefaulted(id), w, cx)))
                        .into_any_element(),
                );
                actions.push(
                    Button::new("loan-pay")
                        .primary()
                        .icon(IconName::Plus)
                        .label(t("Record payment", "Registrar pago"))
                        .on_click(cx.listener(move |this, _, w, cx| this.open_payment_form(Some(id), w, cx)))
                        .into_any_element(),
                );
            }
            ContractStatus::Defaulted => actions.push(
                Button::new("loan-reinstate")
                    .label(t("Reinstate loan", "Reactivar préstamo"))
                    .on_click(cx.listener(move |this, _, w, cx| this.reinstate_loan(id, w, cx)))
                    .into_any_element(),
            ),
            _ => {}
        }

        let rate_line = format!("{} {}", terms.rate.percent_label(), terms.frequency.per());
        let since = ds.format(terms.start_date);
        let lent = money.format(terms.principal);
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
            .child(tr!("·  {lent} at {rate_line}  ·  since {since}", "·  {lent} al {rate_line}  ·  desde {since}"));
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
                            .child(tr!("Loan #{id}", "Préstamo #{id}"))
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
                .child(
                    figure(
                        &p,
                        v,
                        if lead {
                            26.
                        } else if narrow {
                            19.
                        } else {
                            size::FIGURE
                        },
                    )
                    .text_color(color),
                )
                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(sub))
        };
        let owes_label = match loan.status {
            ContractStatus::PaidOff => t("Paid off", "Saldado"),
            ContractStatus::Defaulted => t("Balance at default", "Saldo al marcarlo"),
            _ => t("Owes today", "Debe hoy"),
        };
        let interest_now = period_interest(owed_now.principal_outstanding, terms.rate);
        let strip = card(&p)
            .flex_row()
            .child(cell(
                owes_label,
                money.format(owed_now.payoff()),
                t("Capital plus interest due", "Capital más interés por pagar").into(),
                true,
                p.text,
            ))
            .child(vrule(&p))
            .child(cell(
                t("Capital owed", "Capital adeudado"),
                money.format(owed_now.principal_outstanding),
                tr!("of {lent} lent", "de {lent} prestado"),
                false,
                p.text,
            ))
            .child(vrule(&p))
            .child(cell(
                t("Interest due", "Interés por pagar"),
                money.format(owed_now.interest_outstanding),
                if q.overdue_amount > 0 {
                    let a = money.format(q.overdue_amount);
                    tr!("{a} overdue", "{a} atrasado")
                } else {
                    t("Up to date", "Al día").to_string()
                },
                false,
                if q.overdue_amount > 0 { p.negative } else { p.text },
            ))
            .child(vrule(&p))
            .child(cell(
                t("Interest paid", "Interés pagado"),
                money.format(pos.interest_paid),
                t("Income from this loan", "Ganancia de este préstamo").into(),
                false,
                p.text,
            ))
            .child(vrule(&p))
            .child(cell(
                t("Capital repaid", "Capital pagado"),
                money.format(pos.principal_paid),
                t("Money returned to you", "Dinero que regresó").into(),
                false,
                p.text,
            ));

        let ratio = pos.principal_paid as f32 / terms.principal.max(1) as f32;
        let next_line = match (q.next_due, active) {
            (Some(d), true) => {
                let (a, when) = (money.format(q.next_due_amount), ds.format(d));
                tr!("{a} on {when}", "{a} el {when}")
            }
            _ => "—".to_string(),
        };
        let details = card(&p).child(card_header(&p, t("Summary", "Resumen"), None)).child(
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
                                .child({
                                    let pct = percent_label(pos.principal_paid, terms.principal).unwrap_or_default();
                                    tr!("{pct} of capital repaid", "{pct} del capital pagado")
                                })
                                .child({
                                    let left = money.format(pos.principal_outstanding);
                                    tr!("{left} left", "faltan {left}")
                                }),
                        ),
                )
                .child(
                    v_flex()
                        .child(kv(&p, t("Amount lent", "Monto prestado"), money.format(terms.principal)))
                        .child(kv(&p, t("Interest rate", "Tasa de interés"), rate_line.clone()))
                        .child(kv(&p, t("Interest per period now", "Interés por período ahora"), money.format(interest_now)))
                        .child(kv(&p, t("Next payment", "Próximo pago"), next_line))
                        .when(q.overdue_amount > 0, |d| {
                            d.child(kv(
                                &p,
                                t("Overdue", "Atrasado"),
                                div().text_color(p.negative).child({
                                    let (a, days) = (money.format(q.overdue_amount), q.days_overdue);
                                    tr!("{a} · {days} days", "{a} · {days} días")
                                }),
                            ))
                        })
                        .child(kv(
                            &p,
                            t("Total interest charged so far", "Interés generado hasta hoy"),
                            money.format(pos.interest_charged_total),
                        ))
                        .child(kv(&p, t("Loan date", "Fecha del préstamo"), ds.format(terms.start_date))),
                )
                .when(!loan.notes.is_empty(), |d| {
                    d.child(
                        v_flex().gap(px(4.)).pt(px(space::S)).child(label(&p, t("Notes", "Notas"))).child(div().child(loan.notes.clone())),
                    )
                }),
        );

        // Period by period: the capital it was charged on and whether it is paid.
        let states = period_states(&pos, today);
        let mut period_rows: Vec<AnyElement> = states
            .iter()
            .rev()
            .map(|(period, state, paid)| {
                let (text, tone) = match state {
                    PeriodState::Paid => (t("Paid", "Pagado"), Tone::Positive),
                    PeriodState::PartlyPaid => (t("Partly paid", "Pago parcial"), Tone::Warning),
                    PeriodState::Overdue if *paid > 0 => (t("Partly paid", "Pago parcial"), Tone::Negative),
                    PeriodState::Overdue => (t("Overdue", "Atrasado"), Tone::Negative),
                    PeriodState::Open => (t("Current", "En curso"), Tone::Neutral),
                };
                h_flex()
                    .h(px(42.))
                    .px(px(space::XL))
                    .gap(px(space::L))
                    .border_b_1()
                    .border_color(p.rule)
                    .font_features(tabular())
                    .child(div().w(px(28.)).text_color(p.faint).child(period.seq.to_string()))
                    .child(div().flex_1().min_w_0().truncate().child(ds.format(period.due)))
                    .child(ncolumn(130.).text_color(p.muted).child(money.format(period.balance)))
                    .child(ncolumn(110.).child(money.format(period.interest)))
                    .child(ncolumn(110.).text_color(p.muted).child(money.format(*paid)))
                    .child(div().w(px(110.)).flex().justify_end().child(pill(&p, text, tone)))
                    .into_any_element()
            })
            .collect();
        if active && pos.principal_outstanding > 0 {
            // The next period, projected on today's capital.
            let next_seq = pos.periods.last().map(|x| x.seq + 1).unwrap_or(1);
            let due = terms.frequency.due_date(terms.start_date, next_seq);
            period_rows.insert(
                0,
                h_flex()
                    .h(px(42.))
                    .px(px(space::XL))
                    .gap(px(space::L))
                    .border_b_1()
                    .border_color(p.rule)
                    .font_features(tabular())
                    .text_color(p.muted)
                    .child(div().w(px(28.)).text_color(p.faint).child(next_seq.to_string()))
                    .child(div().flex_1().min_w_0().truncate().child(ds.format(due)))
                    .child(ncolumn(130.).child(money.format(pos.principal_outstanding)))
                    .child(ncolumn(110.).child(money.format(interest_now)))
                    .child(ncolumn(110.).child("—"))
                    .child(div().w(px(110.)).flex().justify_end().child(pill(&p, t("Upcoming", "Próximo"), Tone::Neutral)))
                    .into_any_element(),
            );
        }
        let periods_card = card(&p)
            .overflow_hidden()
            .child(card_header(
                &p,
                t("Interest by period", "Interés por período"),
                Some(
                    div()
                        .text_size(px(size::META))
                        .text_color(p.muted)
                        .child(t(
                            "Each period's interest is charged on the capital owed when it starts",
                            "El interés de cada período se calcula sobre el capital adeudado al iniciar",
                        ))
                        .into_any_element(),
                ),
            ))
            .child(
                table_header(&p)
                    .child(div().w(px(28.)).child(th(&p, "#")))
                    .child(div().flex_1().child(th(&p, t("Due date", "Vence"))))
                    .child(ncolumn(130.).child(th(&p, t("Capital", "Capital"))))
                    .child(ncolumn(110.).child(th(&p, t("Interest", "Interés"))))
                    .child(ncolumn(110.).child(th(&p, t("Paid", "Pagado"))))
                    .child(ncolumn(110.).child(th(&p, t("Status", "Estado")))),
            )
            .child(div().id("periods-scroll").max_h(px(440.)).overflow_y_scroll().children(period_rows));

        // Transactions, newest first. Only the latest live payment can be reversed.
        let payments: Vec<_> = self.snap.payments_for_loan(id).cloned().collect();
        let latest_live = payments.iter().rfind(|x| !x.is_reversed()).map(|x| x.id);
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
                        .child(div().text_size(px(size::META)).text_color(p.faint).child(tr!("Receipt #{pid}", "Recibo #{pid}"))),
                )
                .child(div().flex_1().min_w_0().text_size(px(size::META + 0.5)).text_color(p.muted).truncate().child(if reversed {
                    let why = x.reversal_reason.clone().unwrap_or_default();
                    tr!("Reversed — {why}", "Anulado — {why}")
                } else {
                    x.notes.clone()
                }))
                .child(ncolumn(110.).text_color(p.muted).when(reversed, |d| d.line_through()).child(money.format(x.interest)))
                .child(ncolumn(110.).text_color(p.muted).when(reversed, |d| d.line_through()).child(money.format(x.principal)))
                .child(
                    ncolumn(120.)
                        .text_color(if reversed { p.muted } else { p.text })
                        .when(reversed, |d| d.line_through())
                        .child(money.format(x.amount)),
                )
                .child(
                    h_flex()
                        .w(px(170.))
                        .justify_end()
                        .gap(px(4.))
                        .child(
                            Button::new(("receipt", pid as u64))
                                .ghost()
                                .small()
                                .icon(IconName::Printer)
                                .label(t("Receipt", "Recibo"))
                                .on_click(cx.listener(move |this, _, w, cx| this.open_receipt(pid, w, cx))),
                        )
                        .child(if reversed {
                            pill(&p, t("Reversed", "Anulado"), Tone::Neutral).into_any_element()
                        } else if can_reverse && Some(pid) == latest_live {
                            Button::new(("reverse", pid as u64))
                                .ghost()
                                .small()
                                .label(t("Reverse", "Anular"))
                                .on_click(cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::ReversePayment(pid), w, cx)))
                                .into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
        });
        let tx_card = card(&p).overflow_hidden().child(card_header(&p, t("Payments", "Pagos"), None)).child(if payments.is_empty() {
            empty_state(
                &p,
                IconName::Receipt,
                t("No payments yet", "Aún no hay pagos"),
                t(
                    "Each payment shows how much went to interest and how much to capital.",
                    "Cada pago muestra cuánto fue a interés y cuánto a capital.",
                ),
                active.then(|| {
                    Button::new("loan-empty-pay")
                        .primary()
                        .label(t("Record payment", "Registrar pago"))
                        .on_click(cx.listener(move |this, _, w, cx| this.open_payment_form(Some(id), w, cx)))
                        .into_any_element()
                }),
            )
            .into_any_element()
        } else {
            v_flex()
                .child(
                    table_header(&p)
                        .child(div().w(px(150.)).child(th(&p, t("Date", "Fecha"))))
                        .child(div().flex_1().child(th(&p, t("Note", "Nota"))))
                        .child(ncolumn(110.).child(th(&p, t("Interest", "Interés"))))
                        .child(ncolumn(110.).child(th(&p, t("Capital", "Capital"))))
                        .child(ncolumn(120.).child(th(&p, t("Amount", "Monto"))))
                        .child(div().w(px(170.))),
                )
                .children(tx_rows)
                .into_any_element()
        });

        v_flex()
            .child(crumb(&p, t("Loans", "Préstamos"), Route::Loans, cx))
            .child(header)
            .child(
                v_flex()
                    .gap(px(space::XL))
                    .child(strip)
                    .child(if narrow {
                        v_flex().gap(px(space::XL)).child(details).child(periods_card).into_any_element()
                    } else {
                        h_flex()
                            .items_start()
                            .gap(px(space::XL))
                            .child(v_flex().w(px(380.)).flex_none().child(details))
                            .child(v_flex().flex_1().min_w_0().child(periods_card))
                            .into_any_element()
                    })
                    .child(tx_card),
            )
            .into_any_element()
    }
}
