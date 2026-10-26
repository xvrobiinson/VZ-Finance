//! Small reusable pieces: cards, figures, pills, avatars, table rows,
//! empty states. All styling comes from `ui::theme`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::Location;

use gpui_kit::assets::IconName;
use gpui_kit::base::SelectableText;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::domain::ledger::DisplayStatus;
use crate::ui::theme::{Palette, RADIUS_CHIP, Typeset as _, space, tabular, text};

thread_local! {
    /// How many times each `sel` call site has run in the frame being built.
    static SEL_SEEN: RefCell<HashMap<&'static Location<'static>, u64>> = RefCell::new(HashMap::new());
}

/// Starts a new frame of selectable text ids. Call once at the top of the
/// window's render, before any `sel`.
pub fn begin_selectable_frame() {
    SEL_SEEN.with(|seen| seen.borrow_mut().clear());
}

/// Text the user can select with the mouse and copy with Ctrl+C. It takes
/// its look from the parent like a plain string child does.
///
/// Every piece of selectable text needs a stable id of its own. The call
/// site plus how many times it ran this frame gives one, also for rows
/// built in a loop.
#[track_caller]
pub fn sel(text: impl Into<SharedString>) -> SelectableText {
    let at = Location::caller();
    let nth = SEL_SEEN.with(|seen| {
        let mut seen = seen.borrow_mut();
        let n = seen.entry(at).or_default();
        *n += 1;
        *n
    });
    let id = ElementId::NamedChild(std::sync::Arc::new(ElementId::CodeLocation(*at)), nth.to_string().into());
    SelectableText::new(id, text)
}

pub fn icon(name: IconName, color: Hsla, size_px: f32) -> Icon {
    Icon::new(name).text_color(color).size(px(size_px))
}

#[track_caller]
pub fn num(text: impl Into<SharedString>) -> Div {
    div().font_features(tabular()).whitespace_nowrap().child(sel(text))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Positive,
    Warning,
    Negative,
}

pub fn tone_colors(p: &Palette, tone: Tone) -> (Hsla, Hsla) {
    match tone {
        Tone::Neutral => (p.neutral_tint, p.muted),
        Tone::Accent => (p.accent_tint, p.accent_text),
        Tone::Positive => (p.positive_tint, p.positive),
        Tone::Warning => (p.warning_tint, p.warning),
        Tone::Negative => (p.negative_tint, p.negative),
    }
}

pub fn pill(p: &Palette, text: impl Into<SharedString>, tone: Tone) -> Div {
    let (bg, fg) = tone_colors(p, tone);
    h_flex()
        .flex_none()
        .h(px(22.))
        .px(px(8.))
        .rounded(px(RADIUS_CHIP))
        .bg(bg)
        .text_color(fg)
        .typeset(text::CAPTION)
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(text.into())
}

pub fn status_tone(s: DisplayStatus) -> Tone {
    match s {
        DisplayStatus::Active => Tone::Neutral,
        DisplayStatus::DueSoon => Tone::Warning,
        DisplayStatus::Overdue => Tone::Negative,
        DisplayStatus::PaidOff => Tone::Positive,
        DisplayStatus::Defaulted => Tone::Negative,
        DisplayStatus::Restructured => Tone::Neutral,
    }
}

pub fn status_pill(p: &Palette, s: DisplayStatus) -> Div {
    pill(p, s.label(), status_tone(s))
}

/// Initials in a pale tint whose hue is stable per client id.
pub fn avatar(p: &Palette, name: &str, id: i64, diameter: f32) -> Div {
    let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
    // Golden-angle hue steps keep neighbours far apart.
    let hue = ((id as f32 * 137.508) % 360.0) / 360.0;
    let (bg, fg) = if p.dark {
        (hsla(hue, 0.30, 0.22, 1.0), hsla(hue, 0.45, 0.80, 1.0))
    } else {
        (hsla(hue, 0.55, 0.94, 1.0), hsla(hue, 0.45, 0.32, 1.0))
    };
    h_flex()
        .flex_none()
        .size(px(diameter))
        .rounded_full()
        .bg(bg)
        .justify_center()
        .text_color(fg)
        .text_size(px((diameter * 0.38).round()))
        .font_weight(FontWeight::MEDIUM)
        .child(initials)
}

