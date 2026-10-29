//! Editing the wording printed on receipts: the "How payments work" heading
//! and lines for each loan type (add, remove, reorder), the captions under
//! the signature lines and the closing line of each receipt. Opens with the
//! wording in use; text left as built in stays built in, so it follows the
//! language setting.

use gpui_kit::assets::IconName;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, button::ButtonVariant, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{error_banner, field, footer_with, value};
use crate::app::VzApp;
use crate::i18n::t;
use crate::services::receipt_text::{self, ReceiptText, RuleLine, RuleSet, When, tags};
use crate::services::settings::Settings;
use crate::tr;
use crate::ui::kit::{Button, ButtonSize, Segmented};
use crate::ui::theme::{RADIUS_CONTROL, Typeset as _, palette, space, text};

struct Line {
    when: When,
    input: Entity<InputState>,
}

pub struct ReceiptTextForm {
    app: WeakEntity<VzApp>,
    /// The loan type whose lines are shown.
    set: RuleSet,
    lines: [Vec<Line>; 3],
    pub heading: Entity<InputState>,
    client_sign: Entity<InputState>,
    lender_sign: Entity<InputState>,
    payment_footer: Entity<InputState>,
    loan_footer: Entity<InputState>,
    error: Option<String>,
    saved: bool,
    _subs: Vec<Subscription>,
}

fn index(set: RuleSet) -> usize {
    RuleSet::ALL.iter().position(|s| *s == set).unwrap_or(0)
}

impl ReceiptTextForm {
    pub fn new(app: WeakEntity<VzApp>, settings: &Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = &settings.receipt_text;
        let mut subs = Vec::new();
        let mut input = |v: String, window: &mut Window, cx: &mut Context<Self>| {
            let state = cx.new(|cx| InputState::new(window, cx).default_value(v));
            subs.push(cx.subscribe_in(&state, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
            state
        };
        let heading = input(text.heading(), window, cx);
        let client_sign = input(text.client_sign(), window, cx);
        let lender_sign = input(text.lender_sign(), window, cx);
        let payment_footer = input(text.payment_footer(), window, cx);
        let loan_footer = input(text.loan_footer(), window, cx);
        let lines = RuleSet::ALL
            .map(|set| text.rules(set).into_iter().map(|l| Line { when: l.when, input: input(l.text, window, cx) }).collect::<Vec<_>>());
        Self {
            app,
            set: RuleSet::Declining,
            lines,
            heading,
            client_sign,
            lender_sign,
            payment_footer,
            loan_footer,
            error: None,
            saved: false,
            _subs: subs,
        }
    }

    fn add_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(t("New line", "Línea nueva")));
        self._subs.push(cx.subscribe_in(&state, window, |_, _, ev: &InputEvent, _, cx| {
            if let InputEvent::Change = ev {
                cx.notify()
            }
        }));
        state.update(cx, |s, cx| s.focus(window, cx));
        self.lines[index(self.set)].push(Line { when: When::Always, input: state });
        cx.notify();
    }

    fn move_line(&mut self, i: usize, up: bool, cx: &mut Context<Self>) {
        let lines = &mut self.lines[index(self.set)];
        let j = if up { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < lines.len()) };
        if let Some(j) = j {
            lines.swap(i, j);
            cx.notify();
        }
    }

    fn remove_line(&mut self, i: usize, cx: &mut Context<Self>) {
        let lines = &mut self.lines[index(self.set)];
        if i < lines.len() {
            lines.remove(i);
            cx.notify();
        }
    }

    /// The wording as typed. Text equal to the built-in wording is stored as
    /// built in, so it keeps following the language.
    fn wording(&self, cx: &App) -> ReceiptText {
        let custom = |state: &Entity<InputState>, default: String| {
            let v = value(state, cx).trim().to_string();
            (v != default).then_some(v)
        };
        let mut text = ReceiptText {
            heading: custom(&self.heading, receipt_text::default_heading()),
            client_sign: custom(&self.client_sign, receipt_text::default_client_sign()),
            lender_sign: custom(&self.lender_sign, receipt_text::default_lender_sign()),
            payment_footer: custom(&self.payment_footer, receipt_text::default_payment_footer()),
            loan_footer: custom(&self.loan_footer, receipt_text::default_loan_footer()),
            ..Default::default()
        };
        for set in RuleSet::ALL {
            // Empty lines are dropped; a list with none left prints no section.
            let lines: Vec<RuleLine> = self.lines[index(set)]
                .iter()
                .map(|l| RuleLine { text: value(&l.input, cx).trim().to_string(), when: l.when })
                .filter(|l| !l.text.is_empty())
                .collect();
            text.set_rules(set, (lines != receipt_text::default_rules(set)).then_some(lines));
        }
        text
    }

    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saved {
            return;
        }
        let text = self.wording(cx);
        if let Some(problem) = text.problem() {
            self.error = Some(problem.to_string());
            cx.notify();
            return;
        }
        self.save(text, t("Receipt wording saved", "Texto de los recibos guardado"), window, cx);
    }

    fn save(&mut self, text: ReceiptText, done: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.app.update(cx, |app, cx| {
            let mut s = app.settings().clone();
            s.receipt_text = text;
            match app.save_settings(s, window, cx) {
                Ok(()) => {
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
                let _ = self.app.update(cx, |app, cx| app.notify_success(done, window, cx));
            }
            Ok(Err(msg)) => {
                self.error = Some(msg);
                cx.notify();
            }
            Err(_) => window.close_dialog(cx),
        }
    }

    /// Asks first, then puts back every built-in line and caption.
    fn restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let form = form.clone();
            alert
                .title(t("Restore the original wording?", "¿Restaurar el texto original?"))
                .description(t(
                    "Every heading, line, caption and closing line you changed goes back to the wording receipts came with. Your changes are lost.",
                    "Cada título, línea, texto de firma y línea final que cambiaste vuelve al texto original de los recibos. Tus cambios se pierden.",
                ))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t("Restore", "Restaurar"))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t("Cancel", "Cancelar"))
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let _ = form.update(cx, |f, cx| {
                        f.save(ReceiptText::default(), t("Original wording restored", "Texto original restaurado"), window, cx)
                    });
                    true
                })
        });
    }
}

