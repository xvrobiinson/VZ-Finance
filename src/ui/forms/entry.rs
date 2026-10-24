//! Expenses (money spent outside the business), transfers between the two
//! accounts, and editing those and balance adjustments.

use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{choice_tile, date_field, error_banner, field, footer, invalid_date, picked_date, reason_input, value};
use crate::app::VzApp;
use crate::domain::money::{MoneyFormat, parse_money};
use crate::domain::portfolio::{Account, EntryKind};
use crate::i18n::t;
use crate::services::{Record, Snapshot};
use crate::tr;
use crate::ui::theme::{palette, space};

pub struct EntryForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    pub kind: EntryKind,
    editing: Option<i64>,
    /// Expense: pays. Transfer: money leaves it. Adjustment: corrected.
    account: Account,
    /// Adjustments only: add to the balance (true) or take from it.
    increase: bool,
    pub amount: Entity<InputState>,
    date: Entity<DateField>,
    description: Entity<InputState>,
    reason: Entity<InputState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl EntryForm {
    pub fn new(
        app: WeakEntity<VzApp>,
        snap: Rc<Snapshot>,
        kind: EntryKind,
        editing: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let existing = editing.and_then(|id| snap.entry(id).cloned());
        let kind = existing.as_ref().map_or(kind, |e| e.kind);
        let amount = cx.new(|cx| {
            let s = InputState::new(window, cx).placeholder("0.00");
            match &existing {
                Some(e) => s.default_value(MoneyFormat::input_value(e.amount.abs())),
                None => s,
            }
        });
        let description = cx.new(|cx| {
            let s = InputState::new(window, cx).placeholder(match kind {
                EntryKind::Expense => t("e.g. Car repair, medicine, school fees", "ej. Reparación del carro, medicinas, colegio"),
                EntryKind::Transfer => t("Optional: e.g. reinvesting profits", "Opcional: ej. reinvertir ganancias"),
                EntryKind::Adjustment => t("Why the balance was corrected", "Por qué se corrigió el saldo"),
            });
            match &existing {
                Some(e) => s.default_value(e.description.clone()),
                None => s,
            }
        });
        let reason = reason_input(window, cx);
        let date = date_field(existing.as_ref().map_or(crate::utils::dates::today(), |e| e.occurred_on), window, cx);
        let mut subs = vec![cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify())];
        for input in [&amount, &description, &reason] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self {
            app,
            snap,
            kind,
            editing,
            account: existing.as_ref().map_or(Account::Interest, |e| e.account),
            increase: existing.as_ref().is_none_or(|e| e.amount > 0),
            amount,
            date,
            description,
            reason,
            error: None,
            show_errors: false,
            _subs: subs,
        }
    }

    pub fn title(kind: EntryKind, editing: bool) -> &'static str {
        match (kind, editing) {
            (EntryKind::Expense, false) => t("New expense", "Nuevo gasto"),
            (EntryKind::Expense, true) => t("Edit expense", "Editar gasto"),
            (EntryKind::Transfer, false) => t("Move money", "Mover dinero"),
            (EntryKind::Transfer, true) => t("Edit money moved", "Editar dinero movido"),
            (EntryKind::Adjustment, _) => t("Edit adjustment", "Editar ajuste"),
        }
    }

    /// What `account` can give up on `date`: the most an outflow may take.
    fn available(&self, account: Account, cx: &App) -> i64 {
        let date = picked_date(&self.date, cx).unwrap_or(crate::utils::dates::today());
        let book = match self.editing {
            Some(id) => self.snap.book_without(Record::Entry(id)),
            None => self.snap.book.clone(),
        };
        book.available_from(date, Some(account)).max(0)
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let (Ok(amount), Some(date)) = (parse_money(&value(&self.amount, cx)), picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        let (kind, editing, account) = (self.kind, self.editing, self.account);
        let signed = if kind == EntryKind::Adjustment && !self.increase { -amount } else { amount };
        let description = value(&self.description, cx);
        let reason = value(&self.reason, cx);
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            let r = match (editing, kind) {
                (Some(id), _) => app.store.update_entry(id, account, signed, date, &description, &reason, today),
                (None, EntryKind::Transfer) => app.store.add_transfer(account, amount, date, &description, today).map(|_| ()),
                (None, _) => app.store.add_expense(account, amount, date, &description, today).map(|_| ()),
            };
            match r {
                Ok(()) => {
                    app.reload(cx);
                    Ok(())
                }
                Err(e) => Err(app.error_message(&e)),
            }
        });
        match result {
            Ok(Ok(())) => {
                window.close_dialog(cx);
                let _ = self.app.update(cx, |app, cx| {
                    let m = app.settings().money().format(amount);
                    let msg = match (editing, kind) {
                        (Some(_), _) => t("Changes saved", "Cambios guardados").to_string(),
                        (None, EntryKind::Transfer) => {
                            let to = account.other().label();
                            tr!("{m} moved to {to}", "{m} movido a {to}")
                        }
                        (None, _) => {
                            let from = account.label();
                            tr!("Expense of {m} recorded from {from}", "Gasto de {m} registrado de {from}")
                        }
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

impl Render for EntryForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let money = self.snap.settings.money();
        let today = crate::utils::dates::today();
        let amount = parse_money(&value(&self.amount, cx));
        let picked = picked_date(&self.date, cx);
        let outflow = self.kind != EntryKind::Adjustment || !self.increase;
        let available = self.available(self.account, cx);
        let avail = money.format(available);

        let account_tiles = h_flex().gap(px(space::S)).items_stretch().children(Account::ALL.map(|a| {
            let left = money.format(self.available(a, cx));
            let sub = tr!("{left} available", "{left} disponible");
            choice_tile(&p, a.key(), a.label(), Some(sub), self.account == a).on_click(cx.listener(move |this, _, _, cx| {
                this.account = a;
                cx.notify();
            }))
        }));
        let account_label = match self.kind {
            EntryKind::Expense => t("Pay from", "Pagar de"),
            EntryKind::Transfer => t("Move from", "Mover de"),
            EntryKind::Adjustment => t("Account", "Cuenta"),
        };
        let direction = h_flex().gap(px(space::S)).children(
            [(true, t("Add to the balance", "Sumar al saldo")), (false, t("Take from the balance", "Restar del saldo"))].map(
                |(up, label)| {
                    choice_tile(&p, if up { "adj-up" } else { "adj-down" }, label, None, self.increase == up).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.increase = up;
                            cx.notify();
                        },
                    ))
                },
            ),
        );

        let amount_err = match &amount {
            Err(e) if self.show_errors => Some(e.to_string()),
            Ok(0) if self.show_errors => Some(t("Enter an amount greater than zero", "Escribe un monto mayor que cero").to_string()),
            Ok(a) if outflow && *a > available => {
                Some(tr!("Only {avail} is available on this date", "Solo hay {avail} disponible en esta fecha"))
            }
            _ => None,
        };
        let date_err = match picked {
            None => Some(invalid_date()),
            Some(d) if d > today => Some(t("The date can't be in the future", "La fecha no puede ser futura").to_string()),
            _ => None,
        };
        let description = value(&self.description, cx);
        let needs_text = self.kind != EntryKind::Transfer;
        let desc_err = (self.show_errors && needs_text && description.trim().is_empty())
            .then(|| t("Write what it was for", "Escribe para qué fue").to_string());
        let can_submit = matches!(amount, Ok(a) if a > 0)
            && amount_err.is_none()
            && date_err.is_none()
            && (!needs_text || !description.trim().is_empty());
        let explain = match self.kind {
            EntryKind::Expense => t(
                "Money taken out for something outside the business. Only the account you choose goes down; loans and interest earned stay the same.",
                "Dinero sacado para algo fuera del negocio. Solo baja la cuenta que elijas; los préstamos y el interés cobrado no cambian.",
            ),
            EntryKind::Transfer => t(
                "Moves money between your two accounts, for example to lend out profits as capital. The total doesn't change.",
                "Mueve dinero entre tus dos cuentas, por ejemplo para prestar las ganancias como capital. El total no cambia.",
            ),
            EntryKind::Adjustment => t("A correction to one account's balance.", "Una corrección al saldo de una cuenta."),
        };
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(div().text_color(p.muted).child(explain))
            .child(field(
                &p,
                account_label,
                account_tiles,
                (self.kind == EntryKind::Transfer).then(|| {
                    let to = self.account.other().label();
                    tr!("Goes to {to}", "Pasa a {to}")
                }),
                None,
            ))
            .when(self.kind == EntryKind::Adjustment, |d| d.child(field(&p, t("Direction", "Sentido"), direction, None, None)))
            .child(
                h_flex()
                    .items_start()
                    .gap(px(space::M))
                    .child(div().flex_1().child(field(
                        &p,
                        t("Amount", "Monto"),
                        Input::new(&self.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                        None,
                        amount_err,
                    )))
                    .child(div().w(px(200.)).child(field(&p, t("Date", "Fecha"), self.date.clone(), None, date_err))),
            )
            .child(field(
                &p,
                match self.kind {
                    EntryKind::Expense => t("What was it for?", "¿En qué fue?"),
                    EntryKind::Transfer => t("Note", "Nota"),
                    EntryKind::Adjustment => t("Reason for the adjustment", "Motivo del ajuste"),
                },
                Input::new(&self.description),
                None,
                desc_err,
            ))
            .when(self.editing.is_some(), |d| {
                d.child(field(&p, t("Reason for the change", "Motivo del cambio"), Input::new(&self.reason), None, None))
            })
            .child(footer(
                &p,
                match (self.editing, self.kind) {
                    (Some(_), _) => t("Save changes", "Guardar cambios"),
                    (None, EntryKind::Transfer) => t("Move money", "Mover dinero"),
                    (None, _) => t("Record expense", "Registrar gasto"),
                },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
