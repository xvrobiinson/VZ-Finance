use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use super::{error_banner, field, footer, value};
use crate::app::{Route, VzApp};
use crate::database::repositories::clients::{Client, ClientInput};
use crate::ui::theme::{palette, space};

pub struct ClientForm {
    app: WeakEntity<VzApp>,
    editing: Option<i64>,
    pub name: Entity<InputState>,
    phone: Entity<InputState>,
    email: Entity<InputState>,
    notes: Entity<TextareaState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

impl ClientForm {
    pub fn new(app: WeakEntity<VzApp>, existing: Option<Client>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let e = existing.as_ref();
        let name = cx.new(|cx| {
            InputState::new(window, cx).placeholder("e.g. Marta Quiroga").default_value(e.map(|c| c.full_name.clone()).unwrap_or_default())
        });
        let phone = cx.new(|cx| InputState::new(window, cx).placeholder("Optional").default_value(e.and_then(|c| c.phone.clone()).unwrap_or_default()));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("Optional").default_value(e.and_then(|c| c.email.clone()).unwrap_or_default()));
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder("Anything worth remembering: referral, collateral, preferred contact time")
                .default_value(e.map(|c| c.notes.clone()).unwrap_or_default())
        });
        let mut subs = Vec::new();
        for input in [&name, &phone, &email] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self { app, editing: e.map(|c| c.id), name, phone, email, notes, error: None, show_errors: false, _subs: subs }
    }

    fn input(&self, cx: &App) -> ClientInput {
        ClientInput {
            full_name: value(&self.name, cx),
            phone: value(&self.phone, cx),
            email: value(&self.email, cx),
            notes: self.notes.read(cx).value().to_string(),
        }
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let input = self.input(cx);
        if input.full_name.trim().is_empty() {
            cx.notify();
            return;
        }
        let editing = self.editing;
        let result = self.app.update(cx, |app, cx| {
            let r = match editing {
                Some(id) => app.store.update_client(id, &input).map(|_| id),
                None => app.store.create_client(&input),
            };
            match r {
                Ok(id) => {
                    app.reload(cx);
                    Ok(id)
                }
                Err(e) => Err(app.error_message(&e)),
            }
        });
        match result {
            Ok(Ok(id)) => {
                window.close_dialog(cx);
                let name = input.full_name.trim().to_string();
                let _ = self.app.update(cx, |app, cx| {
                    app.notify_success(if editing.is_some() { format!("Saved {name}") } else { format!("Added {name}") }, window, cx);
                    if editing.is_none() {
                        app.navigate(Route::Client(id), window, cx);
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

impl Render for ClientForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let input = self.input(cx);
        let name_error = (self.show_errors && input.full_name.trim().is_empty()).then(|| "Enter the client's full name".to_string());
        let email = input.email.trim();
        let email_error = (!email.is_empty() && self.show_errors && !email.contains('@')).then(|| "Enter a valid email address".to_string());
        v_flex()
            .gap(px(space::L))
            .children(error_banner(&p, &self.error))
            .child(field(&p, "Full name", Input::new(&self.name), None, name_error))
            .child(
                h_flex()
                    .gap(px(space::M))
                    .child(div().flex_1().child(field(&p, "Phone", Input::new(&self.phone), None, None)))
                    .child(div().flex_1().child(field(&p, "Email", Input::new(&self.email), None, email_error))),
            )
            .child(field(&p, "Notes", Textarea::new(&self.notes), None, None))
            .child(footer(
                &p,
                if self.editing.is_some() { "Save changes" } else { "Add client" },
                true,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ))
    }
}
