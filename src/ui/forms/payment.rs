use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::loan::Choice;
use super::{date_field, error_banner, field, footer, invalid_date, picked_date, value};
use crate::app::VzApp;
use crate::domain::ledger::{Allocation, ContractStatus, PaymentError, owed_at_payment};
use crate::domain::loans::period_interest;
use crate::domain::money::{Cents, MoneyFormat, parse_money};
use crate::i18n::t;
use crate::services::{Snapshot, preview_allocation};
use crate::tr;
use crate::ui::components::{Tone, kv, pill};
use crate::ui::theme::{palette, size, space};

pub struct PaymentForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    loan: Entity<SelectState<Vec<Choice>>>,
    loan_id: Option<i64>,
    amount: Entity<InputState>,
    fee: Entity<InputState>,
    date: Entity<DateField>,
    notes: Entity<InputState>,
    show_fee: bool,
    print_receipt: bool,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl PaymentForm {
    pub fn new(app: WeakEntity<VzApp>, snap: Rc<Snapshot>, loan_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = crate::utils::dates::today();
        let money = snap.settings.money();
        let mut open: Vec<_> = snap.book.loans.iter().filter(|l| l.facts.status == ContractStatus::Active).collect();
        open.sort_by(|a, b| snap.client_name(a.client_id).cmp(snap.client_name(b.client_id)).then(a.id.cmp(&b.id)));
        let choices: Vec<Choice> = open
            .iter()
            .map(|l| {
                let name = snap.client_name(l.client_id);
                let id = l.id;
                let owed = money.format(owed_at_payment(&l.facts, &l.payments, today).payoff());
                Choice { id, title: tr!("{name} — Loan #{id} · {owed} owed", "{name} — Préstamo #{id} · debe {owed}").into() }
            })
            .collect();
        let loan_id = loan_id.or_else(|| (choices.len() == 1).then(|| choices[0].id));
        let selected = loan_id.and_then(|id| choices.iter().position(|c| c.id == id)).map(IndexPath::new);
        let loan = cx.new(|cx| SelectState::new(choices, selected, window, cx).searchable(true));
        let amount = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let fee = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let notes = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("Optional: cash, transfer, reference", "Opcional: efectivo, transferencia, referencia"))
        });
        let date = date_field(today, window, cx);
        let mut subs = vec![
            cx.subscribe_in(&loan, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.loan_id = *v;
                this.error = None;
                cx.notify();
            }),
            cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify()),
        ];
        for input in [&amount, &fee, &notes] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        let print_receipt = snap.settings.receipt_after_payment;
        Self {
            app,
            snap,
            loan,
            loan_id,
            amount,
            fee,
            date,
            notes,
            show_fee: false,
            print_receipt,
            error: None,
            show_errors: false,
            _subs: subs,
        }
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
        Some(preview_allocation(loan, amount, fee, date, crate::utils::dates::today()))
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
        let print = self.print_receipt;
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            match app.store.record_payment(loan_id, amount, fee, date, &notes, today) {
                Ok(id) => {
                    app.reload(cx);
                    let closed = app.snap.loan(loan_id).is_some_and(|l| l.status == ContractStatus::PaidOff);
                    if app.settings().receipt_after_payment != print {
                        // Remember the choice for next time.
                        let s = app.settings().clone();
                        let _ = app.store.save_settings(&crate::services::settings::Settings { receipt_after_payment: print, ..s });
                        app.reload(cx);
                    }
                    Ok((id, closed))
                }
                Err(e) => Err(app.error_message(&e)),
            }
        });
        match result {
            Ok(Ok((payment_id, closed))) => {
                window.close_dialog(cx);
                let _ = self.app.update(cx, |app, cx| {
                    let paid = app.settings().money().format(amount);
                    let msg = if closed {
                        tr!("Payment of {paid} recorded. The loan is paid off.", "Pago de {paid} registrado. El préstamo quedó saldado.")
                    } else {
                        tr!("Payment of {paid} recorded", "Pago de {paid} registrado")
                    };
                    app.notify_success(msg, window, cx);
                    if print {
                        app.open_receipt(payment_id, window, cx);
                    }
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
        let picked = picked_date(&self.date, cx);

        // What the client owes on the chosen date, for the quick-fill buttons.
        let owed = book.map(|l| owed_at_payment(&l.facts, &l.payments, picked.unwrap_or(today)));

        let amount_err = match (&amount, &preview) {
            (Err(e), _) if self.show_errors => Some(e.clone()),
            (_, Some(Err(PaymentError::Overpayment { payoff }))) => {
                let payoff = money.format(*payoff);
                Some(tr!("The client owes {payoff} in total on this date", "El cliente debe {payoff} en total en esta fecha"))
            }
            (_, Some(Err(e @ (PaymentError::NotPositive | PaymentError::FeeTooLarge)))) => Some(e.message().to_string()),
            _ => None,
        };
        let date_err = match (&preview, picked) {
            (_, None) => Some(invalid_date()),
            (Some(Err(e @ (PaymentError::BeforeStart | PaymentError::InFuture))), _) => Some(e.message().to_string()),
            (Some(Err(PaymentError::BeforeLastPayment { last })), _) => {
                let last = self.snap.settings.date_style.format(*last);
                Some(tr!(
                    "A later payment is recorded ({last}). Payments are recorded in date order.",
                    "Hay un pago posterior registrado ({last}). Los pagos se registran en orden de fecha."
                ))
            }
            _ => None,
        };
        let loan_err = (self.show_errors && self.loan_id.is_none()).then(|| {
            if self.snap.book.loans.iter().any(|l| l.facts.status == ContractStatus::Active) {
                t("Choose a loan", "Elige un préstamo").to_string()
            } else {
                t("There are no active loans", "No hay préstamos activos").to_string()
            }
        });

        let summary = owed.as_ref().map(|o| {
            let cap = money.format(o.principal_outstanding);
            let int = money.format(o.interest_outstanding);
            h_flex()
                .gap(px(space::L))
                .text_size(px(size::META + 0.5))
                .text_color(p.muted)
                .child(tr!("Capital owed: {cap}", "Capital adeudado: {cap}"))
                .child(tr!("Interest due: {int}", "Interés por pagar: {int}"))
        });

        let allocation = match (&preview, book) {
            (Some(Ok(a)), Some(l)) => {
                let after = a.principal_after + a.interest_after;
                let next_interest = period_interest(a.principal_after, l.facts.rate);
                v_flex()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .justify_between()
                            .pb(px(6.))
                            .child(
                                div().font_weight(FontWeight::MEDIUM).child(t("How this payment is applied", "Cómo se aplica este pago")),
                            )
                            .when(a.pays_off(), |d| d.child(pill(&p, t("Pays off the loan", "Salda el préstamo"), Tone::Positive))),
                    )
                    .when(a.fee > 0, |d| d.child(kv(&p, t("Fee", "Cargo"), money.format(a.fee))))
                    .child(kv(&p, t("Interest", "Interés"), money.format(a.interest)))
                    .child(kv(&p, t("Capital", "Abono a capital"), money.format(a.principal)))
                    .child(div().h(px(1.)).my(px(6.)).bg(p.border))
                    .child(kv(&p, t("Capital remaining", "Capital pendiente"), money.format(a.principal_after)))
                    .when(a.interest_after > 0, |d| {
                        d.child(kv(&p, t("Interest still due", "Interés aún pendiente"), money.format(a.interest_after)))
                    })
                    .when(after > 0, |d| {
                        d.child(kv(&p, t("Next period's interest", "Interés del próximo período"), money.format(next_interest)))
                    })
                    .into_any_element()
            }
            _ => div()
                .text_color(p.muted)
                .child(match (book, &amount) {
                    (None, _) => {
                        t("Choose a loan to see how the payment is applied.", "Elige un préstamo para ver cómo se aplica el pago.")
                    }
                    (Some(_), Err(_)) => t(
                        "Enter the amount received to see how much goes to interest and to capital.",
                        "Escribe el monto recibido para ver cuánto va a interés y cuánto a capital.",
                    ),
                    _ => t("Adjust the amount or date to see the split.", "Ajusta el monto o la fecha para ver la distribución."),
                })
                .into_any_element(),
        };

        let quick_el = owed.map(|o| {
            let interest = o.interest_outstanding;
            let payoff = o.payoff();
            h_flex()
                .gap(px(space::S))
                .flex_wrap()
                .when(interest > 0, |d| {
                    let label = money.format(interest);
                    d.child(
                        Button::new("fill-interest")
                            .small()
                            .label(tr!("Interest {label}", "Interés {label}"))
                            .on_click(cx.listener(move |this, _, w, cx| this.set_amount(interest, w, cx))),
                    )
                })
                .when(payoff > 0, |d| {
                    let label = money.format(payoff);
                    d.child(
                        Button::new("fill-payoff")
                            .small()
                            .label(tr!("Pay off {label}", "Saldar {label}"))
                            .on_click(cx.listener(move |this, _, w, cx| this.set_amount(payoff, w, cx))),
                    )
                })
        });

        let can_submit = matches!(preview, Some(Ok(_))) && fee.is_ok();
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(
                field(
                    &p,
                    t("Loan", "Préstamo"),
                    Select::new(&self.loan)
                        .placeholder(t("Choose a client's loan", "Elige el préstamo de un cliente"))
                        .search_placeholder(t("Search by client", "Buscar por cliente")),
                    None,
                    loan_err,
                )
                .children(summary),
            )
            .child(
                h_flex()
                    .items_start()
                    .gap(px(space::M))
                    .child(
                        div().flex_1().child(
                            field(
                                &p,
                                t("Amount received", "Monto recibido"),
                                Input::new(&self.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                                None,
                                amount_err,
                            )
                            .children(quick_el),
                        ),
                    )
                    .child(div().w(px(200.)).child(field(&p, t("Date received", "Fecha del pago"), self.date.clone(), None, date_err))),
            )
            .child(if self.show_fee {
                field(
                    &p,
                    t("Fee included in this payment", "Cargo incluido en este pago"),
                    Input::new(&self.fee).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                    Some(
                        t("Late or service fee, applied before interest", "Mora o cargo por servicio, se aplica antes del interés").into(),
                    ),
                    if let Err(e) = &fee { Some(e.clone()) } else { None },
                )
                .into_any_element()
            } else {
                div()
                    .id("add-fee")
                    .text_size(px(size::META + 0.5))
                    .text_color(p.accent_text)
                    .cursor_pointer()
                    .child(t("+ Include a fee", "+ Incluir un cargo"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_fee = true;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .child(field(&p, t("Note", "Nota"), Input::new(&self.notes), None, None))
            .child(div().p(px(space::L)).rounded(px(10.)).bg(p.raised).border_1().border_color(p.rule).child(allocation))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(Switch::new("print-receipt").checked(self.print_receipt).on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.print_receipt = *checked;
                        cx.notify();
                    })))
                    .child(div().child(t("Open the receipt to print after saving", "Abrir el recibo para imprimir al guardar"))),
            )
            .child(footer(
                &p,
                t("Record payment", "Registrar pago"),
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
