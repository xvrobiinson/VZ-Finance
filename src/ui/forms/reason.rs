//! Small confirmation forms that take a reason (and sometimes a date):
//! deleting a record and marking a loan as defaulted.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{danger_footer, date_field, error_banner, field, picked_date, value};
use crate::app::{Route, VzApp};
use crate::i18n::t;
use crate::ui::theme::{Typeset as _, palette, space, text};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReasonKind {
    MarkDefaulted(i64),
    DeleteLoan(i64),
    DeletePayment(i64),
    DeleteCapital(i64),
    DeleteEntry(i64),
    DeleteClient(i64),
    /// Loan id and installment number.
    WaiveFee(i64, u32),
}

impl ReasonKind {
    pub fn title(self) -> &'static str {
        match self {
            ReasonKind::MarkDefaulted(_) => t("Mark loan as defaulted", "Marcar préstamo como incobrable"),
            ReasonKind::DeleteLoan(_) => t("Delete loan", "Eliminar préstamo"),
            ReasonKind::DeletePayment(_) => t("Delete payment", "Eliminar pago"),
            ReasonKind::DeleteCapital(_) => t("Delete capital entry", "Eliminar movimiento de capital"),
            ReasonKind::DeleteEntry(_) => t("Delete entry", "Eliminar movimiento"),
            ReasonKind::DeleteClient(_) => t("Delete client", "Eliminar cliente"),
            ReasonKind::WaiveFee(..) => t("Waive late fee", "Perdonar cargo por mora"),
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            ReasonKind::MarkDefaulted(_) => t(
                "No new interest is charged after this date and the balance is left out of portfolio value. You can reinstate the loan later.",
                "No se cobra más interés después de esta fecha y el saldo queda fuera del valor de la cartera. Puedes reactivar el préstamo luego.",
            ),
            ReasonKind::DeleteLoan(_) => t(
                "The loan and all of its payments are removed, and every figure is worked out again without them. What it was is kept in the change history.",
                "Se eliminan el préstamo y todos sus pagos, y las cifras se recalculan sin ellos. Lo que era queda en el historial de cambios.",
            ),
            ReasonKind::DeletePayment(_) => t(
                "The payment is removed and the loan's other payments are split again between interest and capital. What it was is kept in the change history.",
                "Se elimina el pago y los demás pagos del préstamo se vuelven a dividir entre interés y capital. Lo que era queda en el historial de cambios.",
            ),
            ReasonKind::DeleteCapital(_) | ReasonKind::DeleteEntry(_) => t(
                "The movement is removed and your balances are worked out again without it. What it was is kept in the change history.",
                "Se elimina el movimiento y tus saldos se recalculan sin él. Lo que era queda en el historial de cambios.",
            ),
            ReasonKind::DeleteClient(_) => t(
                "The client is removed. What their details were is kept in the change history.",
                "Se elimina el cliente. Sus datos quedan en el historial de cambios.",
            ),
            ReasonKind::WaiveFee(..) => t(
                "The late fee no longer counts and the loan's payments are worked out again. It stays in the list, marked as waived, and can be restored.",
                "El cargo por mora deja de contar y los pagos del préstamo se recalculan. Queda en la lista marcado como perdonado y se puede restaurar.",
            ),
        }
    }

    fn confirm_label(self) -> &'static str {
        match self {
            ReasonKind::MarkDefaulted(_) => t("Mark as defaulted", "Marcar incobrable"),
            ReasonKind::WaiveFee(..) => t("Waive", "Perdonar"),
            _ => t("Delete", "Eliminar"),
        }
    }

    fn done_message(self) -> &'static str {
        match self {
            ReasonKind::MarkDefaulted(_) => t(
                "Loan marked as defaulted. Its balance is excluded from portfolio value.",
                "Préstamo marcado como incobrable. Su saldo queda fuera del valor de la cartera.",
            ),
            ReasonKind::DeleteLoan(_) => t("Loan deleted", "Préstamo eliminado"),
            ReasonKind::DeletePayment(_) => {
                t("Payment deleted. The loan's payments were split again.", "Pago eliminado. Los pagos del préstamo se recalcularon.")
            }
            ReasonKind::DeleteCapital(_) | ReasonKind::DeleteEntry(_) => t("Movement deleted", "Movimiento eliminado"),
            ReasonKind::DeleteClient(_) => t("Client deleted", "Cliente eliminado"),
            ReasonKind::WaiveFee(..) => t("Late fee waived", "Cargo por mora perdonado"),
        }
    }
}

pub struct ReasonForm {
    app: WeakEntity<VzApp>,
    kind: ReasonKind,
    pub reason: Entity<InputState>,
    date: Entity<DateField>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl ReasonForm {
    pub fn new(app: WeakEntity<VzApp>, kind: ReasonKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reason = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match kind {
                ReasonKind::MarkDefaulted(_) => t("Optional: what happened", "Opcional: qué pasó"),
                _ => t("Optional: e.g. entered twice, wrong client", "Opcional: ej. registrado dos veces, cliente equivocado"),
            })
        });
        let date = date_field(crate::utils::dates::today(), window, cx);
        let subs = vec![
            cx.subscribe_in(&reason, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }),
            cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify()),
        ];
        Self { app, kind, reason, date, error: None, _subs: subs }
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reason = value(&self.reason, cx);
        let date = picked_date(&self.date, cx);
        let kind = self.kind;
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            let r = match kind {
                ReasonKind::MarkDefaulted(id) => app.store.mark_defaulted(id, date.unwrap_or(today), &reason, today),
                ReasonKind::DeleteLoan(id) => app.store.delete_loan(id, &reason),
                ReasonKind::DeletePayment(id) => app.store.delete_payment(id, &reason, today),
                ReasonKind::DeleteCapital(id) => app.store.delete_capital(id, &reason),
                ReasonKind::DeleteEntry(id) => app.store.delete_entry(id, &reason),
                ReasonKind::DeleteClient(id) => app.store.delete_client(id, &reason),
                ReasonKind::WaiveFee(loan, seq) => app.store.set_late_fee_waived(loan, seq, true, &reason, today),
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
                    app.notify_success(kind.done_message(), window, cx);
                    match kind {
                        ReasonKind::DeleteLoan(_) => app.navigate(Route::Loans, window, cx),
                        ReasonKind::DeleteClient(_) => app.navigate(Route::Clients, window, cx),
                        _ => {}
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

impl Render for ReasonForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let defaulting = matches!(self.kind, ReasonKind::MarkDefaulted(_));
        let can = !defaulting || picked_date(&self.date, cx).is_some();
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(div().typeset(text::BODY).text_color(p.muted).child(self.kind.explanation()))
            .when(defaulting, |d| d.child(field(&p, t("Date", "Fecha"), self.date.clone(), None, None)))
            .child(field(&p, t("Reason", "Motivo"), Input::new(&self.reason), None, None))
            .child(danger_footer("confirm", self.kind.confirm_label(), can, cx.listener(|this, _, w, cx| this.submit(w, cx))))
    }
}
