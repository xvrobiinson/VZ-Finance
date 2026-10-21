//! Dialog forms. Each form is its own view so previews update as you type;
//! persistent input state is created once when the dialog opens.

pub mod capital;
pub mod client;
pub mod loan;
pub mod payment;
pub mod reason;

use std::cell::Cell;

use chrono::NaiveDate;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::calendar::Date;
use gpui_kit::component::date_picker::DatePickerState;
use gpui_kit::component::input::InputState;
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui::theme::{Palette, size, space};

/// Label above a control, with an optional hint and inline error.
pub fn field(p: &Palette, label: &str, control: impl IntoElement, hint: Option<String>, error: Option<String>) -> Div {
    v_flex()
        .gap(px(6.))
        .min_w_0()
        .child(div().text_size(px(size::META + 1.)).font_weight(FontWeight::MEDIUM).text_color(p.text).child(label.to_string()))
        .child(control)
        .when_some(error.clone(), |d, e| d.child(div().text_size(px(size::META)).text_color(p.negative).child(e)))
        .when(error.is_none(), |d| d.when_some(hint, |d, h| d.child(div().text_size(px(size::META)).text_color(p.muted).child(h))))
}

pub fn value(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

pub fn picked_date(state: &Entity<DatePickerState>, cx: &App) -> Option<NaiveDate> {
    match state.read(cx).date() {
        Date::Single(d) => d,
        other => other.start(),
    }
}

pub fn date_picker(date: NaiveDate, window: &mut Window, cx: &mut App) -> Entity<DatePickerState> {
    cx.new(|cx| {
        let mut s = DatePickerState::new(window, cx).date_format("%b %-d, %Y");
        s.set_date(date, window, cx);
        s
    })
}

/// Footer with Cancel and the primary action.
pub fn footer(p: &Palette, primary_label: &str, enabled: bool, busy: bool, on_primary: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Div {
    h_flex()
        .justify_end()
        .gap(px(space::S))
        .pt(px(space::XL))
        .mt(px(space::S))
        .border_t_1()
        .border_color(p.rule)
        .child(Button::new("cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
        .child(
            Button::new("submit")
                .primary()
                .label(primary_label.to_string())
                .loading(busy)
                .disabled(!enabled || busy)
                .on_click(on_primary),
        )
}

pub fn error_banner(p: &Palette, error: &Option<String>) -> Option<Div> {
    error.as_ref().map(|e| {
        div()
            .px(px(space::M))
            .py(px(10.))
            .rounded(px(8.))
            .bg(p.negative_tint)
            .text_color(p.negative)
            .child(e.clone())
    })
}

/// Focus `handle` the first time the dialog builder runs, never after.
pub fn focus_once(guard: &Cell<bool>, handle: &FocusHandle, window: &mut Window, cx: &mut App) {
    if guard.replace(false) {
        handle.focus(window, cx);
    }
}
