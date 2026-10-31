//! Stat cards: each [`Stat`] on its own card, `per_row` to a row. The last
//! row keeps the same cell width as the rows above it. Figures never shrink
//! to fit; callers lower `per_row` instead.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::{Card, Stat};
use crate::ui::theme::space;

#[derive(IntoElement)]
pub struct StatCards {
    per_row: usize,
    spacing: f32,
    stats: Vec<Stat>,
}

impl StatCards {
    pub fn new(per_row: usize) -> Self {
        Self { per_row: per_row.max(1), spacing: space::L, stats: Vec::new() }
    }

    /// Space between cards (the screen's card gap).
    pub fn spacing(mut self, px: f32) -> Self {
        self.spacing = px;
        self
    }

    pub fn stats(mut self, stats: impl IntoIterator<Item = Stat>) -> Self {
        self.stats.extend(stats);
        self
    }
}

impl RenderOnce for StatCards {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let gap = px(self.spacing);
        let per_row = self.per_row;
        let mut cells: Vec<AnyElement> = self
            .stats
            .into_iter()
            .map(|s| Card::new().flex_1().flex_basis(px(0.)).min_w_0().p(px(space::L + 2.)).child(s).into_any_element())
            .collect();
        let mut rows = Vec::new();
        while !cells.is_empty() {
            let rest = cells.split_off(per_row.min(cells.len()));
            let short = per_row - cells.len();
            rows.push(h_flex().items_stretch().gap(gap).children(cells).children((0..short).map(|_| div().flex_1().flex_basis(px(0.)))));
            cells = rest;
        }
        v_flex().gap(gap).children(rows)
    }
}
