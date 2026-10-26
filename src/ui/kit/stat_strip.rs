//! Stat strip: a card of [`Stat`]s, `per_row` to a row, divided by rules.
//! When everything fits on one row the lead stat gets more room. Figures
//! never shrink to fit; callers lower `per_row` instead.

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Card, Stat};
use crate::ui::components::vrule;
use crate::ui::theme::{palette, space};

#[derive(IntoElement)]
pub struct StatStrip {
    per_row: usize,
    dense: bool,
    stats: Vec<Stat>,
}

impl StatStrip {
    pub fn new(per_row: usize) -> Self {
        Self { per_row: per_row.max(1), dense: false, stats: Vec::new() }
    }

    /// 16px cell padding instead of 24px, for tight rows.
    pub fn dense(mut self, dense: bool) -> Self {
        self.dense = dense;
        self
    }

    pub fn stat(mut self, stat: Stat) -> Self {
        self.stats.push(stat);
        self
    }

    pub fn stats(mut self, stats: impl IntoIterator<Item = Stat>) -> Self {
        self.stats.extend(stats);
        self
    }
}

impl RenderOnce for StatStrip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let one_row = self.per_row >= self.stats.len();
        let pad = if self.dense { space::L } else { space::XL };
        let mut cells: Vec<Stat> = self
            .stats
            .into_iter()
            .enumerate()
            .map(|(i, s)| s.flex_1().flex_basis(px(0.)).when(i == 0 && one_row, |d| d.flex_grow(1.4)).px(px(pad)).py(px(space::L + 2.)))
            .collect();
        let mut card = Card::new();
        let mut first = true;
        while !cells.is_empty() {
            let rest = cells.split_off(self.per_row.min(cells.len()));
            if !first {
                card = card.child(div().h(px(1.)).mx(px(space::XL)).bg(p.border));
            }
            first = false;
            let row = cells
                .into_iter()
                .enumerate()
                .fold(h_flex().items_stretch(), |row, (i, c)| row.when(i > 0, |r| r.child(vrule(&p))).child(c));
            card = card.child(row);
            cells = rest;
        }
        card
    }
}
