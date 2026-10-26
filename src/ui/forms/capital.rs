use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{DATE_WIDTH, date_field, error_banner, field, field_row, footer, grow_field, invalid_date, picked_date, reason_input, value};
use crate::app::VzApp;
use crate::domain::money::{MoneyFormat, parse_money};
use crate::domain::portfolio::{Account, CapitalKind};
use crate::i18n::t;
use crate::services::{Record, Snapshot};
use crate::tr;
use crate::ui::kit::Segmented;
use crate::ui::theme::{palette, space};

pub struct CapitalForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    pub kind: CapitalKind,
    /// The capital entry being edited, if any.
    editing: Option<i64>,
    pub amount: Entity<InputState>,
    date: Entity<DateField>,
    notes: Entity<InputState>,
    reason: Entity<InputState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl CapitalForm {
    pub fn new(
        app: WeakEntity<VzApp>,
        snap: Rc<Snapshot>,
        kind: CapitalKind,
        editing: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let existing = editing.and_then(|id| snap.capital_tx(id).cloned());
        let kind = existing.as_ref().map_or(kind, |c| c.kind);
        let amount = cx.new(|cx| {
            let s = InputState::new(window, cx).placeholder("0.00");
            match &existing {
                Some(c) => s.default_value(MoneyFormat::input_value(c.amount)),
                None => s,
            }
        });
        let notes = cx.new(|cx| {
            let s = InputState::new(window, cx).placeholder(match kind {
                CapitalKind::Contribution => t("Optional: source, e.g. savings account", "Opcional: origen, ej. cuenta de ahorros"),
                CapitalKind::Withdrawal => t("Optional: reason", "Opcional: motivo"),
            });
            match &existing {
                Some(c) => s.default_value(c.notes.clone()),
                None => s,
            }
        });
        let reason = reason_input(window, cx);
        let date = date_field(existing.as_ref().map_or(crate::utils::dates::today(), |c| c.occurred_on), window, cx);
        let mut subs = vec![cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify())];
        for input in [&amount, &notes, &reason] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self { app, snap, kind, editing, amount, date, notes, reason, error: None, show_errors: false, _subs: subs }
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let (Ok(amount), Some(date)) = (parse_money(&value(&self.amount, cx)), picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        let (kind, editing) = (self.kind, self.editing);
        let notes = value(&self.notes, cx);
        let reason = value(&self.reason, cx);
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            let r = match editing {
                Some(id) => app.store.update_capital(id, kind, amount, date, &notes, &reason, today),
                None => app.store.add_capital(kind, amount, date, &notes, today).map(|_| ()),
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
                        (Some(_), _) => t("Capital entry updated", "Movimiento de capital actualizado").to_string(),
                        (None, CapitalKind::Contribution) => {
                            tr!("{m} added to your lending capital", "{m} agregado a tu capital de préstamos")
                        }
                        (None, CapitalKind::Withdrawal) => {
                            tr!("{m} withdrawn from your lending capital", "{m} retirado de tu capital de préstamos")
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

impl Render for CapitalForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let money = self.snap.settings.money();
        let amount = parse_money(&value(&self.amount, cx));
        let picked = picked_date(&self.date, cx);
        let date = picked.unwrap_or(crate::utils::dates::today());
        let book = match self.editing {
            Some(id) => self.snap.book_without(Record::Capital(id)),
            None => self.snap.book.clone(),
        };
        let available = book.available_from(date, Some(Account::Capital)).max(0);
        let avail = money.format(available);
        let amount_err = match (&amount, self.kind) {
            (Err(e), _) if self.show_errors => Some(e.to_string()),
            (Ok(a), CapitalKind::Withdrawal) if *a > available => {
                Some(tr!("Only {avail} is available to withdraw", "Solo hay {avail} disponible para retirar"))
            }
            (Ok(0), _) if self.show_errors => Some(t("Enter an amount greater than zero", "Escribe un monto mayor que cero").into()),
            _ => None,
        };
        let hint = match self.kind {
            CapitalKind::Contribution => t(
                "Personal money set aside for lending. Not counted as profit.",
                "Dinero propio apartado para prestar. No cuenta como ganancia.",
            )
            .to_string(),
            CapitalKind::Withdrawal => tr!("Available to withdraw: {avail}", "Disponible para retirar: {avail}"),
        };
        let date_err = if picked.is_none() {
            Some(invalid_date())
        } else {
            (date > crate::utils::dates::today()).then(|| t("The date can't be in the future", "La fecha no puede ser futura").to_string())
        };
        let can_submit = matches!(amount, Ok(a) if a > 0) && amount_err.is_none() && date_err.is_none();
        let kinds = [CapitalKind::Contribution, CapitalKind::Withdrawal].map(|k| (k, SharedString::from(k.label()))).to_vec();
        let kind_control = Segmented::pick("capital-kind", kinds, self.kind, cx, |this: &mut Self, k, _, cx| {
            this.kind = k;
            cx.notify();
        });
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .when(self.editing.is_some(), |d| d.child(field(&p, t("Type", "Tipo"), kind_control, None, None)))
            .child(
                field_row()
                    .child(grow_field(field(
                        &p,
                        t("Amount", "Monto"),
                        Input::new(&self.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                        Some(hint),
                        amount_err,
                    )))
                    .child(div().w(px(DATE_WIDTH)).flex_none().child(field(&p, t("Date", "Fecha"), self.date.clone(), None, date_err))),
            )
            .child(field(&p, t("Note", "Nota"), Input::new(&self.notes), None, None))
            .when(self.editing.is_some(), |d| {
                d.child(field(&p, t("Reason for the change", "Motivo del cambio"), Input::new(&self.reason), None, None))
            })
            .child(footer(
                &p,
                match (self.editing, self.kind) {
                    (Some(_), _) => t("Save changes", "Guardar cambios"),
                    (None, CapitalKind::Contribution) => t("Add capital", "Agregar capital"),
                    (None, CapitalKind::Withdrawal) => t("Withdraw", "Retirar"),
                },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
