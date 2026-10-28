//! Sortable table column header: the column name with an arrow that shows
//! the sort direction when the table is sorted by it. Clicking sorts by the
//! column; clicking it again flips the direction.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ClickHandler;
use super::interact::hover_state;
use crate::ui::components::icon;
use crate::ui::theme::{Typeset as _, palette, text};

/// Which column a table is sorted by, and which way. `None`: the table's
/// own order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort<C> {
    pub column: Option<C>,
    pub descending: bool,
}

impl<C> Default for Sort<C> {
    fn default() -> Self {
        Self { column: None, descending: false }
    }
}

impl<C: Copy + PartialEq> Sort<C> {
    /// Sorted by `column`: the first click sorts ascending, the next flips.
    pub fn toggle(self, column: C) -> Self {
        match self.column {
            Some(c) if c == column => Self { column: Some(column), descending: !self.descending },
            _ => Self { column: Some(column), descending: false },
        }
    }

    /// `ordering` in this sort's direction.
    pub fn apply(self, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
        if self.descending { ordering.reverse() } else { ordering }
    }
}

#[derive(IntoElement)]
pub struct SortHeader {
    id: ElementId,
    label: SharedString,
    /// Sorted by this column: Some(descending).
    active: Option<bool>,
    right: bool,
    on_click: Option<ClickHandler>,
}

impl SortHeader {
    /// A header for `column` of a table sorted by `sort`.
    pub fn new<C: Copy + PartialEq>(id: impl Into<ElementId>, label: impl Into<SharedString>, column: C, sort: Sort<C>) -> Self {
        let active = (sort.column == Some(column)).then_some(sort.descending);
        Self { id: id.into(), label: label.into(), active, right: false, on_click: None }
    }

    /// For number and money columns: label and arrow sit on the right.
    pub fn right(mut self) -> Self {
        self.right = true;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SortHeader {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let (hovered, on_hover) = hover_state(&self.id, window, cx);
        let color = if self.active.is_some() || hovered { p.text } else { p.muted };
        // The arrow shows the direction; on hover an inactive column hints
        // that it can be sorted.
        let arrow = match self.active {
            Some(false) => Some((IconName::ArrowUp, p.accent)),
            Some(true) => Some((IconName::ArrowDown, p.accent)),
            None if hovered => Some((IconName::ChevronsUpDown, p.faint)),
            None => None,
        };
        let on_click = self.on_click.clone();
        h_flex()
            .id(self.id.clone())
            .gap(px(3.))
            .when(self.right, |d| d.flex_row_reverse())
            .cursor_pointer()
            .on_hover(on_hover)
            .typeset(text::CAPTION)
            .text_color(color)
            .whitespace_nowrap()
            .when(self.active.is_some(), |d| d.font_weight(FontWeight::MEDIUM))
            .child(self.label)
            // Keeps its place so the label never shifts when the arrow appears.
            .child(div().size(px(12.)).flex_none().children(arrow.map(|(name, c)| icon(name, c, 12.))))
            .when_some(on_click, |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
    }
}
