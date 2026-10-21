use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::nice_ticks;
use crate::domain::money::{Cents, MoneyFormat};
use crate::ui::theme::{Palette, tabular};

/// Columns with square tops on a zero baseline. Empty periods draw no bar.
pub fn column_chart(p: &Palette, labels: &[SharedString], values: &[Cents], highlight_last: bool, money: &MoneyFormat, height: f32) -> AnyElement {
    let max = values.iter().copied().max().unwrap_or(0);
    let ticks = nice_ticks(0, max.max(1), 3, true);
    let top = *ticks.last().unwrap_or(&1);
    let plot_h = height - 26.0;
    let lane = 56.0;
    let y_labels = ticks.iter().map(|t| {
        let y = plot_h * (1.0 - *t as f32 / top as f32) - 8.0;
        div()
            .absolute()
            .left_0()
            .top(px(y))
            .w(px(lane - 10.0))
            .text_right()
            .text_size(px(11.5))
            .text_color(p.faint)
            .font_features(tabular())
            .child(money.format_compact(*t))
    });
    let grid = ticks.iter().map(|t| {
        let y = plot_h * (1.0 - *t as f32 / top as f32);
        div().absolute().left_0().right_0().top(px(y)).h(px(1.)).bg(p.grid)
    });
    let n = values.len();
    let cols = values.iter().enumerate().map(|(i, v)| {
        let h = if top > 0 { plot_h * (*v as f32 / top as f32) } else { 0.0 };
        let is_last = i + 1 == n;
        let color = if highlight_last && !is_last {
            let mut c = p.accent;
            c.a = 0.55;
            c
        } else {
            p.accent
        };
        let label = labels.get(i).cloned().unwrap_or_default();
        let value_text = money.format(*v);
        v_flex()
            .id(("col", i))
            .flex_1()
            .h_full()
            .items_center()
            .justify_end()
            .gap(px(6.))
            .child(
                div()
                    .w(relative(0.56))
                    .max_w(px(36.))
                    .h(px(h.max(0.0)))
                    .when(*v > 0, |d| d.min_h(px(2.)))
                    .bg(color)
                    .rounded_t(px(3.)),
            )
            .child(div().h(px(20.)).text_size(px(11.5)).text_color(p.faint).child(label))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(value_text.clone()).build(window, cx))
    });
    div()
        .relative()
        .w_full()
        .h(px(height))
        .children(y_labels)
        .child(
            div()
                .absolute()
                .left(px(lane))
                .right_0()
                .top_0()
                .bottom_0()
                .children(grid)
                .child(h_flex().absolute().left_0().right_0().top_0().h(px(height)).items_end().children(cols)),
        )
        .into_any_element()
}