impl Render for ReceiptTextForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let i = index(self.set);
        let count = self.lines[i].len();
        let sets = RuleSet::ALL.map(|s| {
            let label = match s {
                RuleSet::Declining => t("Installments", "Cuotas"),
                RuleSet::Flat => t("Flat installments", "Cuotas fijas"),
                RuleSet::InterestOnly => t("Interest only", "Solo interés"),
            };
            (s, SharedString::from(label))
        });
        let picker = Segmented::pick("rule-set", sets, self.set, cx, |this: &mut Self, s, _, cx| {
            this.set = s;
            cx.notify();
        });
        let mini =
            |id: SharedString, icon: IconName, tip: &'static str| Button::new(id).ghost().size(ButtonSize::Mini).icon(icon).tooltip(tip);
        let rows = self.lines[i].iter().enumerate().map(|(n, line)| {
            h_flex()
                .gap(px(space::XS))
                .items_start()
                .child(div().w(px(18.)).pt(px(7.)).flex_none().typeset(text::CAPTION).text_color(p.faint).child(format!("{}", n + 1)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(Input::new(&line.input))
                        .children(line.when.hint().map(|h| div().typeset(text::CAPTION).text_color(p.muted).child(h))),
                )
                .child(
                    mini(format!("line-up-{n}").into(), IconName::ArrowUp, t("Move up", "Subir"))
                        .disabled(n == 0)
                        .on_click(cx.listener(move |this, _, _, cx| this.move_line(n, true, cx))),
                )
                .child(
                    mini(format!("line-down-{n}").into(), IconName::ArrowDown, t("Move down", "Bajar"))
                        .disabled(n + 1 == count)
                        .on_click(cx.listener(move |this, _, _, cx| this.move_line(n, false, cx))),
                )
                .child(
                    mini(format!("line-remove-{n}").into(), IconName::Trash, t("Remove line", "Quitar línea"))
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_line(n, cx))),
                )
        });
        let tag_help = tags().map(|(tag, what)| format!("{tag} {what}")).join(" · ");
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(div().typeset(text::BODY).text_color(p.muted).child(t(
                "This only changes the words on loan and payment receipts, never a loan's terms or numbers. Text you leave as it is stays in the language chosen in Settings.",
                "Esto solo cambia las palabras de los recibos, nunca los términos ni las cifras de un préstamo. El texto que dejes igual sigue el idioma elegido en Configuración.",
            )))
            .child(field(&p, t("Heading on loan receipts", "Título en los recibos de préstamo"), Input::new(&self.heading), None, None))
            .child(field(
                &p,
                t("Lines under it, for each loan type", "Líneas debajo, para cada tipo de préstamo"),
                v_flex()
                    .gap(px(space::S))
                    .child(picker)
                    .child(v_flex().gap(px(space::S)).children(rows).when(count == 0, |d| {
                        d.child(div().typeset(text::CAPTION).text_color(p.muted).child(t(
                            "No lines: receipts for this loan type print no “How payments work” section.",
                            "Sin líneas: los recibos de este tipo de préstamo no imprimen la sección “Cómo funcionan los pagos”.",
                        )))
                    }))
                    .child(
                        h_flex().child(
                            Button::new("add-line")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label(t("Add line", "Agregar línea"))
                                .disabled(count >= 20)
                                .on_click(cx.listener(|this, _, w, cx| this.add_line(w, cx))),
                        ),
                    )
                    .child(
                        div()
                            .p(px(space::S))
                            .rounded(px(RADIUS_CONTROL))
                            .bg(p.raised)
                            .typeset(text::CAPTION)
                            .text_color(p.muted)
                            .child(tr!("Filled in from each loan: {tag_help}", "Se llenan con cada préstamo: {tag_help}")),
                    ),
                None,
                None,
            ))
            .child(
                h_flex()
                    .gap(px(space::M))
                    .items_start()
                    .child(div().flex_1().min_w_0().child(field(
                        &p,
                        t("Client signature caption", "Texto bajo la firma del cliente"),
                        Input::new(&self.client_sign),
                        Some(t("The client's name follows when receipts print it", "Le sigue el nombre del cliente si se imprime").into()),
                        None,
                    )))
                    .child(div().flex_1().min_w_0().child(field(
                        &p,
                        t("Company signature caption", "Texto bajo la firma de la empresa"),
                        Input::new(&self.lender_sign),
                        None,
                        None,
                    ))),
            )
            .child(
                h_flex()
                    .gap(px(space::M))
                    .items_start()
                    .child(div().flex_1().min_w_0().child(field(
                        &p,
                        t("Closing line on payment receipts", "Línea final en los recibos de pago"),
                        Input::new(&self.payment_footer),
                        None,
                        None,
                    )))
                    .child(div().flex_1().min_w_0().child(field(
                        &p,
                        t("Closing line on loan receipts", "Línea final en los recibos de préstamo"),
                        Input::new(&self.loan_footer),
                        Some(t("Optional; printed under the signatures", "Opcional; se imprime bajo las firmas").into()),
                        None,
                    ))),
            )
            .child(footer_with(
                &p,
                Button::new("restore-wording")
                    .danger_ghost()
                    .label(t("Restore original wording", "Restaurar texto original"))
                    .on_click(cx.listener(|this, _, w, cx| this.restore_defaults(w, cx))),
                t("Save wording", "Guardar texto"),
                !self.saved,
                false,
                cx.listener(|this, _, w, cx| this.submit(w, cx)),
            ))
    }
}

