use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{crumb, ncolumn, row_icon_button, status_detail_pill};
use crate::app::{LoanFilter, Route, VzApp};
use crate::database::repositories::loans::Loan;
use crate::domain::ledger::{ContractStatus, DisplayStatus, PeriodState, owed_at_payment, period_states};
use crate::domain::money::{Cents, percent_label};
use crate::domain::portfolio::LoanBook;
use crate::i18n::{count, t};
use crate::tr;
use crate::ui::components::{Tone, avatar, empty_state, kv, page_header, pill, progress_bar, sel, table_header, table_row, th};
use crate::ui::forms::reason::ReasonKind;
use crate::ui::kit::{Button, Card, Dropdown, DropdownItem, Segmented, Stat, StatStrip};
use crate::ui::theme::{Palette, Typeset as _, Width, palette, space, tabular, text};

/// A muted caption for the right of a card header.
fn header_note(p: &Palette, note: impl Into<SharedString>) -> Div {
    div().typeset(text::CAPTION).text_color(p.muted).child(note.into())
}

impl VzApp {
    pub(crate) fn render_loans(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let w = Width::of(self.content_width(window));
        // Lent and the repayment bar need room; below Wide they go, and on
        // Compact the next payment does too (the status pill names it).
        let full = w >= Width::Wide;
        let show_next = w > Width::Compact;
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
        // Five counted segments need about 830px; narrower, the filter is a select.
        let filter_el = if full {
            let options: Vec<(LoanFilter, &str)> = labels.iter().map(|(f, l)| (*f, l.as_str())).collect();
            Segmented::pick("loanfilter", options.iter().copied(), self.loan_filter, cx, |app, f, _, cx| {
                app.loan_filter = f;
                cx.notify();
            })
            .into_any_element()
        } else {
            let current = labels.iter().find(|(f, _)| *f == self.loan_filter).map(|(_, l)| l.clone()).unwrap_or_default();
            Dropdown::new("loanfilter")
                .label(current)
                .items(labels.iter().map(|(f, l)| {
                    let f = *f;
                    DropdownItem::new(
                        l.clone(),
                        cx.listener(move |app, _, _, cx| {
                            app.loan_filter = f;
                            cx.notify();
                        }),
                    )
                    .selected(f == self.loan_filter)
                }))
                .into_any_element()
        };

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
            let desc = match self.snap.loan(lid) {
                Some(loan) => match loan.terms.installments {
                    Some(n) => {
                        let rate = loan.terms.rate_line();
                        let every = loan.terms.frequency.label().to_lowercase();
                        tr!("#{lid} · {rate} · {n} installments, {every}", "#{lid} · {rate} · {n} cuotas, {every}")
                    }
                    None => format!("#{lid} · {}", loan.terms.rate_line()),
                },
                None => format!("#{lid}"),
            };
            let ratio = if l.facts.principal > 0 { pos.principal_paid as f32 / l.facts.principal as f32 } else { 0.0 };
            let next = match (q.next_due, st) {
                (_, DisplayStatus::PaidOff | DisplayStatus::Defaulted) => "—".to_string(),
                (Some(d), _) => ds.format_short(d, today),
                (None, _) => "—".to_string(),
            };
            table_row(&p, ("loan", lid as u64), true)
                .child(
                    h_flex().flex_1().min_w(px(180.)).gap(px(space::M)).child(avatar(&p, &name, l.client_id, 32.)).child(
                        v_flex()
                            .min_w_0()
                            .child(div().truncate().child(sel(name)))
                            .child(div().typeset(text::CAPTION).text_color(p.muted).truncate().child(sel(desc))),
                    ),
                )
                .when(full, |d| d.child(ncolumn(130.).text_color(p.muted).child(sel(money.format(l.facts.principal)))))
                .child(ncolumn(130.).child(sel(money.format(pos.payoff()))))
                .when(show_next, |d| {
                    d.child(v_flex().w(px(130.)).flex_none().items_end().font_features(tabular()).child(div().child(sel(next))).when(
                        q.next_due_amount > 0 && l.facts.status == ContractStatus::Active,
                        |d| {
                            d.child(
                                div()
                                    .typeset(text::CAPTION)
                                    .text_color(p.muted)
                                    .whitespace_nowrap()
                                    .child(sel(money.format(q.next_due_amount))),
                            )
                        },
                    ))
                })
                .when(full, |d| {
                    d.child(v_flex().w(px(120.)).flex_none().gap(px(4.)).child(progress_bar(&p, ratio, p.accent)).child(
                        div().typeset(text::CAPTION).text_color(p.muted).font_features(tabular()).child(sel({
                            let pct = percent_label(pos.principal_paid, l.facts.principal).unwrap_or_default();
                            tr!("{pct} repaid", "{pct} pagado")
                        })),
                    ))
                })
                .child(div().w(px(130.)).flex_none().flex().justify_end().child(status_detail_pill(&p, *st, &q, today)))
                .on_click(cx.listener(move |this, _, w, cx| this.open_row(Route::Loan(lid), w, cx)))
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
                        .child(div().flex_1().min_w(px(180.)).child(th(&p, t("Client and loan", "Cliente y préstamo"))))
                        .when(full, |d| d.child(ncolumn(130.).child(th(&p, t("Lent", "Prestado")))))
                        .child(ncolumn(130.).child(th(&p, t("Owes today", "Debe hoy"))))
                        .when(show_next, |d| d.child(ncolumn(130.).child(th(&p, t("Next payment", "Próximo pago")))))
                        .when(full, |d| d.child(div().w(px(120.)).flex_none().child(th(&p, t("Capital repaid", "Capital pagado")))))
                        .child(ncolumn(130.).child(th(&p, t("Status", "Estado")))),
                )
                .children(rows)
                .into_any_element()
        };

        v_flex()
            .child(header)
            .child(div().pb(px(space::L)).child(filter_el))
            .child(Card::new().overflow_hidden().child(body))
            .into_any_element()
    }

    pub(crate) fn render_loan(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let w = Width::of(self.content_width(window));
        let gap = px(w.gap());
        let side_by_side = w >= Width::Wide;
        let (Some(loan), Some(book)) = (self.snap.loan(id).cloned(), self.snap.book_loan(id).cloned()) else {
            return div().child(t("Loan not found", "Préstamo no encontrado")).into_any_element();
        };
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let today = self.today;
        let pos = book.position(today);
        let plan_state = book.plan_state(today);
        let owed_now = match &plan_state {
            Some(s) => s.position(),
            None => owed_at_payment(&book.facts, &book.payments, today),
        };
        let q = book.delinquency(today);
        let status = book.display_status(today);
        let terms = &loan.terms;
        let active = loan.status == ContractStatus::Active;

        let header = h_flex()
            .flex_wrap()
            .justify_between()
            .items_end()
            .gap(px(space::L))
            .pb(px(space::XL))
            .child(
                v_flex()
                    .gap(px(6.))
                    .min_w_0()
                    .child(
                        h_flex()
                            .gap(px(space::M))
                            .child(div().typeset(text::TITLE).text_color(p.text).child(sel(tr!("Loan #{id}", "Préstamo #{id}"))))
                            .child(status_detail_pill(&p, status, &q, today)),
                    )
                    .child(div().text_color(p.muted).child(self.loan_subtitle(&p, &loan, cx))),
            )
            .child(h_flex().gap(px(space::S)).flex_none().children(self.loan_actions(id, loan.status, w, cx)));

        let owes_label = match loan.status {
            ContractStatus::PaidOff => t("Paid off", "Saldado"),
            ContractStatus::Defaulted => t("Balance at default", "Saldo al marcarlo"),
            _ => t("Owes today", "Debe hoy"),
        };
        let lent = money.format(terms.principal);
        let rate_line = terms.rate_line();
        let interest_now = book.facts.rate.interest(owed_now.principal_outstanding);
        let installment_loan = plan_state.is_some();
        let third = if installment_loan {
            let fees_due = owed_now.fees_outstanding;
            Stat::new(t("Overdue now", "Atrasado ahora"), money.format(q.overdue_amount)).caption(sel(match (q.overdue_since, fees_due) {
                (Some(_), f) if f > 0 => {
                    let f = money.format(f);
                    tr!("includes {f} late fees", "incluye {f} de mora")
                }
                (Some(d), _) => {
                    let when = ds.format_short(d, today);
                    tr!("since {when}", "desde el {when}")
                }
                _ => t("Up to date", "Al día").to_string(),
            }))
        } else {
            Stat::new(t("Interest due", "Interés por pagar"), money.format(owed_now.interest_outstanding)).caption(sel(
                if q.overdue_amount > 0 {
                    let a = money.format(q.overdue_amount);
                    tr!("{a} overdue", "{a} atrasado")
                } else {
                    t("Up to date", "Al día").to_string()
                },
            ))
        }
        .when(q.overdue_amount > 0, |st| st.tone(p.negative));
        let strip = StatStrip::new(match w {
            Width::Compact => 2,
            Width::Medium => 3,
            Width::Wide | Width::Ultra => 5,
        })
        .dense(matches!(w, Width::Compact | Width::Wide))
        .stats(vec![
            Stat::new(owes_label, money.format(owed_now.payoff())).lead(true).caption(sel(if installment_loan {
                t("Capital, interest and late fees due", "Capital, interés y mora por pagar")
            } else {
                t("Capital plus interest due", "Capital más interés por pagar")
            })),
            Stat::new(t("Capital owed", "Capital adeudado"), money.format(owed_now.principal_outstanding))
                .caption(sel(tr!("of {lent} lent", "de {lent} prestado"))),
            third,
            Stat::new(t("Interest paid", "Interés pagado"), money.format(pos.interest_paid))
                .caption(sel(t("Income from this loan", "Ganancia de este préstamo"))),
            Stat::new(t("Capital repaid", "Capital pagado"), money.format(pos.principal_paid))
                .caption(sel(t("Money returned to you", "Dinero que regresó"))),
        ]);

        let ratio = pos.principal_paid as f32 / terms.principal.max(1) as f32;
        let next_line = match (q.next_due, active) {
            (Some(d), true) => {
                let (a, when) = (money.format(q.next_due_amount), ds.format(d));
                tr!("{a} on {when}", "{a} el {when}")
            }
            _ => "—".to_string(),
        };
        let details = Card::new()
            .when(side_by_side, |d| d.flex_grow(1.).flex_basis(px(0.)).min_w(px(340.)).max_w(px(440.)))
            .title(t("Summary", "Resumen"))
            .child(
                v_flex()
                    .px(px(space::XL))
                    .pb(px(space::XL))
                    .gap(px(space::M))
                    .child(
                        v_flex()
                            .gap(px(space::S))
                            .child(progress_bar(&p, ratio, if loan.status == ContractStatus::PaidOff { p.positive } else { p.accent }))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap(px(space::S))
                                    .typeset(text::CAPTION)
                                    .text_color(p.muted)
                                    .font_features(tabular())
                                    .child(sel({
                                        let pct = percent_label(pos.principal_paid, terms.principal).unwrap_or_default();
                                        tr!("{pct} of capital repaid", "{pct} del capital pagado")
                                    }))
                                    .child(div().whitespace_nowrap().child(sel({
                                        let left = money.format(pos.principal_outstanding);
                                        tr!("{left} left", "faltan {left}")
                                    }))),
                            ),
                    )
                    .child(
                        v_flex()
                            .child(kv(&p, t("Amount lent", "Monto prestado"), sel(money.format(terms.principal))))
                            .child(kv(&p, t("Loan type", "Tipo de préstamo"), sel(terms.method.short_label())))
                            .child(kv(&p, t("Interest rate", "Tasa de interés"), sel(rate_line.clone())))
                            .when_some(terms.period_rate().conversion_note(), |d, note| {
                                d.child(kv(&p, t("Per payment", "Por pago"), sel(note)))
                            })
                            .when_some(terms.installments, |d, n| {
                                let every = terms.frequency.label().to_lowercase();
                                d.child(kv(&p, t("Installments", "Cuotas"), sel(format!("{n} · {every}"))))
                                    .when(terms.late_fee_rate.0 > 0, |d| {
                                        d.child(kv(&p, t("Late fee", "Cargo por mora"), sel(terms.late_fee_rate.percent_label())))
                                    })
                                    .when(loan.schedule_edited, |d| {
                                        d.child(kv(&p, t("Schedule", "Calendario"), sel(t("Set by hand", "Fijado a mano"))))
                                    })
                            })
                            .when(!installment_loan, |d| {
                                d.child(kv(&p, t("Interest per period now", "Interés por período ahora"), sel(money.format(interest_now))))
                            })
                            .child(kv(&p, t("Next payment", "Próximo pago"), sel(next_line)))
                            .when(q.overdue_amount > 0, |d| {
                                d.child(kv(
                                    &p,
                                    t("Overdue", "Atrasado"),
                                    div().text_color(p.negative).child(sel({
                                        let (a, days) = (money.format(q.overdue_amount), q.days_overdue);
                                        tr!("{a} · {days} days", "{a} · {days} días")
                                    })),
                                ))
                            })
                            .child(kv(
                                &p,
                                t("Total interest charged so far", "Interés generado hasta hoy"),
                                sel(money.format(pos.interest_charged_total)),
                            ))
                            .child(kv(&p, t("Loan date", "Fecha del préstamo"), sel(ds.format(terms.start_date)))),
                    )
                    .when(!loan.notes.is_empty(), |d| {
                        d.child(
                            v_flex()
                                .gap(px(4.))
                                .pt(px(space::S))
                                .child(div().typeset(text::CAPTION).text_color(p.muted).child(sel(t("Notes", "Notas"))))
                                .child(div().child(sel(loan.notes.clone()))),
                        )
                    }),
            );

        let periods_card = match &plan_state {
            Some(state) => self.installments_card(&p, state, today, w),
            None => self.periods_card(&p, &book, &loan, interest_now, w, side_by_side),
        };
        let fees_card = plan_state.as_ref().and_then(|_| self.late_fees_card(&p, id, cx));
        let tx_card = self.loan_payments_card(&p, id, installment_loan, active, w, cx);

        // The installment table needs the full width; the summary sits next
        // to the late fees. Interest-only loans put the periods beside it.
        let pair = |left: AnyElement, right: Option<AnyElement>| {
            div()
                .flex()
                .gap(gap)
                .when(side_by_side, |d| d.flex_row().items_start())
                .when(!side_by_side, |d| d.flex_col())
                .child(left)
                .children(right.map(|r| v_flex().when(side_by_side, |d| d.flex_grow(2.).flex_basis(px(0.))).min_w_0().child(r)))
                .into_any_element()
        };
        let middle = if installment_loan {
            v_flex().gap(gap).child(periods_card).child(pair(details.into_any_element(), fees_card)).into_any_element()
        } else {
            pair(details.into_any_element(), Some(periods_card))
        };

        v_flex()
            .child(crumb(&p, t("Loans", "Préstamos"), Route::Loans, cx))
            .child(header)
            .child(v_flex().gap(gap).child(strip).child(middle).child(tx_card).children({
                let changes: Vec<_> = self.snap.changes.iter().filter(|c| c.loan_id == Some(id)).collect();
                (!changes.is_empty()).then(|| self.history_card(&p, &changes, 30))
            }))
            .into_any_element()
    }

    /// Client link, amount, rate and start date under the loan's title.
    fn loan_subtitle(&self, p: &Palette, loan: &Loan, cx: &mut Context<Self>) -> Div {
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let client_id = loan.client_id;
        let rate_line = loan.terms.rate_line();
        let since = ds.format(loan.terms.start_date);
        let lent = money.format(loan.terms.principal);
        h_flex()
            .flex_wrap()
            .gap(px(6.))
            .font_features(tabular())
            .child(
                div()
                    .id("loan-client")
                    .text_color(p.accent_text)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(self.snap.client_name(client_id).to_string())
                    .on_click(cx.listener(move |this, _, w, cx| this.navigate(Route::Client(client_id), w, cx))),
            )
            .child(sel(tr!("·  {lent} at {rate_line}  ·  since {since}", "·  {lent} al {rate_line}  ·  desde {since}")))
    }

    /// Page actions. Wide windows show them all; narrower ones fold the
    /// secondary ones into a "More" menu so the main action never wraps.
    fn loan_actions(&self, id: i64, status: ContractStatus, w: Width, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let receipt = cx.listener(move |this, _, w, cx| this.open_loan_receipt(id, w, cx));
        let edit = cx.listener(move |this, _, w, cx| this.edit_loan(id, w, cx));
        let delete = cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::DeleteLoan(id), w, cx));
        let default = cx.listener(move |this, _, w, cx| this.open_reason_form(ReasonKind::MarkDefaulted(id), w, cx));
        let main = match status {
            ContractStatus::Active => Some(
                Button::new("loan-pay")
                    .primary()
                    .icon(IconName::Plus)
                    .label(t("Record payment", "Registrar pago"))
                    .on_click(cx.listener(move |this, _, w, cx| this.open_payment_form(Some(id), w, cx)))
                    .into_any_element(),
            ),
            ContractStatus::Defaulted => Some(
                Button::new("loan-reinstate")
                    .label(t("Reinstate loan", "Reactivar préstamo"))
                    .on_click(cx.listener(move |this, _, w, cx| this.reinstate_loan(id, w, cx)))
                    .into_any_element(),
            ),
            _ => None,
        };
        let active = status == ContractStatus::Active;
        let mut out = if w >= Width::Wide {
            let mut v = vec![
                Button::new("loan-receipt")
                    .ghost()
                    .icon(IconName::Printer)
                    .label(t("Loan receipt", "Recibo del préstamo"))
                    .on_click(receipt)
                    .into_any_element(),
                Button::new("loan-edit").ghost().icon(IconName::Pencil).label(t("Edit", "Editar")).on_click(edit).into_any_element(),
                Button::new("loan-delete")
                    .ghost()
                    .icon(IconName::Trash)
                    .tooltip(t("Delete loan", "Eliminar préstamo"))
                    .on_click(delete)
                    .into_any_element(),
            ];
            if active {
                v.push(
                    Button::new("loan-default")
                        .ghost()
                        .label(t("Mark defaulted", "Marcar incobrable"))
                        .on_click(default)
                        .into_any_element(),
                );
            }
            v
        } else {
            let mut more = Dropdown::new("loan-more")
                .icon(IconName::Ellipsis)
                .tooltip(t("More actions", "Más acciones"))
                .align_end()
                .item(DropdownItem::new(t("Loan receipt", "Recibo del préstamo"), receipt).icon(IconName::Printer))
                .item(DropdownItem::new(t("Edit", "Editar"), edit).icon(IconName::Pencil));
            if active {
                more = more.item(DropdownItem::new(t("Mark defaulted", "Marcar incobrable"), default).icon(IconName::CircleAlert));
            }
            vec![
                more.item(DropdownItem::new(t("Delete loan", "Eliminar préstamo"), delete).icon(IconName::Trash).danger())
                    .into_any_element(),
            ]
        };
        out.extend(main);
        out
    }

    /// Period by period for an interest-only loan: the capital it was
    /// charged on and whether it is paid. The capital column goes on Compact.
    fn periods_card(&self, p: &Palette, book: &LoanBook, loan: &Loan, interest_now: Cents, w: Width, beside: bool) -> AnyElement {
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let today = self.today;
        let terms = &loan.terms;
        let pos = book.position(today);
        let compact = w == Width::Compact;
        let row = |seq: String, due: String| {
            h_flex()
                .h(px(42.))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .child(div().w(px(28.)).flex_none().text_color(p.faint).child(sel(seq)))
                .child(div().flex_1().min_w(px(90.)).truncate().child(sel(due)))
        };
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
                row(period.seq.to_string(), ds.format(period.due))
                    .when(!compact, |d| d.child(ncolumn(130.).text_color(p.muted).child(sel(money.format(period.balance)))))
                    .child(ncolumn(110.).child(sel(money.format(period.interest))))
                    .child(ncolumn(110.).text_color(p.muted).child(sel(money.format(*paid))))
                    .child(div().w(px(110.)).flex_none().flex().justify_end().child(pill(p, text, tone)))
                    .into_any_element()
            })
            .collect();
        if loan.status == ContractStatus::Active && pos.principal_outstanding > 0 {
            // The next period, projected on today's capital.
            let next_seq = pos.periods.last().map(|x| x.seq + 1).unwrap_or(1);
            let due = terms.frequency.due_date(terms.start_date, next_seq);
            period_rows.insert(
                0,
                row(next_seq.to_string(), ds.format(due))
                    .text_color(p.muted)
                    .when(!compact, |d| d.child(ncolumn(130.).child(sel(money.format(pos.principal_outstanding)))))
                    .child(ncolumn(110.).child(sel(money.format(interest_now))))
                    .child(ncolumn(110.).child("—"))
                    .child(div().w(px(110.)).flex_none().flex().justify_end().child(pill(p, t("Upcoming", "Próximo"), Tone::Neutral)))
                    .into_any_element(),
            );
        }
        let note = t(
            "Each period's interest is charged on the capital owed when it starts",
            "El interés de cada período se calcula sobre el capital adeudado al iniciar",
        );
        // Beside the summary or on Compact the note no longer fits next to
        // the title, so it moves under it.
        let note_in_header = !compact && !beside;
        Card::new()
            .overflow_hidden()
            .title(t("Interest by period", "Interés por período"))
            .when(note_in_header, |c| c.action(header_note(p, note)))
            .when(!note_in_header, |c| c.child(header_note(p, note).px(px(space::XL)).pb(px(space::M))))
            .child(
                table_header(p)
                    .child(div().w(px(28.)).flex_none().child(th(p, "#")))
                    .child(div().flex_1().min_w(px(90.)).child(th(p, t("Due date", "Vence"))))
                    .when(!compact, |d| d.child(ncolumn(130.).child(th(p, t("Capital", "Capital")))))
                    .child(ncolumn(110.).child(th(p, t("Interest", "Interés"))))
                    .child(ncolumn(110.).child(th(p, t("Paid", "Pagado"))))
                    .child(ncolumn(110.).child(th(p, t("Status", "Estado")))),
            )
            .child(div().id("periods-scroll").max_h(px(440.)).overflow_y_scroll().children(period_rows))
            .into_any_element()
    }

    /// Transactions on the loan, newest first. Below Wide the note moves
    /// under the date and the receipt button loses its label; on Compact the
    /// interest/capital split goes too, leaving date, amount and actions.
    fn loan_payments_card(
        &self,
        p: &Palette,
        id: i64,
        installment_loan: bool,
        active: bool,
        w: Width,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let money = self.settings().money();
        let ds = self.settings().date_style;
        let full = w >= Width::Wide;
        let split = w > Width::Compact;
        let actions_w = if full { 190. } else { 140. };
        let payments: Vec<_> = self.snap.payments_for_loan(id).cloned().collect();
        let tx_rows = payments.iter().rev().map(|x| {
            let reversed = x.is_reversed();
            let pid = x.id;
            let note = if reversed {
                let why = x.reversal_reason.clone().unwrap_or_default();
                tr!("Reversed — {why}", "Anulado — {why}")
            } else if installment_loan {
                let to: Vec<String> = x.applied.iter().map(|a| format!("#{}", a.seq)).collect();
                let to = to.join(", ");
                let mut line = if to.is_empty() { String::new() } else { tr!("To installment {to}", "A la cuota {to}") };
                if x.fee > 0 {
                    let f = money.format(x.fee);
                    line = tr!("{line} · late fee {f}", "{line} · mora {f}");
                }
                if !x.notes.is_empty() {
                    line = format!("{line} · {}", x.notes);
                }
                line
            } else {
                x.notes.clone()
            };
            let receipt_no = tr!("Receipt #{pid}", "Recibo #{pid}");
            let sub = if full || note.is_empty() { receipt_no } else { format!("{receipt_no} · {note}") };
            let struck = |d: Div| d.when(reversed, |d| d.line_through());
            let receipt = Button::new(("receipt", pid as u64))
                .ghost()
                .small()
                .icon(IconName::Printer)
                .on_click(cx.listener(move |this, _, w, cx| this.open_receipt(pid, w, cx)));
            h_flex()
                .min_h(px(52.))
                .py(px(space::S))
                .px(px(space::XL))
                .gap(px(space::L))
                .border_b_1()
                .border_color(p.rule)
                .font_features(tabular())
                .child(
                    v_flex()
                        .when(full, |d| d.w(px(150.)).flex_none())
                        .when(!full, |d| d.flex_1().min_w(px(120.)))
                        .child(div().text_color(if reversed { p.muted } else { p.text }).child(sel(ds.format(x.paid_on))))
                        .child(div().typeset(text::CAPTION).text_color(p.faint).truncate().child(sel(sub))),
                )
                .when(full, |d| d.child(div().flex_1().min_w_0().typeset(text::CAPTION).text_color(p.muted).truncate().child(sel(note))))
                .when(split, |d| {
                    d.child(struck(ncolumn(110.).text_color(p.muted)).child(sel(money.format(x.interest))))
                        .child(struck(ncolumn(110.).text_color(p.muted)).child(sel(money.format(x.principal))))
                })
                .child(struck(ncolumn(120.).text_color(if reversed { p.muted } else { p.text })).child(sel(money.format(x.amount))))
                .child(
                    h_flex()
                        .w(px(actions_w))
                        .flex_none()
                        .justify_end()
                        .gap(px(2.))
                        .when(reversed, |d| d.child(pill(p, t("Reversed", "Anulado"), Tone::Neutral)))
                        .child(if full { receipt.label(t("Receipt", "Recibo")) } else { receipt.tooltip(t("Receipt", "Recibo")) })
                        .when(!reversed, |d| {
                            d.child(row_icon_button(
                                ("pay-edit", pid as u64),
                                IconName::Pencil,
                                t("Edit payment", "Editar pago"),
                                cx,
                                move |this, w, cx| this.edit_payment(pid, w, cx),
                            ))
                        })
                        .child(row_icon_button(
                            ("pay-del", pid as u64),
                            IconName::Trash,
                            t("Delete payment", "Eliminar pago"),
                            cx,
                            move |this, w, cx| this.open_reason_form(ReasonKind::DeletePayment(pid), w, cx),
                        )),
                )
        });
        let body = if payments.is_empty() {
            empty_state(
                p,
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
                    table_header(p)
                        .when(full, |d| d.child(div().w(px(150.)).flex_none().child(th(p, t("Date", "Fecha")))))
                        .when(!full, |d| d.child(div().flex_1().min_w(px(120.)).child(th(p, t("Date", "Fecha")))))
                        .when(full, |d| d.child(div().flex_1().child(th(p, t("Note", "Nota")))))
                        .when(split, |d| {
                            d.child(ncolumn(110.).child(th(p, t("Interest", "Interés"))))
                                .child(ncolumn(110.).child(th(p, t("Capital", "Capital"))))
                        })
                        .child(ncolumn(120.).child(th(p, t("Amount", "Monto"))))
                        .child(div().w(px(actions_w)).flex_none()),
                )
                .children(tx_rows)
                .into_any_element()
        };
        Card::new().overflow_hidden().title(t("Payments", "Pagos")).child(body).into_any_element()
    }
}
