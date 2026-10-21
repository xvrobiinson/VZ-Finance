use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::button::Button;
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::loan::Choice;
use super::{date_picker, error_banner, field, footer, picked_date, value};
use crate::app::VzApp;
use crate::domain::ledger::{Allocation, PaymentError, allocate};
use crate::domain::money::{Cents, MoneyFormat, parse_money};
use crate::services::{Snapshot, payment_error_message};
use crate::ui::components::{Tone, kv, pill};
use crate::ui::theme::{palette, size, space};

pub struct PaymentForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    loan: Entity<SelectState<Vec<Choice>>>,
    loan_id: Option<i64>,
    amount: Entity<InputState>,
    fee: Entity<InputState>,
    date: Entity<DatePickerState>,
    notes: Entity<InputState>,
    show_fee: bool,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl PaymentForm {
    pub fn new(app: WeakEntity<VzApp>, snap: Rc<Snapshot>, loan_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = crate::utils::dates::today();
        let money = snap.settings.money();
        let mut open: Vec<_> = snap.book.loans.iter().filter(|l| l.facts.status == crate::domain::ledger::ContractStatus::Active).collect();
        open.sort_by(|a, b| snap.client_name(a.client_id).cmp(snap.client_name(b.client_id)).then(a.id.cmp(&b.id)));
        let choices: Vec<Choice> = open
            .iter()
            .map(|l| Choice {
                id: l.id,
                title: format!("{} — Loan #{} · {} owed", snap.client_name(l.client_id), l.id, money.format(l.position(today).payoff())).into(),
            })
            .collect();
        let loan_id = loan_id.or_else(|| (choices.len() == 1).then(|| choices[0].id));
        let selected = loan_id.and_then(|id| choices.iter().position(|c| c.id == id)).map(IndexPath::new);
        let loan = cx.new(|cx| SelectState::new(choices, selected, window, cx).searchable(true));
        let amount = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let fee = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let notes = cx.new(|cx| InputState::new(window, cx).placeholder("Optional: method, reference, remarks"));
        let date = date_picker(today, window, cx);
        let mut subs = vec![
            cx.subscribe_in(&loan, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.loan_id = *v;
                this.error = None;
                cx.notify();
            }),
            cx.subscribe_in(&date, window, |_, _, _: &DatePickerEvent, _, cx| cx.notify()),
        ];
        for input in [&amount, &fee, &notes] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self { app, snap, loan, loan_id, amount, fee, date, notes, show_fee: false, error: None, show_errors: false, _subs: subs }
    }

    pub fn first_focus(&self, cx: &App) -> FocusHandle {
        if self.loan_id.is_some() { self.amount.focus_handle(cx) } else { self.loan.focus_handle(cx) }
    }

    fn amounts(&self, cx: &App) -> (Result<Cents, String>, Result<Cents, String>) {
        let amount = parse_money(&value(&self.amount, cx)).map_err(|e| e.to_string());
        let fee_text = value(&self.fee, cx);
        let fee = if fee_text.trim().is_empty() { Ok(0) } else { parse_money(&fee_text).map_err(|e| e.to_string()) };
        (amount, fee)
    }

    fn preview(&self, cx: &App) -> Option<Result<Allocation, PaymentError>> {
        let loan = self.snap.book_loan(self.loan_id?)?;
        let (Ok(amount), Ok(fee)) = self.amounts(cx) else { return None };
        let date = picked_date(&self.date, cx)?;
        Some(allocate(&loan.facts, &loan.payments, amount, fee, date, crate::utils::dates::today()))
    }

    fn set_amount(&mut self, cents: Cents, window: &mut Window, cx: &mut Context<Self>) {
        self.amount.update(cx, |s, cx| s.set_value(MoneyFormat::input_value(cents), window, cx));
        cx.notify();
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let (Some(loan_id), (Ok(amount), Ok(fee)), Some(date)) = (self.loan_id, self.amounts(cx), picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        let notes = value(&self.notes, cx);
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            match app.store.record_payment(loan_id, amount, fee, date, &notes, today) {
                Ok(_) => {
                    app.reload(cx);
                    let closed = app.snap.loan(loan_id).is_some_and(|l| l.status == crate::domain::ledger::ContractStatus::PaidOff);
                    Ok(closed)
                }
                Err(e) => Err(app.error_message(&e)),
            }
        });
        match result {
            Ok(Ok(closed)) => {
                window.close_dialog(cx);
                let _ = self.app.update(cx, |app, cx| {
                    let money = app.settings().money();
                    let msg = if closed {
                        format!("Payment of {} recorded. The loan is paid off.", money.format(amount))
                    } else {
                        format!("Payment of {} recorded", money.format(amount))
                    };
                    app.notify_success(msg, window, cx);
                });
            }
            Ok(Err(msg)) => {
                self.error = Some(msg);
                cx.notify();
            }
            Err(_) => window.close_dialog(cx),
        }
    }
}

