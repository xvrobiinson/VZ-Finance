//! Screens render the snapshot held by `VzApp`; they never touch the store.

pub mod analytics;
pub mod clients;
pub mod loans;
pub mod overview;
pub mod payments;
pub mod settings;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{Route, VzApp};
use crate::domain::ledger::{DisplayStatus, Delinquency};
use crate::ui::components::{Tone, pill, status_tone};
use crate::ui::theme::{Palette, size};
use crate::utils::dates::days_between;

/// Status pill with the delinquency spelled out ("12 days overdue").
pub fn status_detail_pill(p: &Palette, status: DisplayStatus, q: &Delinquency, today: chrono::NaiveDate) -> Div {
    let text = match status {
        DisplayStatus::Overdue => {
            let d = q.days_overdue.max(1);
            format!("{d} day{} overdue", if d == 1 { "" } else { "s" })
        }
        DisplayStatus::DueSoon => match q.next_due.map(|n| days_between(today, n)) {
            Some(0) => "Due today".to_string(),
            Some(1) => "Due tomorrow".to_string(),
            Some(n) => format!("Due in {n} days"),
            None => "Due soon".to_string(),
        },
        s => s.label().to_string(),
    };
    pill(p, text, status_tone(status))
}

/// Segmented control. `options` are (key, label); returns the row.
pub fn segmented<K: Copy + PartialEq + 'static>(
    p: &Palette,
    id: &str,
    options: &[(K, &str)],
    selected: K,
    cx: &mut Context<VzApp>,
    on_select: impl Fn(&mut VzApp, K, &mut Window, &mut Context<VzApp>) + Clone + 'static,
) -> Div {
    h_flex().p(px(2.)).gap(px(2.)).rounded(px(8.)).bg(p.neutral_tint).children(options.iter().map(|(k, label)| {
        let k = *k;
        let is = k == selected;
        let on_select = on_select.clone();
        h_flex()
            .id(SharedString::from(format!("{id}-{label}")))
            .h(px(26.))
            .px(px(10.))
            .rounded(px(6.))
            .text_size(px(size::META + 0.5))
            .cursor_pointer()
            .when(is, |d| {
                d.bg(p.surface).text_color(p.text).font_weight(FontWeight::MEDIUM).shadow(vec![BoxShadow::new(px(0.), px(1.), p.shadow).blur_radius(px(3.))])
            })
            .when(!is, |d| d.text_color(p.muted).hover(|s| s.text_color(p.text)))
            .child(label.to_string())
            .on_click(cx.listener(move |this, _, w, cx| on_select(this, k, w, cx)))
    }))
}

/// "View all" style link in a card header.
pub fn header_link(p: &Palette, id: &str, text: &str, route: Route, cx: &mut Context<VzApp>) -> AnyElement {
    div()
        .id(SharedString::from(id.to_string()))
        .text_size(px(size::META + 0.5))
        .text_color(p.accent_text)
        .cursor_pointer()
        .hover(|s| s.underline())
        .child(text.to_string())
        .on_click(cx.listener(move |this, _, w, cx| this.navigate(route, w, cx)))
        .into_any_element()
}

/// Breadcrumb back link above a detail page.
pub fn crumb(p: &Palette, text: &str, route: Route, cx: &mut Context<VzApp>) -> AnyElement {
    h_flex()
        .id("crumb")
        .gap(px(4.))
        .pb(px(10.))
        .text_size(px(size::META + 0.5))
        .text_color(p.muted)
        .cursor_pointer()
        .hover(|s| s.text_color(p.text))
        .child(crate::ui::components::icon(gpui_kit::assets::IconName::ArrowLeft, p.muted, 14.))
        .child(text.to_string())
        .on_click(cx.listener(move |this, _, w, cx| this.navigate(route, w, cx)))
        .into_any_element()
}

pub fn tone_for_gain(gain: i64) -> Tone {
    match gain.signum() {
        1 => Tone::Positive,
        -1 => Tone::Negative,
        _ => Tone::Neutral,
    }
}

pub fn column(width: f32) -> Div {
    div().w(px(width)).flex_none()
}

pub fn ncolumn(width: f32) -> Div {
    div().w(px(width)).flex_none().text_right()
}

pub fn grow() -> Div {
    v_flex().flex_1().min_w_0()
}
