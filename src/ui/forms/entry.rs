//! New record (income from outside the business, or an expense), transfers
//! between the two accounts, and editing those and balance adjustments.

use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{
    DATE_WIDTH, choice_tile, date_field, error_banner, field, field_row, footer, grow_field, invalid_date, picked_date, reason_input, value,
};
use crate::app::VzApp;
use crate::domain::money::{MoneyFormat, parse_money};
use crate::domain::portfolio::{Account, EntryKind};
use crate::i18n::t;
use crate::services::{Record, Snapshot};
use crate::sync::change::Change;
use crate::tr;
use crate::ui::kit::Segmented;
use crate::ui::theme::{Typeset as _, palette, space, text};

pub struct EntryForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    pub kind: EntryKind,
    editing: Option<i64>,
    /// Expense: pays. Income: receives. Transfer: money leaves it.
    /// Adjustment: corrected.
    account: Account,
    /// Adjustments only: add to the balance (true) or take from it.
    increase: bool,
    pub amount: Entity<InputState>,
    date: Entity<DateField>,
    description: Entity<InputState>,
    reason: Entity<InputState>,
    error: Option<String>,
    show_errors: bool,
    /// Set once saved: a second Enter or click before the dialog closes does
    /// nothing.
    saved: bool,
    _subs: Vec<Subscription>,
}

