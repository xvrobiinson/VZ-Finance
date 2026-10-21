use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::nice_ticks;
use crate::domain::money::{Cents, MoneyFormat};
use crate::ui::theme::{Palette, size, tabular};

#[derive(Clone)]
pub struct Series {
    pub name: SharedString,
    pub values: Vec<Cents>,
    pub color: Hsla,
    pub width: f32,
    pub dashed: bool,
    /// Flat tint under the line (alpha is applied here).
    pub fill: bool,
}

#[derive(Clone)]
pub struct LineChart {
    pub labels: Vec<SharedString>,
    /// Long label per point for the hover readout.
    pub hover_labels: Vec<SharedString>,
    pub series: Vec<Series>,
    pub height: f32,
    pub money: MoneyFormat,
}

const Y_LANE: f32 = 64.0;
const X_LANE: f32 = 26.0;
const TOP_PAD: f32 = 10.0;

pub type HoverHandler = Rc<dyn Fn(Option<usize>, &mut Window, &mut App)>;

impl LineChart {
    fn point_count(&self) -> usize {
        self.series.iter().map(|s| s.values.len()).max().unwrap_or(0)
    }

    /// Renders the chart. `hover` is the highlighted point index, owned by the
    /// caller so the readout survives re-renders.
    pub fn render(self, id: impl Into<ElementId>, p: &Palette, hover: Option<usize>, on_hover: HoverHandler) -> AnyElement {
        let n = self.point_count();
        let all: Vec<Cents> = self.series.iter().flat_map(|s| s.values.iter().copied()).collect();
        let (min, max) = (all.iter().copied().min().unwrap_or(0), all.iter().copied().max().unwrap_or(0));
        let ticks = nice_ticks(min, max, 4, min >= 0 && max > 0 && min < max / 3);
        let (lo, hi) = (*ticks.first().unwrap_or(&0), *ticks.last().unwrap_or(&1));
        let span = (hi - lo).max(1) as f32;
        let plot_h = self.height - X_LANE - TOP_PAD;
        let frac_y = move |v: Cents| 1.0 - (v - lo) as f32 / span;

        let bounds_cell: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));
        let series = self.series.clone();
        let grid = p.grid;
        let tick_fracs: Vec<f32> = ticks.iter().map(|t| frac_y(*t)).collect();
        let hover_line = p.chart_reference;
        let surface = p.surface;

        let canvas_el = {
            let bounds_cell = bounds_cell.clone();
            canvas(
                move |bounds, _, _| {
                    bounds_cell.set(Some(bounds));
                    bounds
                },
                move |bounds, _, window, _| {
                    let w = bounds.size.width;
                    let h = bounds.size.height;
                    let ox = bounds.origin.x;
                    let oy = bounds.origin.y;
                    let x_at = |i: usize| ox + if n <= 1 { w / 2.0 } else { w * (i as f32 / (n - 1) as f32) };
                    let y_at = |f: f32| oy + h * f;
                    for (i, f) in tick_fracs.iter().enumerate() {
                        let mut b = PathBuilder::stroke(px(1.));
                        b.move_to(point(ox, y_at(*f)));
                        b.line_to(point(ox + w, y_at(*f)));
                        if let Ok(path) = b.build() {
                            let mut c = grid;
                            if i == 0 {
                                c.a = 1.0;
                            }
                            window.paint_path(path, c);
                        }
                    }
                    if let Some(i) = hover.filter(|i| *i < n) {
                        let mut b = PathBuilder::stroke(px(1.));
                        b.move_to(point(x_at(i), oy));
                        b.line_to(point(x_at(i), oy + h));
                        if let Ok(path) = b.build() {
                            window.paint_path(path, hover_line);
                        }
                    }
                    for s in &series {
                        if s.values.is_empty() {
                            continue;
                        }
                        let pts: Vec<Point<Pixels>> =
                            s.values.iter().enumerate().map(|(i, v)| point(x_at(i), y_at(1.0 - (*v - lo) as f32 / span))).collect();
                        if s.fill && pts.len() > 1 {
                            let mut b = PathBuilder::fill();
                            b.move_to(point(pts[0].x, oy + h));
                            for pt in &pts {
                                b.line_to(*pt);
                            }
                            b.line_to(point(pts[pts.len() - 1].x, oy + h));
                            b.close();
                            if let Ok(path) = b.build() {
                                let mut c = s.color;
                                c.a = 0.07;
                                window.paint_path(path, c);
                            }
                        }
                        let mut b = PathBuilder::stroke(px(s.width));
                        if s.dashed {
                            b = b.dash_array(&[px(4.), px(4.)]);
                        }
                        b.move_to(pts[0]);
                        if pts.len() == 1 {
                            b.line_to(point(pts[0].x + px(0.5), pts[0].y));
                        }
                        for pt in pts.iter().skip(1) {
                            b.line_to(*pt);
                        }
                        if let Ok(path) = b.build() {
                            window.paint_path(path, s.color);
                        }
                        if let Some(i) = hover.filter(|i| *i < pts.len()) {
                            let c = pts[i];
                            let r = px(4.);
                            window.paint_quad(
                                fill(Bounds::new(point(c.x - r, c.y - r), size(r * 2., r * 2.)), surface)
                                    .corner_radii(r)
                                    .border_widths(px(2.))
                                    .border_color(s.color),
                            );
                        }
                    }
                },
            )
            .size_full()
        };

        // Y tick labels, in their own lane.
        let y_labels = ticks.iter().map(|t| {
            let top = TOP_PAD + plot_h * frac_y(*t) - 8.0;
            div()
                .absolute()
                .left_0()
                .top(px(top))
                .w(px(Y_LANE - 12.0))
                .text_right()
                .text_size(px(11.5))
                .text_color(p.faint)
                .font_features(tabular())
                .child(self.money.format_compact(*t))
        });

        // X labels: about five, evenly spaced, first and last pulled inside.
        let label_count = n.min(6);
        let picks: Vec<usize> = if n == 0 {
            vec![]
        } else if label_count <= 1 {
            vec![0]
        } else {
            (0..label_count).map(|k| k * (n - 1) / (label_count - 1)).collect()
        };
        let x_labels = picks.into_iter().map(|i| {
            let frac = if n <= 1 { 0.5 } else { i as f32 / (n - 1) as f32 };
            let text = self.labels.get(i).cloned().unwrap_or_default();
            let el = div().absolute().bottom_0().w(px(90.)).text_size(px(11.5)).text_color(p.faint).child(text);
            if frac <= 0.0 {
                el.left_0().text_left()
            } else if frac >= 1.0 {
                el.right_0().text_right()
            } else {
                el.left(relative(frac)).ml(px(-45.)).text_center()
            }
        });

        // Hover readout.
        let readout = hover.filter(|i| *i < n).map(|i| {
            let frac = if n <= 1 { 0.5 } else { i as f32 / (n - 1) as f32 };
            let rows = self.series.iter().filter_map(|s| s.values.get(i).map(|v| (s, *v))).map(|(s, v)| {
                h_flex()
                    .gap(px(8.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(div().size(px(7.)).rounded_full().bg(s.color))
                            .child(div().text_color(p.muted).child(s.name.clone())),
                    )
                    .child(div().font_features(tabular()).text_color(p.text).child(self.money.format(v)))
            });
            let el = v_flex()
                .absolute()
                .top(px(0.))
                .w(px(230.))
                .p(px(10.))
                .gap(px(4.))
                .bg(p.surface)
                .rounded(px(8.))
                .border_1()
                .border_color(p.border)
                .shadow(vec![BoxShadow::new(px(0.), px(6.), p.shadow).blur_radius(px(20.))])
                .text_size(px(size::META))
                .child(div().text_color(p.text).font_weight(FontWeight::MEDIUM).child(self.hover_labels.get(i).cloned().unwrap_or_default()))
                .children(rows);
            if frac > 0.6 { el.right(relative(1.0 - frac)).mr(px(12.)) } else { el.left(relative(frac)).ml(px(12.)) }
        });

        let handler_move = on_hover.clone();
        let handler_leave = on_hover;
        let bounds_for_move = bounds_cell.clone();

        div()
            .relative()
            .w_full()
            .h(px(self.height))
            .children(y_labels)
            .child(
                div()
                    .id(id)
                    .absolute()
                    .left(px(Y_LANE))
                    .right_0()
                    .top(px(TOP_PAD))
                    .h(px(plot_h))
                    .child(canvas_el)
                    .children(readout)
                    .on_mouse_move(move |e: &MouseMoveEvent, window, cx| {
                        if let Some(b) = bounds_for_move.get()
                            && n > 0
                        {
                            let rel = ((e.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0);
                            let idx = (rel * (n.saturating_sub(1)) as f32).round() as usize;
                            if Some(idx) != hover {
                                handler_move(Some(idx), window, cx);
                            }
                        }
                    })
                    .on_hover(move |hovered, window, cx| {
                        if !*hovered {
                            handler_leave(None, window, cx);
                        }
                    }),
            )
            .child(div().absolute().left(px(Y_LANE)).right_0().bottom_0().h(px(X_LANE - 6.0)).children(x_labels))
            .into_any_element()
    }
}