pub fn progress_bar(p: &Palette, ratio: f32, color: Hsla) -> Div {
    let ratio = ratio.clamp(0.0, 1.0);
    div().w_full().h(px(6.)).rounded_full().bg(p.neutral_tint).child(div().h_full().rounded_full().bg(color).w(relative(ratio)))
}

/// An empty block keeps its frame: icon, one sentence, optional action.
pub fn empty_state(p: &Palette, icon_name: IconName, title: &str, body: &str, action: Option<AnyElement>) -> Div {
    v_flex()
        .items_center()
        .justify_center()
        .gap(px(space::S))
        .py(px(40.))
        .px(px(space::XL))
        .child(h_flex().size(px(40.)).rounded_full().bg(p.neutral_tint).justify_center().child(icon(icon_name, p.muted, 18.)))
        .child(div().pt(px(4.)).typeset(text::HEADING).font_weight(FontWeight::MEDIUM).text_color(p.text).child(title.to_string()))
        .child(div().max_w(px(380.)).text_center().text_color(p.muted).child(body.to_string()))
        .children(action.map(|a| div().pt(px(space::S)).child(a)))
}

/// Table header cell / row helpers. Columns are given fixed or flex widths
/// by the caller; numbers are right-aligned.
pub fn th(p: &Palette, label: &str) -> Div {
    div().typeset(text::CAPTION).text_color(p.muted).whitespace_nowrap().child(label.to_string())
}

pub fn table_header(p: &Palette) -> Div {
    h_flex().h(px(36.)).px(px(space::XL)).gap(px(space::L)).border_b_1().border_color(p.border)
}

pub fn table_row(p: &Palette, id: impl Into<ElementId>, clickable: bool) -> Stateful<Div> {
    let hover = p.hover;
    h_flex()
        .id(id)
        .min_h(px(52.))
        .px(px(space::XL))
        .gap(px(space::L))
        .border_b_1()
        .border_color(p.rule)
        .when(clickable, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
}

/// A key/value line inside a card ("Maturity    Jan 1, 2027"). Wrap a text
/// value in `sel` to make it copyable.
#[track_caller]
pub fn kv(p: &Palette, key: impl Into<SharedString>, value: impl IntoElement) -> Div {
    h_flex()
        .justify_between()
        .gap(px(space::L))
        .min_h(px(30.))
        .child(div().text_color(p.muted).child(sel(key)))
        .child(div().font_features(tabular()).text_color(p.text).child(value))
}

/// Page header: title, one-line finding, actions.
#[track_caller]
pub fn page_header(p: &Palette, title: impl Into<SharedString>, subtitle: Option<AnyElement>, actions: Vec<AnyElement>) -> Div {
    h_flex()
        .flex_wrap()
        .justify_between()
        .items_end()
        .gap(px(space::L))
        .pb(px(space::XL))
        .child(
            v_flex()
                .gap(px(6.))
                .min_w_0()
                .child(div().typeset(text::TITLE).text_color(p.text).child(sel(title)))
                .children(subtitle.map(|s| div().text_color(p.muted).child(s))),
        )
        .child(h_flex().gap(px(space::S)).flex_none().children(actions))
}

/// A thin vertical divider for strips of figures.
pub fn vrule(p: &Palette) -> Div {
    div().w(px(1.)).self_stretch().my(px(space::L)).bg(p.border)
}

/// Keyboard hint chip ("Ctrl P").
pub fn kbd(p: &Palette, keys: &str) -> Div {
    h_flex()
        .h(px(18.))
        .px(px(5.))
        .rounded(px(4.))
        .border_1()
        .border_color(p.border)
        .typeset(text::MICRO)
        .font_weight(FontWeight::NORMAL)
        .text_color(p.faint)
        .child(keys.to_string())
}