fn placeholder(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Expense => t("e.g. Car repair, medicine, school fees", "ej. Reparación del carro, medicinas, colegio"),
        EntryKind::Income => {
            t("e.g. Rent received, sale of equipment, another business", "ej. Alquiler cobrado, venta de equipo, otro negocio")
        }
        EntryKind::Transfer => t("Optional: e.g. reinvesting profits", "Opcional: ej. reinvertir ganancias"),
        EntryKind::Adjustment => t("Why the balance was corrected", "Por qué se corrigió el saldo"),
    }
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
            let s = InputState::new(window, cx).placeholder(placeholder(kind));
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
            saved: false,
            _subs: subs,
        }
    }

    pub fn title(kind: EntryKind, editing: bool) -> &'static str {
        match (kind, editing) {
            (EntryKind::Expense | EntryKind::Income, false) => t("New record", "Nuevo registro"),
            (EntryKind::Expense, true) => t("Edit expense", "Editar gasto"),
            (EntryKind::Income, true) => t("Edit income", "Editar ingreso"),
            (EntryKind::Transfer, false) => t("Move money", "Mover dinero"),
            (EntryKind::Transfer, true) => t("Edit money moved", "Editar dinero movido"),
            (EntryKind::Adjustment, _) => t("Edit adjustment", "Editar ajuste"),
        }
    }

    /// Switches a new record between income and expense. What was typed
    /// stays.
    fn set_kind(&mut self, kind: EntryKind, window: &mut Window, cx: &mut Context<Self>) {
        self.kind = kind;
        self.error = None;
        self.description.update(cx, |s, cx| s.set_placeholder(placeholder(kind), window, cx));
        cx.notify();
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

    fn is_outflow(&self) -> bool {
        match self.kind {
            EntryKind::Expense | EntryKind::Transfer => true,
            EntryKind::Income => false,
            EntryKind::Adjustment => !self.increase,
        }
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button. Whatever
    /// was typed stays when something is wrong.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saved {
            return;
        }
        self.show_errors = true;
        let description = value(&self.description, cx);
        let needs_text = matches!(self.kind, EntryKind::Expense | EntryKind::Income | EntryKind::Adjustment);
        let (Ok(amount), Some(date)) = (parse_money(&value(&self.amount, cx)), picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        if amount <= 0 || (needs_text && description.trim().is_empty()) {
            cx.notify();
            return;
        }
        let (kind, editing, account) = (self.kind, self.editing, self.account);
        let signed = if kind == EntryKind::Adjustment && !self.increase { -amount } else { amount };
        let reason = value(&self.reason, cx);
        let result = self.app.update(cx, |app, cx| {
            let change = match (editing, kind) {
                (Some(id), _) => Change::EditEntry { id, account, amount: signed, on: date, description, reason },
                (None, EntryKind::Transfer | EntryKind::Income) => Change::NewEntry { kind, account, amount, on: date, description },
                (None, _) => Change::NewEntry { kind: EntryKind::Expense, account, amount, on: date, description },
            };
            match app.save(change, window, cx) {
                Ok(_) => {
                    app.reload(cx);
                    Ok(())
                }
                Err(e) => Err(app.error_message(&e)),
            }
        });
        match result {
            Ok(Ok(())) => {
                self.saved = true;
                window.close_dialog(cx);
                let _ = self.app.update(cx, |app, cx| {
                    let m = app.settings().money().format(amount);
                    let msg = match (editing, kind) {
                        (Some(_), _) => t("Changes saved", "Cambios guardados").to_string(),
                        (None, EntryKind::Transfer) => {
                            let to = account.other().label();
                            tr!("{m} moved to {to}", "{m} movido a {to}")
                        }
                        (None, EntryKind::Income) => {
                            let into = account.label();
                            tr!("Income of {m} recorded into {into}", "Ingreso de {m} registrado en {into}")
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
        let outflow = self.is_outflow();
        let available = self.available(self.account, cx);
        let avail = money.format(available);
        let new_record = self.editing.is_none() && matches!(self.kind, EntryKind::Expense | EntryKind::Income);

        let record_type = Segmented::pick(
            "record-type",
            vec![
                (EntryKind::Income, SharedString::from(t("Income", "Ingreso"))),
                (EntryKind::Expense, SharedString::from(t("Expense", "Gasto"))),
            ],
            self.kind,
            cx,
            |this: &mut Self, kind, window, cx| this.set_kind(kind, window, cx),
        );
        let account_tiles = h_flex().gap(px(space::S)).items_stretch().children(Account::ALL.map(|a| {
            let sub = if self.kind == EntryKind::Income {
                match a {
                    Account::Interest => t("Counts as income", "Cuenta como ganancia").to_string(),
                    Account::Capital => t("Counts as capital invested", "Cuenta como capital invertido").to_string(),
                }
            } else {
                let left = money.format(self.available(a, cx));
                tr!("{left} available", "{left} disponible")
            };
            choice_tile(&p, a.key(), a.label(), Some(sub), self.account == a).on_click(cx.listener(move |this, _, _, cx| {
                this.account = a;
                cx.notify();
            }))
        }));
        let account_label = match self.kind {
            EntryKind::Expense => t("Pay from", "Pagar de"),
            EntryKind::Income => t("Put into", "Entra a"),
            EntryKind::Transfer => t("Move from", "Mover de"),
            EntryKind::Adjustment => t("Account", "Cuenta"),
        };
        let directions = vec![
            (true, SharedString::from(t("Add to the balance", "Sumar al saldo"))),
            (false, SharedString::from(t("Take from the balance", "Restar del saldo"))),
        ];
        let direction = Segmented::pick("adj-direction", directions, self.increase, cx, |this: &mut Self, up, _, cx| {
            this.increase = up;
            cx.notify();
        });

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
        let desc_err = (self.show_errors && needs_text && description.trim().is_empty()).then(|| {
            match self.kind {
                EntryKind::Income => t("Write where the income came from", "Escribe de dónde vino el ingreso"),
                EntryKind::Adjustment => t("Write why the balance was corrected", "Escribe por qué se corrigió el saldo"),
                _ => t("Write what it was for", "Escribe para qué fue"),
            }
            .to_string()
        });
        // The button stays usable so a click shows what is missing.
        let can_submit = !self.saved && amount_err.is_none() && date_err.is_none();
        let explain = match self.kind {
            EntryKind::Expense => t(
                "Money taken out for something outside the business. Only the account you choose goes down; loans and interest earned stay the same.",
                "Dinero sacado para algo fuera del negocio. Solo baja la cuenta que elijas; los préstamos y el interés cobrado no cambian.",
            ),
            EntryKind::Income => t(
                "Money received from outside the business, not a loan payment and not money moved between your accounts. Into available interest it counts as income; into available capital it counts as capital invested.",
                "Dinero recibido de fuera del negocio, que no es un pago de préstamo ni dinero movido entre tus cuentas. En el interés disponible cuenta como ganancia; en el capital disponible cuenta como capital invertido.",
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
            .when(new_record, |d| d.child(field(&p, t("Record type", "Tipo de registro"), record_type, None, None)))
            .child(div().typeset(text::BODY).text_color(p.muted).child(explain))
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
                field_row()
                    .child(grow_field(field(
                        &p,
                        t("Amount", "Monto"),
                        Input::new(&self.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                        None,
                        amount_err,
                    )))
                    .child(div().w(px(DATE_WIDTH)).flex_none().child(field(&p, t("Date", "Fecha"), self.date.clone(), None, date_err))),
            )
            .child(field(
                &p,
                match self.kind {
                    EntryKind::Expense => t("What was it for?", "¿En qué fue?"),
                    EntryKind::Income => t("Where did it come from?", "¿De dónde vino?"),
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
                    (None, EntryKind::Income) => t("Record income", "Registrar ingreso"),
                    (None, _) => t("Record expense", "Registrar gasto"),
                },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}

#[cfg(test)]
mod tests {
    // The kit's glob import also brings GPUI's `test` macro; keep the
    // built-in one for what `#[gpui::test]` expands to.
    use ::gpui;
    use core::prelude::v1::test;

    use super::*;
    use crate::services::Store;
    use crate::ui::forms::test_app;

    fn new_record(cx: &mut TestAppContext) -> (Entity<VzApp>, Entity<EntryForm>, &mut VisualTestContext) {
        let (app, cx) = test_app(cx, Store::in_memory());
        let form = cx.update(|window, cx| {
            let snap = app.read(cx).snap.clone();
            let weak = app.downgrade();
            cx.new(|cx| EntryForm::new(weak, snap, EntryKind::Expense, None, window, cx))
        });
        (app, form, cx)
    }

    fn type_in(form: &Entity<EntryForm>, amount: &str, reason: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            form.update(cx, |f, cx| {
                f.amount.update(cx, |s, cx| s.set_value(amount.to_string(), window, cx));
                f.description.update(cx, |s, cx| s.set_value(reason.to_string(), window, cx));
            })
        });
    }

    fn entries(app: &Entity<VzApp>, cx: &mut VisualTestContext) -> Vec<(EntryKind, i64, String)> {
        cx.update(|_, cx| app.read(cx).snap.entries.iter().map(|e| (e.kind, e.amount, e.description.clone())).collect())
    }

    fn submit(form: &Entity<EntryForm>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| form.update(cx, |f, cx| f.submit(window, cx)));
        cx.run_until_parked();
    }

    #[gpui::test]
    fn records_income_with_a_reason_once(cx: &mut TestAppContext) {
        let (app, form, cx) = new_record(cx);
        cx.update(|window, cx| form.update(cx, |f, cx| f.set_kind(EntryKind::Income, window, cx)));
        // A reason of spaces is refused and what was typed stays.
        type_in(&form, "250", "   ", cx);
        submit(&form, cx);
        assert!(entries(&app, cx).is_empty());
        let kept = cx.update(|_, cx| (value(&form.read(cx).amount, cx), form.read(cx).show_errors));
        assert_eq!(kept, ("250".to_string(), true));
        // With a reason it saves, into available interest by default.
        type_in(&form, "250", "Rent from the shop", cx);
        submit(&form, cx);
        assert_eq!(entries(&app, cx), vec![(EntryKind::Income, 25_000, "Rent from the shop".into())]);
        // Enter pressed again before the dialog closes adds nothing.
        submit(&form, cx);
        assert_eq!(entries(&app, cx).len(), 1);
        let interest = cx.update(|_, cx| app.read(cx).snap.book.summary(crate::utils::dates::today()).available_interest);
        assert_eq!(interest, 25_000);
    }

    #[gpui::test]
    fn an_expense_needs_an_amount_and_money_to_pay_it(cx: &mut TestAppContext) {
        let (app, form, cx) = new_record(cx);
        type_in(&form, "", "Printer ink", cx);
        submit(&form, cx);
        type_in(&form, "abc", "Printer ink", cx);
        submit(&form, cx);
        assert!(entries(&app, cx).is_empty());
        // Nothing in the account: the store refuses and says why.
        type_in(&form, "40", "Printer ink", cx);
        submit(&form, cx);
        assert!(entries(&app, cx).is_empty());
        assert!(cx.update(|_, cx| form.read(cx).error.clone()).is_some());
        // Switching to income keeps what was typed and saves it.
        cx.update(|window, cx| form.update(cx, |f, cx| f.set_kind(EntryKind::Income, window, cx)));
        assert_eq!(cx.update(|_, cx| value(&form.read(cx).description, cx)), "Printer ink");
    }
}
