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
use super::{choice_tile, date_field, error_banner, field, footer, invalid_date, picked_date, reason_input, value};
use crate::app::VzApp;
use crate::database::repositories::payments::AllocationMode;
use crate::domain::installments::{Applied, Instruction, PlanState};
use crate::domain::ledger::{Allocation, ContractStatus, PaymentError, Position, owed_at_payment};
use crate::domain::money::{Cents, MoneyFormat, parse_money};
use crate::i18n::t;
use crate::services::{Snapshot, installment_state_before, loan_facts, payments_before, preview_installment_payment, preview_payment};
use crate::tr;
use crate::ui::components::{Tone, kv, pill, th};
use crate::ui::theme::{Palette, palette, size, space, tabular};

/// The three ways to apply a payment on an installment loan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PayOption {
    /// Option 1: to one installment, which may be left partly paid.
    Installment,
    /// Option 2: across installments, oldest first or with chosen amounts.
    Spread,
    /// Option 3: chosen amounts to capital, interest and late fees.
    Custom,
}

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
    /// The payment being edited, if any. Its loan can't change.
    editing: Option<i64>,
    reason: Entity<InputState>,
    // Installment loans.
    option: PayOption,
    target: Entity<SelectState<Vec<Choice>>>,
    target_seq: Option<u32>,
    /// Option 2 with amounts chosen per installment.
    manual: bool,
    spread: Vec<(u32, Entity<InputState>)>,
    custom_principal: Entity<InputState>,
    custom_interest: Entity<InputState>,
    custom_fee: Entity<InputState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
    _per_loan: Vec<Subscription>,
}

fn notify_on_change<T: 'static>(input: &Entity<InputState>, window: &mut Window, cx: &mut Context<T>) -> Subscription {
    cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
        if let InputEvent::Change = ev {
            cx.notify()
        }
    })
}

