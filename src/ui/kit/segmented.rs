//! Segmented control: a few mutually exclusive options in one track. The
//! thumb glides to the chosen option instead of jumping; cells share one
//! width so nothing shifts as it moves.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ClickHandler;
use super::interact::{faded, scoped};
use crate::ui::theme::{RADIUS_CHIP, RADIUS_CONTROL, Typeset as _, motion, palette, text};

const INSET: f32 = 2.0;
/// Cells are sized from the longest label: an average glyph of the label
/// face plus the cell's side padding, never under `CELL_MIN`.
const GLYPH: f32 = 7.2;
const CELL_PAD: f32 = 22.0;
const CELL_MIN: f32 = 40.0;

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    cell: Option<f32>,
    options: Vec<(SharedString, bool, ClickHandler)>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self { id: id.into(), cell: None, options: Vec::new() }
    }

    /// A control over `options` (key, label) that calls `on_select` on the
    /// view with the picked key. The usual way to build one.
    pub fn pick<T: 'static, K: Copy + PartialEq + 'static>(
        id: impl Into<ElementId>,
        options: impl IntoIterator<Item = (K, impl Into<SharedString>)>,
        selected: K,
        cx: &mut Context<T>,
        on_select: impl Fn(&mut T, K, &mut Window, &mut Context<T>) + Clone + 'static,
    ) -> Self {
        options.into_iter().fold(Self::new(id), |seg, (k, label)| {
            let on_select = on_select.clone();
            seg.option(label, k == selected, cx.listener(move |this, _, w, cx| on_select(this, k, w, cx)))
        })
    }

    /// Width of every cell (px). Without it, cells fit the longest label.
    pub fn cell_width(mut self, w: f32) -> Self {
        self.cell = Some(w);
        self
    }

    /// The width cells take when fitted to these labels.
    pub fn fitted_cell<'a>(labels: impl IntoIterator<Item = &'a str>) -> f32 {
        let longest = labels.into_iter().map(|l| l.chars().count()).max().unwrap_or(1) as f32;
        (longest * GLYPH + CELL_PAD).max(CELL_MIN)
    }

    /// Full width of a control fitted to these labels, track included.
    pub fn fitted_width(labels: &[&str]) -> f32 {
        Self::fitted_cell(labels.iter().copied()) * labels.len() as f32 + 2.0 * INSET
    }

    pub fn option(
        mut self,
        label: impl Into<SharedString>,
        selected: bool,
        on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.options.push((label.into(), selected, Rc::new(on_select)));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let cell = self.cell.unwrap_or_else(|| Self::fitted_cell(self.options.iter().map(|(l, _, _)| l.as_ref())));
        let count = self.options.len();
        let at = self.options.iter().position(|(_, s, _)| *s).unwrap_or(0);
        let thumb = div()
            .absolute()
            .top(px(INSET))
            .w(px(cell))
            .h(px(26.))
            .rounded(px(RADIUS_CHIP))
            .bg(p.surface)
            .shadow(vec![
                BoxShadow::new(px(0.), px(1.), faded(p.shadow_ink, 0.08 * p.shadow_gain)).blur_radius(px(2.)),
                BoxShadow::new(px(0.), px(2.), faded(p.shadow_ink, 0.05 * p.shadow_gain)).blur_radius(px(6.)),
            ])
            .with_spring(scoped(&self.id, "thumb"), SpringAnimation::new(motion::GLIDE).to(px(INSET + at as f32 * cell)), |d, x| d.left(x));
        let cells = self.options.into_iter().enumerate().map(|(ix, (label, selected, f))| {
            h_flex()
                .id(ElementId::NamedChild(Arc::new(self.id.clone()), ix.to_string().into()))
                .relative()
                .w(px(cell))
                .h(px(26.))
                .justify_center()
                .typeset(text::LABEL)
                .text_size(px(12.5))
                .cursor_pointer()
                .text_color(if selected { p.text } else { p.muted })
                .when(!selected, |d| d.hover(move |s| s.text_color(p.text)))
                .child(label)
                .on_click(move |e, w, cx| f(e, w, cx))
        });
        h_flex()
            .relative()
            .flex_none()
            .w(px(cell * count as f32 + 2.0 * INSET))
            .p(px(INSET))
            .rounded(px(RADIUS_CONTROL))
            .bg(p.neutral_tint)
            .child(thumb)
            .children(cells)
    }
}
