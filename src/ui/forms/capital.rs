use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{date_field, error_banner, field, footer, invalid_date, picked_date, value};
use crate::app::VzApp;
use crate::domain::money::parse_money;
use crate::domain::portfolio::CapitalKind;
use crate::i18n::t;
use crate::services::Snapshot;
use crate::tr;
use crate::ui::theme::{palette, space};

pub struct CapitalForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    pub kind: CapitalKind,
    pub amount: Entity<InputState>,
    date: Entity<DateField>,
    notes: Entity<InputState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl CapitalForm {
    pub fn new(app: WeakEntity<VzApp>, snap: Rc<Snapshot>, kind: CapitalKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let amount = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let notes = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match kind {
                CapitalKind::Contribution => t("Optional: source, e.g. savings account", "Opcional: origen, ej. cuenta de ahorros"),
                CapitalKind::Withdrawal => t("Optional: reason", "Opcional: motivo"),
            })
        });
        let date = date_field(crate::utils::dates::today(), window, cx);
        let mut subs = vec![cx.subscribe(&date, |_, _, _: &DateChanged, cx| cx.notify())];
        for input in [&amount, &notes] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self { app, snap, kind, amount, date, notes, error: None, show_errors: false, _subs: subs }
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let (Ok(amount), Some(date)) = (parse_money(&value(&self.amount, cx)), picked_date(&self.date, cx)) else {
            cx.notify();
            return;
        };
        let kind = self.kind;
        let notes = value(&self.notes, cx);
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            match app.store.add_capital(kind, amount, date, &notes, today) {
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
                    let m = app.settings().money().format(amount);
                    let msg = match kind {
                        CapitalKind::Contribution => tr!("{m} added to your lending capital", "{m} agregado a tu capital de préstamos"),
                        CapitalKind::Withdrawal => {
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
        let avail = money.format(self.snap.book.min_cash_from(date).max(0));
        let available = self.snap.book.min_cash_from(date).max(0);
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
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(
                h_flex()
                    .items_start()
                    .gap(px(space::M))
                    .child(div().flex_1().child(field(
                        &p,
                        t("Amount", "Monto"),
                        Input::new(&self.amount).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                        Some(hint),
                        amount_err,
                    )))
                    .child(div().w(px(200.)).child(field(&p, t("Date", "Fecha"), self.date.clone(), None, date_err))),
            )
            .child(field(&p, t("Note", "Nota"), Input::new(&self.notes), None, None))
            .child(footer(
                &p,
                match self.kind {
                    CapitalKind::Contribution => t("Add capital", "Agregar capital"),
                    CapitalKind::Withdrawal => t("Withdraw", "Retirar"),
                },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