impl Render for PaymentForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let money = self.snap.settings.money();
        let today = crate::utils::dates::today();
        let book = self.loan_id.and_then(|id| self.snap.book_loan(id));
        let (amount, fee) = self.amounts(cx);
        let preview = self.preview(cx);

        // Quick fills: the next amount due and the payoff on the chosen date.
        let date = picked_date(&self.date, cx).unwrap_or(today);
        let quick = book.map(|l| {
            let q = l.delinquency(today);
            let payoff = l.position(date).payoff();
            let due = if q.overdue_amount > 0 { q.overdue_amount } else { q.next_due_amount };
            (due.min(payoff), payoff, q.overdue_amount > 0)
        });

        let amount_err = match (&amount, &preview) {
            (Err(e), _) if self.show_errors => Some(e.clone()),
            (_, Some(Err(PaymentError::Overpayment { payoff }))) => Some(format!("The payoff on this date is {}", money.format(*payoff))),
            (_, Some(Err(e @ (PaymentError::NotPositive | PaymentError::FeeTooLarge)))) => Some(payment_error_message(e)),
            _ => None,
        };
        let date_err = match &preview {
            Some(Err(e @ (PaymentError::BeforeStart | PaymentError::InFuture))) => Some(payment_error_message(e)),
            Some(Err(PaymentError::BeforeLastPayment { last })) => {
                Some(format!("A later payment exists ({}). Payments are recorded in date order.", self.snap.settings.date_style.format(*last)))
            }
            _ => None,
        };
        let loan_err = (self.show_errors && self.loan_id.is_none()).then(|| {
            if self.snap.book.loans.iter().any(|l| l.facts.status == crate::domain::ledger::ContractStatus::Active) {
                "Choose a loan".to_string()
            } else {
                "There are no active loans".to_string()
            }
        });

        let allocation = match &preview {
            Some(Ok(a)) => {
                let after = a.principal_after + a.interest_after;
                v_flex()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .justify_between()
                            .pb(px(6.))
                            .child(div().font_weight(FontWeight::MEDIUM).child("Allocation"))
                            .when(a.pays_off(), |d| d.child(pill(&p, "Pays off the loan", Tone::Positive))),
                    )
                    .child(kv(&p, "Interest (income)", money.format(a.interest)))
                    .child(kv(&p, "Principal (capital returned)", money.format(a.principal)))
                    .when(a.fee > 0, |d| d.child(kv(&p, "Fee (income)", money.format(a.fee))))
                    .child(div().h(px(1.)).my(px(6.)).bg(p.border))
                    .child(kv(&p, "Still owed after this payment", money.format(after)))
                    .child(
                        div()
                            .pt(px(6.))
                            .text_size(px(size::META))
                            .text_color(p.faint)
                            .child("Applied to fees first, then accrued interest, then principal."),
                    )
                    .into_any_element()
            }
            _ => div()
                .text_color(p.muted)
                .child(match (book, &amount) {
                    (None, _) => "Choose a loan to see how the payment will be split.",
                    (Some(_), Err(_)) => "Enter the amount received to see the split between interest and principal.",
                    _ => "Adjust the amount or date to see the split.",
                })
                .into_any_element(),
        };

        let quick_el = quick.map(|(due, payoff, overdue)| {
            h_flex()
                .gap(px(space::S))
                .flex_wrap()
                .when(due > 0, |d| {
                    d.child(
                        Button::new("fill-due")
                            .small()
                            .label(format!("{} {}", if overdue { "Overdue" } else { "Next due" }, money.format(due)))
                            .on_click(cx.listener(move |this, _, w, cx| this.set_amount(due, w, cx))),
                    )
                })
                .child(
                    Button::new("fill-payoff")
                        .small()
                        .label(format!("Pay off {}", money.format(payoff)))
                        .on_click(cx.listener(move |this, _, w, cx| this.set_amount(payoff, w, cx))),
                )
        });

        let can_submit = matches!(preview, Some(Ok(_))) && fee.is_ok();
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(field(&p, "Loan", Select::new(&self.loan).placeholder("Choose a client's loan").search_placeholder("Search by client"), None, loan_err))
            .child(
                h_flex()
                    .items_start()
                    .gap(px(space::M))
                    .child(
                        div().flex_1().child(
                            field(&p, "Amount received", Input::new(&self.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())), None, amount_err)
                                .children(quick_el),
                        ),
                    )
                    .child(div().w(px(200.)).child(field(&p, "Date received", DatePicker::new(&self.date), None, date_err))),
            )
            .child(if self.show_fee {
                field(
                    &p,
                    "Fee included in this payment",
                    Input::new(&self.fee).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                    Some("Late or service fee, counted as income before interest".into()),
                    if let Err(e) = &fee { Some(e.clone()) } else { None },
                )
                .into_any_element()
            } else {
                div()
                    .id("add-fee")
                    .text_size(px(size::META + 0.5))
                    .text_color(p.accent_text)
                    .cursor_pointer()
                    .child("+ Include a fee")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_fee = true;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .child(field(&p, "Note", Input::new(&self.notes), None, None))
            .child(div().p(px(space::L)).rounded(px(10.)).bg(p.raised).border_1().border_color(p.rule).child(allocation))
            .child(footer(&p, "Record payment", can_submit, false, cx.listener(|this, _, window, cx| this.submit(window, cx))))
    }
}
