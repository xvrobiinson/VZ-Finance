//! List row: leading mark, a title over a subtitle, and right-aligned
//! trailing content (amounts, pills, row actions). Clickable rows fade a
//! fill in under the pointer.

use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ClickHandler;
use super::interact::{faded, hover_state, mix, scoped};
use crate::ui::components::sel;
use crate::ui::theme::{Typeset as _, motion, palette, space, text};

#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    leading: Option<AnyElement>,
    title: Option<AnyElement>,
    subtitle: Option<AnyElement>,
    trailing: Vec<AnyElement>,
    muted: bool,
    on_click: Option<ClickHandler>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), leading: None, title: None, subtitle: None, trailing: Vec::new(), muted: false, on_click: None }
    }

    /// Avatar or icon on the left.
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    #[track_caller]
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(sel(title).into_any_element());
        self
    }

    #[track_caller]
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(sel(subtitle).into_any_element());
        self
    }

    /// Right-aligned content; numbers here line up down the list.
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing.push(el.into_any_element());
        self
    }

    /// Greys the title, for rows that no longer count (reversed, archived).
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let (hovered, on_hover) = hover_state(&self.id, window, cx);
        let clickable = self.on_click.is_some();
        h_flex()
            .id(self.id.clone())
            .min_h(px(60.))
            .py(px(space::S))
            .px(px(space::XL))
            .gap(px(space::M))
            .children(self.leading)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().typeset(text::BODY).text_color(if self.muted { p.muted } else { p.text }).truncate().children(self.title))
                    .children(self.subtitle.map(|s| {
                        div().typeset(text::CAPTION).text_color(p.muted).font_features(crate::ui::theme::tabular()).truncate().child(s)
                    })),
            )
            .children(self.trailing)
            .when_some(self.on_click, |d, f| d.cursor_pointer().on_hover(on_hover).on_click(move |e, w, cx| f(e, w, cx)))
            .with_spring(scoped(&self.id, "fill"), SpringAnimation::new(motion::CONTROL).to(hovered && clickable), move |d, t| {
                d.bg(mix(faded(p.hover, 0.0), p.hover, t.0))
            })
    }
}
