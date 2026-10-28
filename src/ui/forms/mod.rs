//! Dialog forms. Each form is its own view so previews update as you type;
//! persistent input state is created once when the dialog opens.

pub mod adjust;
pub mod capital;
pub mod client;
pub mod date_field;
pub mod entry;
pub mod loan;
pub mod payment;
pub mod reason;
pub mod receipt_text;
pub mod reset;
pub mod schedule;

use std::cell::Cell;

use chrono::NaiveDate;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::t;
use crate::ui::kit::Button;
use crate::ui::theme::{Palette, RADIUS_CARD, RADIUS_CONTROL, Typeset as _, space, text};
use date_field::DateField;

/// Width of a date field beside another field.
pub const DATE_WIDTH: f32 = 200.0;
/// Narrowest a growing field gets before its row wraps.
pub const FIELD_MIN: f32 = 160.0;
/// Room a dialog keeps around itself in the window, and inside around its
/// body, on each side (GPUI Component's dialog: 16px margin, 16px padding).
const DIALOG_INSET: f32 = 2.0 * space::L;

/// Label above a control, with an optional hint and inline error.
pub fn field(p: &Palette, label: &str, control: impl IntoElement, hint: Option<String>, error: Option<String>) -> Div {
    v_flex()
        .gap(px(6.))
        .min_w_0()
        .child(div().typeset(text::LABEL).text_color(p.text).child(label.to_string()))
        .child(control)
        .when_some(error.clone(), |d, e| d.child(div().typeset(text::CAPTION).text_color(p.negative).child(e)))
        .when(error.is_none(), |d| d.when_some(hint, |d, h| d.child(div().typeset(text::CAPTION).text_color(p.muted).child(h))))
}

/// A row of fields side by side that wraps to the next line rather than
/// squeezing a field under [`FIELD_MIN`]. Give growing fields [`grow_field`].
pub fn field_row() -> Div {
    h_flex().flex_wrap().items_start().gap(px(space::M))
}

/// A field in a [`field_row`] that takes the room left over.
pub fn grow_field(content: impl IntoElement) -> Div {
    div().flex_1().min_w(px(FIELD_MIN)).child(content)
}

/// The widest a dialog body can be in this window. Wide forms stack their
/// columns when this is less than they need.
pub fn body_room(window: &Window) -> f32 {
    f32::from(window.viewport_size().width) - 2.0 * DIALOG_INSET
}

pub fn value(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

pub fn picked_date(state: &Entity<DateField>, cx: &App) -> Option<NaiveDate> {
    state.read(cx).date(cx)
}

pub fn date_field(date: NaiveDate, window: &mut Window, cx: &mut App) -> Entity<DateField> {
    cx.new(|cx| DateField::new(date, window, cx))
}

/// Cancel, then `primary`, at the right.
fn actions(primary: Button) -> Div {
    h_flex()
        .justify_end()
        .gap(px(space::S))
        .child(Button::new("cancel").label(t("Cancel", "Cancelar")).on_click(|_, window, cx| window.close_dialog(cx)))
        .child(primary)
}

/// Footer with Cancel and the primary action.
pub fn footer(
    p: &Palette,
    primary_label: &str,
    enabled: bool,
    busy: bool,
    on_primary: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    footer_with(p, div(), primary_label, enabled, busy, on_primary)
}

/// `footer` with a secondary action on the left, such as "Restore".
pub fn footer_with(
    p: &Palette,
    left: impl IntoElement,
    primary_label: &str,
    enabled: bool,
    busy: bool,
    on_primary: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    h_flex()
        .justify_between()
        .items_center()
        .gap(px(space::S))
        .pt(px(space::XL))
        .mt(px(space::S))
        .border_t_1()
        .border_color(p.rule)
        .child(left)
        .child(actions(
            Button::new("submit").primary().label(primary_label.to_string()).loading(busy).disabled(!enabled).on_click(on_primary),
        ))
}

/// Footer of a confirmation that destroys or removes something: Cancel and
/// a danger button.
pub fn danger_footer(
    id: &'static str,
    label: &str,
    enabled: bool,
    on_confirm: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    actions(Button::new(id).danger().label(label.to_string()).disabled(!enabled).on_click(on_confirm)).pt(px(space::S))
}

pub fn error_banner(p: &Palette, error: &Option<String>) -> Option<Div> {
    error.as_ref().map(|e| {
        div()
            .px(px(space::M))
            .py(px(space::S))
            .rounded(px(RADIUS_CONTROL))
            .bg(p.negative_tint)
            .typeset(text::BODY)
            .text_color(p.negative)
            .child(e.clone())
    })
}

/// An inset panel inside a dialog for previews (the schedule, how a
/// payment is split). Not a card: it sits flat in the dialog.
pub fn panel(p: &Palette) -> Div {
    v_flex().p(px(space::L)).rounded(px(RADIUS_CARD)).bg(p.raised).border_1().border_color(p.rule)
}

/// Focus `handle` the first time the dialog builder runs, never after.
pub fn focus_once(guard: &Cell<bool>, handle: &FocusHandle, window: &mut Window, cx: &mut App) {
    if guard.replace(false) {
        handle.focus(window, cx);
    }
}

pub fn invalid_date() -> String {
    t("Enter a valid date", "Escribe una fecha válida").to_string()
}

/// Optional "why" for an edit, kept in the change history.
pub fn reason_input<T: 'static>(window: &mut Window, cx: &mut Context<T>) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(t("Optional: why it changed", "Opcional: por qué cambió")))
}

/// A selectable tile with a title and an optional line under it, for
/// choices that need a word of explanation (a plain choice is a
/// [`segmented`] control). The caller adds the click handler.
pub fn choice_tile(p: &Palette, id: &'static str, title: &str, sub: Option<String>, selected: bool) -> Stateful<Div> {
    v_flex()
        .id(id)
        .flex_1()
        .min_w_0()
        .p(px(space::M))
        .gap(px(2.))
        .rounded(px(RADIUS_CONTROL))
        .border_1()
        .border_color(if selected { p.accent } else { p.border })
        .when(selected, |d| d.bg(p.accent_tint))
        .when(!selected, |d| d.hover(|s| s.bg(p.hover)))
        .cursor_pointer()
        .child(div().typeset(text::BODY_STRONG).text_color(if selected { p.accent_text } else { p.text }).child(title.to_string()))
        .children(sub.map(|s| div().typeset(text::CAPTION).text_color(p.muted).child(s)))
}

/// Test support: the app in a GPUI test window over `store`.
#[cfg(test)]
pub(crate) fn test_app(cx: &mut TestAppContext, store: crate::services::Store) -> (Entity<crate::app::VzApp>, &mut VisualTestContext) {
    use std::cell::RefCell;
    use std::rc::Rc;
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::ui::theme::install(Default::default(), cx);
        cx.set_reduce_motion(true);
    });
    let slot = Rc::new(RefCell::new(None));
    let keep = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let app = cx.new(|cx| crate::app::VzApp::new(store, window, cx));
        *keep.borrow_mut() = Some(app.clone());
        gpui_kit::component::Root::new(app, window, cx)
    });
    let app = slot.borrow_mut().take().expect("the window builds the app");
    (app, cx)
}