impl PaymentForm {
    pub fn new(
        app: WeakEntity<VzApp>,
        snap: Rc<Snapshot>,
        loan_id: Option<i64>,
        editing: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let today = crate::utils::dates::today();
        let existing = editing.and_then(|id| snap.payment(id).cloned());
        let loan_id = existing.as_ref().map(|x| x.loan_id).or(loan_id);
        let money = snap.settings.money();
        let mut open: Vec<_> = snap.book.loans.iter().filter(|l| l.facts.status == ContractStatus::Active).collect();
        open.sort_by(|a, b| snap.client_name(a.client_id).cmp(snap.client_name(b.client_id)).then(a.id.cmp(&b.id)));
        let choices: Vec<Choice> = open
            .iter()
            .map(|l| {
                let name = snap.client_name(l.client_id);
                let id = l.id;
                let owed = match l.plan_state(today) {
                    Some(s) => s.payoff(),
                    None => owed_at_payment(&l.facts, &l.payments, today).payoff(),
                };
                let owed = money.format(owed);
                Choice { id, title: tr!("{name} — Loan #{id} · {owed} owed", "{name} — Préstamo #{id} · debe {owed}").into() }
            })
            .collect();
        let loan_id = loan_id.or_else(|| (choices.len() == 1).then(|| choices[0].id));
        let selected = loan_id.and_then(|id| choices.iter().position(|c| c.id == id)).map(IndexPath::new);
        let loan = cx.new(|cx| SelectState::new(choices, selected, window, cx).searchable(true));
        let prefill = |s: InputState, v: Option<String>| match v {
            Some(v) => s.default_value(v),
            None => s,
        };
        let amount = cx.new(|cx| {
            prefill(InputState::new(window, cx).placeholder("0.00"), existing.as_ref().map(|x| MoneyFormat::input_value(x.amount)))
        });
        let fee = cx.new(|cx| {
            prefill(
                InputState::new(window, cx).placeholder("0.00"),
                existing.as_ref().filter(|x| x.fee > 0).map(|x| MoneyFormat::input_value(x.fee)),
            )
        });
        let notes = cx.new(|cx| {
            prefill(
                InputState::new(window, cx)
                    .placeholder(t("Optional: cash, transfer, reference", "Opcional: efectivo, transferencia, referencia")),
                existing.as_ref().map(|x| x.notes.clone()),
            )
        });
        let date = date_field(existing.as_ref().map_or(today, |x| x.paid_on), window, cx);
        let reason = reason_input(window, cx);
        let custom = existing.as_ref().filter(|x| x.mode == AllocationMode::Custom);
        let money_input = |v: Option<Cents>, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| prefill(InputState::new(window, cx).placeholder("0.00"), v.map(MoneyFormat::input_value)))
        };
        let custom_principal = money_input(custom.map(|x| x.principal), window, cx);
        let custom_interest = money_input(custom.map(|x| x.interest), window, cx);
        let custom_fee = money_input(custom.map(|x| x.fee), window, cx);
        let target = cx.new(|cx| SelectState::new(Vec::<Choice>::new(), None, window, cx));
        let option = match existing.as_ref().map(|x| x.mode) {
            Some(AllocationMode::Auto) | Some(AllocationMode::Spread) => PayOption::Spread,
            Some(AllocationMode::Custom) => PayOption::Custom,
            _ => PayOption::Installment,
        };
        let mut subs = vec![
            cx.subscribe_in(&loan, window, |this, _, ev: &SelectEvent<Vec<Choice>>, window, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.loan_id = *v;
                this.error = None;
                this.target_seq = None;
                this.load_installments(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&target, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.target_seq = v.map(|s| s as u32);
                cx.notify();
            }),
            cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify()),
        ];
        for input in [&amount, &fee, &notes, &custom_principal, &custom_interest, &custom_fee] {
            subs.push(notify_on_change(input, window, cx));
        }
        let print_receipt = snap.settings.receipt_after_payment;
        let mut form = Self {
            app,
            snap,
            loan,
            loan_id,
            amount,
            fee,
            date,
            notes,
            show_fee: existing.as_ref().is_some_and(|x| x.fee > 0),
            print_receipt,
            editing,
            reason,
            option,
            target,
            target_seq: existing.as_ref().and_then(|x| x.target_seq),
            manual: existing.as_ref().is_some_and(|x| x.mode == AllocationMode::Spread),
            spread: Vec::new(),
            custom_principal,
            custom_interest,
            custom_fee,
            error: None,
            show_errors: false,
            _subs: subs,
            _per_loan: Vec::new(),
        };
        form.load_installments(window, cx);
        form
    }

    fn is_installment_loan(&self) -> bool {
        self.loan_id.and_then(|id| self.snap.loan(id)).is_some_and(|l| l.terms.method.has_installments())
    }

    fn date_or_today(&self, cx: &App) -> chrono::NaiveDate {
        picked_date(&self.date, cx).unwrap_or(crate::utils::dates::today())
    }

    /// The installment loan right before this payment.
    fn state_before(&self, cx: &App) -> Option<PlanState> {
        let loan_id = self.loan_id?;
        installment_state_before(&self.snap, loan_id, self.editing, self.date_or_today(cx), crate::utils::dates::today())
    }

    /// Fills the installment choices and the per-installment amounts for the
    /// selected loan.
    fn load_installments(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.spread.clear();
        self._per_loan.clear();
        let Some(state) = self.state_before(cx) else { return };
        let money = self.snap.settings.money();
        let ds = self.snap.settings.date_style;
        let open: Vec<_> = state.installments.iter().filter(|x| x.remaining() > 0).collect();
        let choices: Vec<Choice> = open
            .iter()
            .map(|x| {
                let (k, when, owes) = (x.seq, ds.format(x.due), money.format(x.remaining()));
                Choice { id: k as i64, title: tr!("Installment {k} · due {when} · {owes}", "Cuota {k} · vence {when} · {owes}").into() }
            })
            .collect();
        let target = self.target_seq.filter(|s| open.iter().any(|x| x.seq == *s)).or(open.first().map(|x| x.seq));
        self.target_seq = target;
        let selected = target.and_then(|s| choices.iter().position(|c| c.id == s as i64)).map(IndexPath::new);
        self.target.update(cx, |t, cx| {
            t.set_items(choices, window, cx);
            t.set_selected_index(selected, window, cx);
        });
        let previous: Vec<(u32, Cents)> = self.editing.and_then(|id| self.snap.payment(id)).map(|p| p.targets.clone()).unwrap_or_default();
        for x in open {
            let v = previous.iter().find(|(s, _)| *s == x.seq).map(|(_, a)| MoneyFormat::input_value(*a));
            let input = cx.new(|cx| {
                let s = InputState::new(window, cx).placeholder("0.00");
                match v {
                    Some(v) => s.default_value(v),
                    None => s,
                }
            });
            self._per_loan.push(notify_on_change(&input, window, cx));
            self.spread.push((x.seq, input));
        }
    }

    /// What the client owed at the moment of this payment (interest-only loans).
    fn owed(&self, on: chrono::NaiveDate) -> Option<Position> {
        let loan = self.snap.loan(self.loan_id?)?;
        Some(owed_at_payment(&loan_facts(loan), &payments_before(&self.snap, loan.id, self.editing, on), on))
    }

    /// Live payments on the loan dated after this one; they are worked out
    /// again on save.
    fn later_payments(&self, on: chrono::NaiveDate) -> usize {
        let Some(loan_id) = self.loan_id else { return 0 };
        let key = (on, self.editing.unwrap_or(i64::MAX));
        self.snap.payments_for_loan(loan_id).filter(|x| !x.is_reversed() && Some(x.id) != self.editing && (x.paid_on, x.id) > key).count()
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

    fn optional_money(state: &Entity<InputState>, cx: &App) -> Result<Cents, String> {
        let text = value(state, cx);
        if text.trim().is_empty() { Ok(0) } else { parse_money(&text).map_err(|e| e.to_string()) }
    }

    /// The amount and how it should be applied (installment loans).
    fn request(&self, cx: &App) -> Result<(Cents, Instruction), String> {
        match self.option {
            PayOption::Installment => {
                let seq = self.target_seq.ok_or_else(|| t("Choose the installment", "Elige la cuota").to_string())?;
                Ok((parse_money(&value(&self.amount, cx)).map_err(|e| e.to_string())?, Instruction::Installment(seq)))
            }
            PayOption::Spread if !self.manual => Ok((parse_money(&value(&self.amount, cx)).map_err(|e| e.to_string())?, Instruction::Auto)),
            PayOption::Spread => {
                let mut parts = Vec::new();
                for (seq, input) in &self.spread {
                    let a = Self::optional_money(input, cx).map_err(|e| tr!("Installment {seq}: {e}", "Cuota {seq}: {e}"))?;
                    if a > 0 {
                        parts.push((*seq, a));
                    }
                }
                let total = parts.iter().map(|(_, a)| a).sum();
                Ok((total, Instruction::Spread(parts)))
            }
            PayOption::Custom => {
                let principal = Self::optional_money(&self.custom_principal, cx)?;
                let interest = Self::optional_money(&self.custom_interest, cx)?;
                let fee = Self::optional_money(&self.custom_fee, cx)?;
                Ok((principal + interest + fee, Instruction::Custom { principal, interest, fee }))
            }
        }
    }

    fn installment_preview(&self, cx: &App) -> Option<Result<(Vec<Applied>, PlanState), String>> {
        let money = self.snap.settings.money();
        let (amount, instruction) = match self.request(cx) {
            // Nothing typed yet is not an error until the form is submitted.
            Ok((0, _)) if !self.show_errors => return None,
            Err(_) if !self.show_errors && value(&self.amount, cx).trim().is_empty() => return None,
            Ok(r) => r,
            Err(e) => return Some(Err(e)),
        };
        let date = picked_date(&self.date, cx)?;
        preview_installment_payment(&self.snap, self.loan_id?, self.editing, amount, date, instruction, crate::utils::dates::today())
            .map(|r| r.map_err(|e| e.message(&money)))
    }

    fn preview(&self, cx: &App) -> Option<Result<Allocation, PaymentError>> {
        let (Ok(amount), Ok(fee)) = self.amounts(cx) else { return None };
        let date = picked_date(&self.date, cx)?;
        preview_payment(&self.snap, self.loan_id?, self.editing, amount, fee, date, crate::utils::dates::today())
    }

    fn set_amount(&mut self, cents: Cents, window: &mut Window, cx: &mut Context<Self>) {
        self.amount.update(cx, |s, cx| s.set_value(MoneyFormat::input_value(cents), window, cx));
        cx.notify();
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let (Some(loan_id), Some(date)) = (self.loan_id, picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        let notes = value(&self.notes, cx);
        let reason = value(&self.reason, cx);
        let editing = self.editing;
        let installment = self.is_installment_loan();
        let (amount, fee, instruction) = if installment {
            match self.request(cx) {
                Ok((amount, instruction)) => (amount, 0, Some(instruction)),
                Err(e) => {
                    self.error = Some(e);
                    cx.notify();
                    return;
                }
            }
        } else {
            match self.amounts(cx) {
                (Ok(amount), Ok(fee)) => (amount, fee, None),
                _ => {
                    cx.notify();
                    return;
                }
            }
        };
        let print = self.print_receipt;
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            let r = match (editing, &instruction) {
                (Some(id), Some(i)) => app.store.update_installment_payment(id, amount, date, &notes, i, &reason, today).map(|_| id),
                (Some(id), None) => app.store.update_payment(id, amount, fee, date, &notes, &reason, today).map(|_| id),
                (None, Some(i)) => app.store.record_installment_payment(loan_id, amount, date, &notes, i, today),
                (None, None) => app.store.record_payment(loan_id, amount, fee, date, &notes, today),
            };
            match r {
                Ok(id) => {
                    app.reload(cx);
                    let closed = app.snap.loan(loan_id).is_some_and(|l| l.status == ContractStatus::PaidOff);
                    if editing.is_none() && app.settings().receipt_after_payment != print {
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
                    if editing.is_some() {
                        app.notify_success(
                            t(
                                "Payment updated. The loan's payments were worked out again.",
                                "Pago actualizado. Los pagos del préstamo se recalcularon.",
                            ),
                            window,
                            cx,
                        );
                        return;
                    }
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

    fn loan_control(&self, p: &Palette) -> AnyElement {
        if self.editing.is_some() {
            let lid = self.loan_id.unwrap_or_default();
            let name = self.snap.loan(lid).map(|l| self.snap.client_name(l.client_id).to_string()).unwrap_or_default();
            div()
                .h(px(32.))
                .px(px(space::M))
                .flex()
                .items_center()
                .rounded(px(8.))
                .bg(p.raised)
                .text_color(p.muted)
                .child(tr!("{name} — Loan #{lid}", "{name} — Préstamo #{lid}"))
                .into_any_element()
        } else {
            Select::new(&self.loan)
                .placeholder(t("Choose a client's loan", "Elige el préstamo de un cliente"))
                .search_placeholder(t("Search by client", "Buscar por cliente"))
                .into_any_element()
        }
    }

    fn receipt_switch(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .gap(px(10.))
            .child(Switch::new("print-receipt").checked(self.print_receipt).on_click(cx.listener(|this, checked: &bool, _, cx| {
                this.print_receipt = *checked;
                cx.notify();
            })))
            .child(div().child(t("Open the receipt to print after saving", "Abrir el recibo para imprimir al guardar")))
    }

    fn render_installments(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let money = self.snap.settings.money();
        let ds = self.snap.settings.date_style;
        let picked = picked_date(&self.date, cx);
        let on = picked.unwrap_or(crate::utils::dates::today());
        let editing = self.editing.is_some();
        let before = self.state_before(cx);
        let preview = self.installment_preview(cx);
        let later = self.later_payments(on);

        let summary = before.as_ref().map(|s| {
            let due_now: Cents = s.installments.iter().filter(|x| x.due < on).map(|x| x.owed()).sum();
            let fees: Cents = s.installments.iter().map(|x| x.unpaid_fee()).sum();
            let next = s.installments.iter().find(|x| x.due >= on && x.remaining() > 0);
            h_flex()
                .gap(px(space::L))
                .flex_wrap()
                .text_size(px(size::META + 0.5))
                .text_color(p.muted)
                .when(due_now > 0, |d| {
                    let a = money.format(due_now);
                    d.child(div().text_color(p.negative).child(tr!("Overdue: {a}", "Atrasado: {a}")))
                })
                .when(fees > 0, |d| {
                    let f = money.format(fees);
                    d.child(div().text_color(p.negative).child(tr!("Late fees: {f}", "Mora: {f}")))
                })
                .when_some(next, |d, x| {
                    let (k, when, a) = (x.seq, ds.format(x.due), money.format(x.remaining()));
                    d.child(tr!("Next: #{k} on {when}, {a}", "Próxima: #{k} el {when}, {a}"))
                })
                .child({
                    let payoff = money.format(s.payoff());
                    tr!("Pays off today: {payoff}", "Salda hoy: {payoff}")
                })
        });

        let options = h_flex().gap(px(space::S)).items_stretch().children(
            [
                (
                    PayOption::Installment,
                    "opt-one",
                    t("Pay an installment", "Pagar una cuota"),
                    t("Partial payments allowed", "Acepta pagos parciales"),
                ),
                (
                    PayOption::Spread,
                    "opt-spread",
                    t("Spread over installments", "Repartir entre cuotas"),
                    t("Oldest first, or your amounts", "Lo más antiguo primero, o tus montos"),
                ),
                (
                    PayOption::Custom,
                    "opt-custom",
                    t("Choose the split", "Elegir el reparto"),
                    t("Capital, interest and fees", "Capital, interés y cargos"),
                ),
            ]
            .map(|(o, id, title, sub)| {
                choice_tile(p, id, title, Some(sub.to_string()), self.option == o).on_click(cx.listener(move |this, _, _, cx| {
                    this.option = o;
                    this.error = None;
                    cx.notify();
                }))
            }),
        );

        let open: Vec<_> =
            before.as_ref().map(|s| s.installments.iter().filter(|x| x.remaining() > 0).cloned().collect()).unwrap_or_default();
        let amount_input = |this: &Self, label: &str| {
            field(p, label, Input::new(&this.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())), None, None)
        };
        let quick = |id: &'static str, label: String, cents: Cents, cx: &mut Context<Self>| {
            Button::new(id).small().label(label).on_click(cx.listener(move |this, _, w, cx| this.set_amount(cents, w, cx)))
        };
        let controls: AnyElement = match self.option {
            PayOption::Installment => {
                let target = open.iter().find(|x| Some(x.seq) == self.target_seq);
                v_flex()
                    .gap(px(space::M))
                    .child(field(
                        p,
                        t("Installment", "Cuota"),
                        Select::new(&self.target).placeholder(t("Choose the installment", "Elige la cuota")),
                        None,
                        None,
                    ))
                    .child(amount_input(self, t("Amount received", "Monto recibido")).children(target.map(|x| {
                        let owes = x.owed().max(0);
                        let label = {
                            let a = money.format(owes);
                            tr!("All of it {a}", "Todo {a}")
                        };
                        h_flex().gap(px(space::S)).when(owes > 0, |d| d.child(quick("fill-installment", label, owes, cx)))
                    })))
                    .into_any_element()
            }
            PayOption::Spread => {
                let mode = h_flex().gap(px(space::S)).children(
                    [
                        (false, "spread-auto", t("Oldest first", "Lo más antiguo primero")),
                        (true, "spread-manual", t("Choose amounts", "Elegir montos")),
                    ]
                    .map(|(manual, id, label)| {
                        choice_tile(p, id, label, None, self.manual == manual).on_click(cx.listener(move |this, _, _, cx| {
                            this.manual = manual;
                            cx.notify();
                        }))
                    }),
                );
                let body: AnyElement = if self.manual {
                    let rows = self.spread.iter().filter_map(|(seq, input)| {
                        let x = open.iter().find(|x| x.seq == *seq)?;
                        Some(
                            h_flex()
                                .gap(px(space::M))
                                .min_h(px(36.))
                                .font_features(tabular())
                                .child(div().w(px(28.)).text_color(p.faint).child(seq.to_string()))
                                .child(div().w(px(110.)).child(ds.format(x.due)))
                                .child(div().flex_1().text_right().text_color(p.muted).child({
                                    let owes = money.format(x.owed());
                                    tr!("owes {owes} now", "debe {owes} ahora")
                                }))
                                .child(
                                    div()
                                        .w(px(150.))
                                        .child(Input::new(input).small().prefix(div().text_color(p.muted).child(money.symbol.clone()))),
                                ),
                        )
                    });
                    let total: Cents = self.spread.iter().filter_map(|(_, i)| parse_money(&value(i, cx)).ok()).sum();
                    v_flex()
                        .gap(px(2.))
                        .child(div().id("spread-rows").max_h(px(220.)).overflow_y_scroll().child(v_flex().children(rows)))
                        .child(kv(p, t("Total received", "Total recibido"), money.format(total)))
                        .into_any_element()
                } else {
                    let due_now: Cents = open.iter().filter(|x| x.due <= on).map(|x| x.owed()).sum();
                    let payoff: Cents = open.iter().map(|x| x.owed()).sum();
                    amount_input(self, t("Amount received", "Monto recibido"))
                        .child(
                            h_flex()
                                .gap(px(space::S))
                                .when(due_now > 0, |d| {
                                    let a = money.format(due_now);
                                    d.child(quick("fill-due", tr!("Everything due {a}", "Todo lo vencido {a}"), due_now, cx))
                                })
                                .when(payoff > 0, |d| {
                                    let a = money.format(payoff);
                                    d.child(quick("fill-payoff", tr!("Pay off {a}", "Saldar {a}"), payoff, cx))
                                }),
                        )
                        .into_any_element()
                };
                v_flex().gap(px(space::M)).child(mode).child(body).into_any_element()
            }
            PayOption::Custom => {
                let owed = |f: fn(&crate::domain::installments::InstallmentState) -> Cents| open.iter().map(f).sum::<Cents>();
                let (fees, interest, capital) = (owed(|x| x.unpaid_fee()), owed(|x| x.unpaid_interest()), owed(|x| x.unpaid_principal()));
                let part = |label: &str, input: &Entity<InputState>, max: Cents| {
                    let m = money.format(max);
                    div().flex_1().child(field(
                        p,
                        label,
                        Input::new(input).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                        Some(tr!("Owed now: {m}", "Se debe ahora: {m}")),
                        None,
                    ))
                };
                let total = self.request(cx).map(|(a, _)| a).unwrap_or_default();
                v_flex()
                    .gap(px(space::S))
                    .child(
                        h_flex()
                            .gap(px(space::M))
                            .items_start()
                            .child(part(t("To late fees", "A cargos por mora"), &self.custom_fee, fees))
                            .child(part(t("To interest", "A interés"), &self.custom_interest, interest))
                            .child(part(t("To capital", "A capital"), &self.custom_principal, capital)),
                    )
                    .child(kv(p, t("Total received", "Total recibido"), money.format(total)))
                    .child(div().text_size(px(size::META + 0.5)).text_color(p.warning).child(t(
                        "Each part is applied to the oldest installments first. Capital paid ahead of the schedule lowers the capital of later installments, so the regular schedule changes.",
                        "Cada parte se aplica primero a las cuotas más antiguas. El capital pagado antes del calendario baja el capital de las cuotas siguientes, así que el calendario normal cambia.",
                    )))
                    .into_any_element()
            }
        };

        let (preview_el, can_submit): (AnyElement, bool) = match &preview {
            Some(Ok((applied, after))) => {
                let rows = applied.iter().map(|a| {
                    let x = after.installments.iter().find(|x| x.seq == a.seq);
                    h_flex()
                        .h(px(26.))
                        .gap(px(space::M))
                        .font_features(tabular())
                        .text_size(px(size::META + 0.5))
                        .child(div().w(px(90.)).child({
                            let k = a.seq;
                            tr!("Installment {k}", "Cuota {k}")
                        }))
                        .child(div().flex_1().text_right().child(money.format(a.fee)))
                        .child(div().flex_1().text_right().child(money.format(a.interest)))
                        .child(div().flex_1().text_right().child(money.format(a.principal)))
                        .child(div().w(px(104.)).flex().justify_end().children(x.map(|x| {
                            let st = x.status(on);
                            pill(
                                p,
                                st.label(),
                                if st == crate::domain::installments::InstallmentStatus::Paid { Tone::Positive } else { Tone::Warning },
                            )
                        })))
                });
                let due_now: Cents = after.installments.iter().filter(|x| x.due <= on).map(|x| x.owed()).sum();
                let fees: Cents = after.installments.iter().map(|x| x.unpaid_fee()).sum();
                let next = after.installments.iter().find(|x| x.due > on && x.remaining() > 0);
                (
                    v_flex()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .justify_between()
                                .pb(px(6.))
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(t("How this payment is applied", "Cómo se aplica este pago")),
                                )
                                .when(after.paid_off(), |d| d.child(pill(p, t("Pays off the loan", "Salda el préstamo"), Tone::Positive))),
                        )
                        .child(
                            h_flex()
                                .gap(px(space::M))
                                .pb(px(2.))
                                .child(div().w(px(90.)).child(th(p, "")))
                                .child(div().flex_1().text_right().child(th(p, t("Late fee", "Mora"))))
                                .child(div().flex_1().text_right().child(th(p, t("Interest", "Interés"))))
                                .child(div().flex_1().text_right().child(th(p, t("Capital", "Capital"))))
                                .child(div().w(px(104.))),
                        )
                        .children(rows)
                        .child(div().h(px(1.)).my(px(6.)).bg(p.border))
                        .child(kv(p, t("Capital remaining", "Capital pendiente"), money.format(after.principal_outstanding())))
                        .when(due_now > 0, |d| {
                            d.child(kv(
                                p,
                                t("Still due and unpaid", "Aún vencido sin pagar"),
                                div().text_color(p.negative).child(money.format(due_now)),
                            ))
                        })
                        .when(fees > 0, |d| d.child(kv(p, t("Late fees pending", "Mora pendiente"), money.format(fees))))
                        .when_some(next, |d, x| {
                            let (a, when) = (money.format(x.remaining()), ds.format(x.due));
                            d.child(kv(p, t("Next installment", "Próxima cuota"), tr!("{a} on {when}", "{a} el {when}")))
                        })
                        .into_any_element(),
                    true,
                )
            }
            Some(Err(e)) => (div().text_color(p.negative).child(e.clone()).into_any_element(), false),
            None => (
                div()
                    .text_color(p.muted)
                    .child(t("Enter the amount received to see where it goes.", "Escribe el monto recibido para ver a dónde va."))
                    .into_any_element(),
                false,
            ),
        };
        let date_err = picked.is_none().then(invalid_date);
        let date_hint = match later {
            0 => None,
            1 => Some(t("1 later payment will be worked out again", "1 pago posterior se volverá a calcular").to_string()),
            n => Some(tr!("{n} later payments will be worked out again", "{n} pagos posteriores se volverán a calcular")),
        };

        v_flex()
            .gap(px(space::L))
            .children(error_banner(p, &self.error))
            .child(field(p, t("Loan", "Préstamo"), self.loan_control(p), None, None).children(summary))
            .child(field(p, t("How to apply it", "Cómo aplicarlo"), options, None, None))
            .child({
                let date = div().w(px(200.)).flex_none().child(field(
                    p,
                    t("Date received", "Fecha del pago"),
                    self.date.clone(),
                    date_hint,
                    date_err,
                ));
                // The split and per-installment amounts need the full width.
                if self.option == PayOption::Custom || (self.option == PayOption::Spread && self.manual) {
                    v_flex().gap(px(space::L)).child(date).child(controls).into_any_element()
                } else {
                    h_flex().items_start().gap(px(space::M)).child(div().flex_1().min_w_0().child(controls)).child(date).into_any_element()
                }
            })
            .child(field(p, t("Note", "Nota"), Input::new(&self.notes), None, None))
            .child(div().p(px(space::L)).rounded(px(10.)).bg(p.raised).border_1().border_color(p.rule).child(preview_el))
            .when(editing, |d| d.child(field(p, t("Reason for the change", "Motivo del cambio"), Input::new(&self.reason), None, None)))
            .when(!editing, |d| d.child(self.receipt_switch(cx)))
            .child(footer(
                p,
                if editing { t("Save changes", "Guardar cambios") } else { t("Record payment", "Registrar pago") },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
            .into_any_element()
    }
}

impl Render for PaymentForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        if self.is_installment_loan() {
            return self.render_installments(&p, cx);
        }
        let money = self.snap.settings.money();
        let today = crate::utils::dates::today();
        let book = self.loan_id.and_then(|id| self.snap.book_loan(id));
        let (amount, fee) = self.amounts(cx);
        let preview = self.preview(cx);
        let picked = picked_date(&self.date, cx);

        // What the client owes on the chosen date, for the quick-fill buttons.
        let owed = self.owed(picked.unwrap_or(today));
        let later = self.later_payments(picked.unwrap_or(today));

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
            _ => None,
        };
        let date_hint = match later {
            0 => None,
            1 => Some(t("1 later payment will be split again", "1 pago posterior se volverá a dividir").to_string()),
            n => Some(tr!("{n} later payments will be split again", "{n} pagos posteriores se volverán a dividir")),
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
                let next_interest = l.facts.rate.interest(a.principal_after);
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
        let editing = self.editing.is_some();
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(field(&p, t("Loan", "Préstamo"), self.loan_control(&p), None, loan_err).children(summary))
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
                    .child(div().w(px(200.)).child(field(
                        &p,
                        t("Date received", "Fecha del pago"),
                        self.date.clone(),
                        date_hint,
                        date_err,
                    ))),
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
            .when(editing, |d| d.child(field(&p, t("Reason for the change", "Motivo del cambio"), Input::new(&self.reason), None, None)))
            .when(!editing, |d| d.child(self.receipt_switch(cx)))
            .child(footer(
                &p,
                if editing { t("Save changes", "Guardar cambios") } else { t("Record payment", "Registrar pago") },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
            .into_any_element()
    }
}
