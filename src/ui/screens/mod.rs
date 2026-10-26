//! Screens render the snapshot held by `VzApp`; they never touch the store.

pub mod analytics;
pub mod clients;
mod installments;
pub mod loans;
pub mod overview;
pub mod payments;
pub mod settings;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{Route, VzApp};
use crate::database::repositories::changes::{Action, Change};
use crate::domain::ledger::{Delinquency, DisplayStatus};
use crate::i18n::t;
use crate::tr;
use crate::ui::components::{icon, pill, status_tone};
use crate::ui::kit::{Button, Card};
use crate::ui::theme::{Palette, Typeset as _, space, tabular, text};
use crate::utils::dates::days_between;

/// A small ghost icon button for a table row (edit, delete). Clicks don't
/// reach the row underneath.
pub fn row_icon_button(
    id: impl Into<ElementId>,
    icon_name: IconName,
    tip: &'static str,
    cx: &mut Context<VzApp>,
    on_click: impl Fn(&mut VzApp, &mut Window, &mut Context<VzApp>) + 'static,
) -> Button {
    Button::new(id).ghost().small().icon(icon_name).tooltip(tip).on_click(cx.listener(move |this, _, w, cx| {
        cx.stop_propagation();
        on_click(this, w, cx)
    }))
}

impl VzApp {
    /// Edits and deletions, newest first, each with what changed and why.
    pub(crate) fn history_card(&self, p: &Palette, changes: &[&Change], limit: usize) -> AnyElement {
        let rows = changes.iter().take(limit).map(|c| {
            let d = crate::ui::history::describe(c, &self.snap);
            let deleted = c.action == Action::Delete;
            h_flex()
                .items_start()
                .gap(px(space::M))
                .px(px(space::XL))
                .py(px(space::M))
                .border_b_1()
                .border_color(p.rule)
                .child(
                    h_flex()
                        .flex_none()
                        .mt(px(1.))
                        .size(px(24.))
                        .rounded_full()
                        .justify_center()
                        .bg(if deleted { p.negative_tint } else { p.neutral_tint })
                        .child(icon(
                            if deleted { IconName::Trash } else { IconName::Pencil },
                            if deleted { p.negative } else { p.muted },
                            12.,
                        )),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            h_flex()
                                .justify_between()
                                .gap(px(space::M))
                                .child(div().typeset(text::BODY_STRONG).min_w_0().truncate().child(d.title))
                                .child(div().flex_none().typeset(text::CAPTION).text_color(p.faint).child(d.when)),
                        )
                        .children(
                            d.lines.into_iter().map(|l| div().typeset(text::CAPTION).text_color(p.muted).font_features(tabular()).child(l)),
                        )
                        .when(!d.reason.is_empty(), |el| {
                            let why = d.reason.clone();
                            el.child(div().typeset(text::CAPTION).text_color(p.text).child(tr!("Reason: {why}", "Motivo: {why}")))
                        }),
                )
        });
        Card::new()
            .overflow_hidden()
            .title(t("Change history", "Historial de cambios"))
            .action(
                div()
                    .typeset(text::CAPTION)
                    .text_color(p.muted)
                    .child(t("Every edit and deletion is kept here", "Aquí queda cada edición y eliminación")),
            )
            .child(v_flex().children(rows))
            .into_any_element()
    }
}

/// Status pill with the delinquency spelled out ("12 days overdue").
pub fn status_detail_pill(p: &Palette, status: DisplayStatus, q: &Delinquency, today: chrono::NaiveDate) -> Div {
    let text = match status {
        DisplayStatus::Overdue => {
            let d = q.days_overdue.max(1);
            if d == 1 { t("1 day overdue", "1 día de atraso").to_string() } else { tr!("{d} days overdue", "{d} días de atraso") }
        }
        DisplayStatus::DueSoon => match q.next_due.map(|n| days_between(today, n)) {
            Some(0) => t("Due today", "Vence hoy").to_string(),
            Some(1) => t("Due tomorrow", "Vence mañana").to_string(),
            Some(n) => tr!("Due in {n} days", "Vence en {n} días"),
            None => t("Due soon", "Vence pronto").to_string(),
        },
        s => s.label().to_string(),
    };
    pill(p, text, status_tone(status))
}

/// "View all" style link in a card header.
pub fn header_link(p: &Palette, id: &str, text: &str, route: Route, cx: &mut Context<VzApp>) -> AnyElement {
    div()
        .id(SharedString::from(id.to_string()))
        .typeset(text::CAPTION)
        .text_size(px(12.5))
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
        .typeset(text::CAPTION)
        .text_size(px(12.5))
        .text_color(p.muted)
        .cursor_pointer()
        .hover(|s| s.text_color(p.text))
        .child(crate::ui::components::icon(gpui_kit::assets::IconName::ArrowLeft, p.muted, 14.))
        .child(text.to_string())
        .on_click(cx.listener(move |this, _, w, cx| this.navigate(route, w, cx)))
        .into_any_element()
}

pub fn column(width: f32) -> Div {
    div().w(px(width)).flex_none()
}

/// A right-aligned number column: tabular figures, never wrapped or cut.
pub fn ncolumn(width: f32) -> Div {
    div().w(px(width)).flex_none().text_right().font_features(tabular()).whitespace_nowrap()
}
