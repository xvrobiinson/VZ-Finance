//! Small confirmation forms that need a reason or a date: reversing a
//! payment and marking a loan as defaulted.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{date_picker, error_banner, field, picked_date, value};
use crate::app::VzApp;
use crate::ui::theme::{palette, space};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReasonKind {
    ReversePayment(i64),
    MarkDefaulted(i64),
}

pub struct ReasonForm {
    app: WeakEntity<VzApp>,
    kind: ReasonKind,
    pub reason: Entity<InputState>,
    date: Entity<DatePickerState>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl ReasonForm {
    pub fn new(app: WeakEntity<VzApp>, kind: ReasonKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let reason = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match kind {
                ReasonKind::ReversePayment(_) => "e.g. Entered twice, wrong amount",
                ReasonKind::MarkDefaulted(_) => "Optional: what happened",
            })
        });
        let date = date_picker(crate::utils::dates::today(), window, cx);
        let subs = vec![
            cx.subscribe_in(&reason, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }),
            cx.subscribe_in(&date, window, |_, _, _: &DatePickerEvent, _, cx| cx.notify()),
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
                ReasonKind::ReversePayment(id) => app.store.reverse_payment(id, &reason, today),
                ReasonKind::MarkDefaulted(id) => app.store.mark_defaulted(id, date.unwrap_or(today), &reason, today),
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
                    app.notify_success(
                        match kind {
                            ReasonKind::ReversePayment(_) => "Payment reversed. The original stays on record.",
                            ReasonKind::MarkDefaulted(_) => "Loan marked as defaulted. Its balance is excluded from portfolio value.",
                        },
                        window,
                        cx,
                    );
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
        let reason = value(&self.reason, cx);
        let (explain, label, can) = match self.kind {
            ReasonKind::ReversePayment(_) => (
                "The payment stays in the history, marked as reversed, and no longer counts toward balances, interest or cash.",
                "Reverse payment",
                !reason.trim().is_empty(),
            ),
            ReasonKind::MarkDefaulted(_) => (
                "Interest stops accruing on the default date and the remaining balance is excluded from portfolio value. You can reinstate the loan later.",
                "Mark as defaulted",
                picked_date(&self.date, cx).is_some(),
            ),
        };
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(div().text_color(p.muted).child(explain))
            .when(matches!(self.kind, ReasonKind::MarkDefaulted(_)), |d| d.child(field(&p, "Default date", DatePicker::new(&self.date), None, None)))
            .child(field(&p, "Reason", Input::new(&self.reason), None, None))
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(space::S))
                    .pt(px(space::S))
                    .child(Button::new("cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(Button::new("confirm").danger().label(label).disabled(!can).on_click(cx.listener(|this, _, w, cx| this.submit(w, cx)))),
            )
    }
}
