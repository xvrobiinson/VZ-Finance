//! Correcting the portfolio total: type what each account really holds and
//! an adjustment is recorded for every difference.

use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{DATE_WIDTH, date_field, error_banner, field, field_row, footer, grow_field, invalid_date, picked_date, value};
use crate::app::VzApp;
use crate::domain::money::{Cents, MoneyFormat, parse_money};
use crate::domain::portfolio::Account;
use crate::i18n::t;
use crate::services::Snapshot;
use crate::tr;
use crate::ui::theme::{Typeset as _, palette, space, text};

pub struct AdjustForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    pub capital: Entity<InputState>,
    interest: Entity<InputState>,
    date: Entity<DateField>,
    reason: Entity<InputState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl AdjustForm {
    pub fn new(app: WeakEntity<VzApp>, snap: Rc<Snapshot>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = crate::utils::dates::today();
        let now = snap.book.balances_on(today);
        let capital = cx.new(|cx| InputState::new(window, cx).default_value(MoneyFormat::input_value(now.capital.max(0))));
        let interest = cx.new(|cx| InputState::new(window, cx).default_value(MoneyFormat::input_value(now.interest.max(0))));
        let reason =
            cx.new(|cx| InputState::new(window, cx).placeholder(t("e.g. Counted the cash on hand", "ej. Conté el efectivo disponible")));
        let date = date_field(today, window, cx);
        let mut subs = vec![cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify())];
        for input in [&capital, &interest, &reason] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self { app, snap, capital, interest, date, reason, error: None, show_errors: false, _subs: subs }
    }

    fn targets(&self, cx: &App) -> (Result<Cents, String>, Result<Cents, String>) {
        let parse = |s: &Entity<InputState>| parse_money(&value(s, cx)).map_err(|e| e.to_string());
        (parse(&self.capital), parse(&self.interest))
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let ((Ok(capital), Ok(interest)), Some(date)) = (self.targets(cx), picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        let reason = value(&self.reason, cx);
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            match app.store.adjust_balances(capital, interest, date, &reason, today) {
                Ok(_) => {
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
                    app.notify_success(t("Balances adjusted", "Saldos ajustados"), window, cx);
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

impl Render for AdjustForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let money = self.snap.settings.money();
        let today = crate::utils::dates::today();
        let picked = picked_date(&self.date, cx);
        let recorded = self.snap.book.balances_on(picked.unwrap_or(today));
        let (cap, int) = self.targets(cx);
        let line = |account: Account, target: &Result<Cents, String>| -> (Option<String>, Option<String>) {
            let was = money.format(recorded.get(account));
            match target {
                Ok(v) => {
                    let diff = v - recorded.get(account);
                    let change = if diff == 0 { t("no change", "sin cambio").to_string() } else { money.format_signed(diff) };
                    (Some(tr!("Recorded: {was} · {change}", "Registrado: {was} · {change}")), None)
                }
                Err(e) => (Some(tr!("Recorded: {was}", "Registrado: {was}")), self.show_errors.then(|| e.clone())),
            }
        };
        let (cap_hint, cap_err) = line(Account::Capital, &cap);
        let (int_hint, int_err) = line(Account::Interest, &int);
        let date_err = match picked {
            None => Some(invalid_date()),
            Some(d) if d > today => Some(t("The date can't be in the future", "La fecha no puede ser futura").to_string()),
            _ => None,
        };
        let reason = value(&self.reason, cx);
        let reason_err = (self.show_errors && reason.trim().is_empty()).then(|| t("Give a reason", "Indica un motivo").to_string());
        let changed = matches!((&cap, &int), (Ok(c), Ok(i)) if *c != recorded.capital || *i != recorded.interest);
        let can_submit = cap.is_ok() && int.is_ok() && date_err.is_none() && changed && !reason.trim().is_empty();
        let money_input = |s: &Entity<InputState>| Input::new(s).prefix(div().text_color(p.muted).child(money.symbol.clone()));
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(div().typeset(text::BODY).text_color(p.muted).child(t(
                "Type what you really have in each account. An adjustment is recorded for each difference, so the history still adds up. Loans and payments are not changed; edit those directly.",
                "Escribe lo que realmente tienes en cada cuenta. Se registra un ajuste por cada diferencia, para que el historial cuadre. Los préstamos y pagos no cambian; edítalos directamente.",
            )))
            .child(
                field_row()
                    .child(grow_field(field(&p, Account::Capital.label(), money_input(&self.capital), cap_hint, cap_err)))
                    .child(grow_field(field(&p, Account::Interest.label(), money_input(&self.interest), int_hint, int_err))),
            )
            .child(div().w(px(DATE_WIDTH)).child(field(&p, t("As of", "Al día"), self.date.clone(), None, date_err)))
            .child(field(&p, t("Reason", "Motivo"), Input::new(&self.reason), None, reason_err))
            .child(footer(
                &p,
                t("Save adjustment", "Guardar ajuste"),
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
