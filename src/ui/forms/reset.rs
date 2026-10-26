//! Deleting all clients, loans and payments, after a typed confirmation and
//! with a copy saved first.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::{error_banner, field, value};
use crate::app::{Route, VzApp};
use crate::i18n::t;
use crate::tr;
use crate::ui::theme::{palette, space};

/// The word to type: RESET in English, BORRAR in Spanish.
fn word() -> &'static str {
    t("RESET", "BORRAR")
}

pub struct ResetForm {
    app: WeakEntity<VzApp>,
    pub confirm: Entity<InputState>,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl ResetForm {
    pub fn new(app: WeakEntity<VzApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let confirm = cx.new(|cx| InputState::new(window, cx).placeholder(word()));
        let subs = vec![cx.subscribe_in(&confirm, window, |_, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                cx.notify()
            }
        })];
        Self { app, confirm, error: None, _subs: subs }
    }

    fn confirmed(&self, cx: &App) -> bool {
        value(&self.confirm, cx).trim().eq_ignore_ascii_case(word())
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.confirmed(cx) {
            return;
        }
        let result = self.app.update(cx, |app, cx| match app.store.reset_all_data() {
            Ok(copy) => {
                app.reload(cx);
                Ok(copy)
            }
            Err(e) => {
                log::error!("reset failed: {e:#}");
                Err(format!("{e:#}"))
            }
        });
        match result {
            Ok(Ok(copy)) => {
                window.close_dialog(cx);
                let _ = self.app.update(cx, |app, cx| {
                    let saved = copy.display().to_string();
                    app.notify_success(
                        tr!(
                            "All data was deleted. A copy of it was saved first in {saved}",
                            "Se borraron todos los datos. Antes se guardó una copia en {saved}"
                        ),
                        window,
                        cx,
                    );
                    app.navigate(Route::Overview, window, cx);
                });
            }
            Ok(Err(msg)) => {
                self.error = Some(tr!("Nothing was deleted. {msg}", "No se borró nada. {msg}"));
                cx.notify();
            }
            Err(_) => window.close_dialog(cx),
        }
    }
}

impl Render for ResetForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let w = word();
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(div().text_color(p.muted).child(t(
                "Every client, loan, payment, capital entry, expense and the change history will be deleted. Settings, the receipt name and the logo are kept. A complete copy is saved in the backups folder first, so it can be restored.",
                "Se borrarán todos los clientes, préstamos, pagos, movimientos de capital, gastos y el historial de cambios. La configuración, el nombre de los recibos y el logo se conservan. Antes se guarda una copia completa en la carpeta de copias, para poder restaurarla.",
            )))
            .child(field(&p, &tr!("Type {w} to confirm", "Escribe {w} para confirmar"), Input::new(&self.confirm), None, None))
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(space::S))
                    .pt(px(space::S))
                    .child(Button::new("cancel").label(t("Cancel", "Cancelar")).on_click(|_, window, cx| window.close_dialog(cx)))
                    .child(
                        Button::new("reset")
                            .danger()
                            .label(t("Delete all data", "Borrar todo"))
                            .disabled(!self.confirmed(cx))
                            .on_click(cx.listener(|this, _, w, cx| this.submit(w, cx))),
                    ),
            )
    }
}