#[cfg(test)]
mod tests {
    use ::gpui;
    use core::prelude::v1::test;

    use super::*;
    use crate::services::Store;
    use crate::ui::forms::test_app;

    fn open(cx: &mut TestAppContext) -> (Entity<VzApp>, Entity<ReceiptTextForm>, &mut VisualTestContext) {
        let (app, cx) = test_app(cx, Store::in_memory());
        let form = cx.update(|window, cx| {
            let settings = app.read(cx).settings().clone();
            let weak = app.downgrade();
            cx.new(|cx| ReceiptTextForm::new(weak, &settings, window, cx))
        });
        (app, form, cx)
    }

    fn saved(app: &Entity<VzApp>, cx: &mut VisualTestContext) -> ReceiptText {
        cx.update(|_, cx| app.read(cx).settings().receipt_text.clone())
    }

    #[gpui::test]
    fn saving_unchanged_wording_keeps_the_built_in_text(cx: &mut TestAppContext) {
        let (app, form, cx) = open(cx);
        cx.update(|window, cx| form.update(cx, |f, cx| f.submit(window, cx)));
        assert!(saved(&app, cx).is_default());
    }

    #[gpui::test]
    fn lines_can_be_reordered_removed_added_and_restored(cx: &mut TestAppContext) {
        let (app, form, cx) = open(cx);
        let defaults = receipt_text::default_rules(RuleSet::Declining);
        cx.update(|window, cx| {
            form.update(cx, |f, cx| {
                assert_eq!(f.lines[0].len(), defaults.len());
                f.move_line(0, false, cx); // first line goes second
                f.remove_line(4, cx); // "set by agreement"
                f.add_line(window, cx);
                let last = f.lines[0].last().unwrap().input.clone();
                last.update(cx, |s, cx| s.set_value("Bring your ID.", window, cx));
                f.heading.update(cx, |s, cx| s.set_value("Our terms", window, cx));
                f.submit(window, cx);
            })
        });
        let text = saved(&app, cx);
        let lines = text.rules(RuleSet::Declining);
        assert_eq!(lines[0], defaults[1]);
        assert_eq!(lines[1], defaults[0]);
        assert_eq!(lines.len(), defaults.len());
        assert_eq!(lines.last().unwrap(), &RuleLine { text: "Bring your ID.".into(), when: When::Always });
        assert_eq!(text.heading.as_deref(), Some("Our terms"));
        // The other loan types were not touched.
        assert_eq!(text.rules[1], None);
        assert_eq!(text.rules[2], None);

        // Restoring puts everything back.
        let form = cx.update(|window, cx| {
            let settings = app.read(cx).settings().clone();
            let weak = app.downgrade();
            cx.new(|cx| ReceiptTextForm::new(weak, &settings, window, cx))
        });
        cx.update(|window, cx| form.update(cx, |f, cx| f.save(ReceiptText::default(), "", window, cx)));
        assert!(saved(&app, cx).is_default());
    }
}
