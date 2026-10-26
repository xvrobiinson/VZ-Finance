//! A date field in the app's language: type the date (dd/mm/yyyy in
//! Spanish, mm/dd/yyyy in English) or pick it from a small calendar.

use chrono::{Datelike, NaiveDate};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::i18n::{self, t};
use crate::ui::components::icon;
use crate::ui::kit::Button;
use crate::ui::theme::{Elevation, RADIUS_CHIP, RADIUS_CONTROL, Typeset as _, elevation, palette, space, tabular, text};
use crate::utils::dates::{add_months, input_format, input_placeholder, month_start, month_year, parse_input, today};

pub struct DateChanged;

pub struct DateField {
    input: Entity<InputState>,
    open: bool,
    month: NaiveDate,
    _subs: Vec<Subscription>,
}

impl EventEmitter<DateChanged> for DateField {}

impl DateField {
    pub fn new(date: NaiveDate, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let lang = i18n::get();
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(input_placeholder(lang)).default_value(date.format(input_format(lang)).to_string())
        });
        let subs = vec![cx.subscribe(&input, |this, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                if let Some(d) = this.date(cx) {
                    this.month = month_start(d);
                }
                cx.emit(DateChanged);
                cx.notify();
            }
        })];
        Self { input, open: false, month: month_start(date), _subs: subs }
    }

    /// The typed date, if it is a valid date.
    pub fn date(&self, cx: &App) -> Option<NaiveDate> {
        parse_input(&self.input.read(cx).value(), i18n::get())
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.read(cx).focus_handle(cx)
    }

    fn pick(&mut self, d: NaiveDate, window: &mut Window, cx: &mut Context<Self>) {
        let text = d.format(input_format(i18n::get())).to_string();
        self.input.update(cx, |s, cx| s.set_value(text, window, cx));
        self.month = month_start(d);
        self.open = false;
        cx.emit(DateChanged);
        cx.notify();
    }

    fn calendar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let selected = self.date(cx);
        let now = today();
        let first = self.month;
        // Sunday-first grid covering the whole month.
        let lead = first.weekday().num_days_from_sunday() as i64;
        let grid_start = first - chrono::Duration::days(lead);
        let weeks = (0..6).map(|w| {
            h_flex().children((0..7).map(|dow| {
                let day = grid_start + chrono::Duration::days(w * 7 + dow);
                let in_month = day.month() == first.month();
                let is_sel = selected == Some(day);
                let is_today = day == now;
                div()
                    .id(("day", (w * 7 + dow) as u64))
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(RADIUS_CHIP))
                    .typeset(text::CAPTION)
                    .font_features(tabular())
                    .cursor_pointer()
                    .text_color(if is_sel {
                        p.on_accent
                    } else if !in_month {
                        p.faint
                    } else {
                        p.text
                    })
                    .when(is_sel, |d| d.bg(p.accent))
                    .when(!is_sel && is_today, |d| d.border_1().border_color(p.accent))
                    .when(!is_sel, |d| d.hover(|s| s.bg(p.hover)))
                    .child(day.day().to_string())
                    .on_click(cx.listener(move |this, _, w, cx| this.pick(day, w, cx)))
            }))
        });
        let nav_button = |id: &'static str, name: IconName, delta: i32| {
            Button::new(id).ghost().small().icon(name).on_click(cx.listener(move |this, _, _, cx| {
                this.month = if delta < 0 {
                    this.month.checked_sub_months(chrono::Months::new(1)).unwrap_or(this.month)
                } else {
                    add_months(this.month, 1)
                };
                cx.notify();
            }))
        };
        let title = month_year(first);
        let title = {
            let mut c = title.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        };
        v_flex()
            .occlude()
            .p(px(10.))
            .gap(px(6.))
            .w(px(244.))
            .bg(p.surface)
            .rounded(px(RADIUS_CONTROL + 2.))
            .border_1()
            .border_color(p.border)
            .shadow(elevation(&p, Elevation::Floating))
            .child(
                h_flex()
                    .justify_between()
                    .child(nav_button("prev", IconName::ChevronLeft, -1))
                    .child(div().typeset(text::LABEL).child(title))
                    .child(nav_button("next", IconName::ChevronRight, 1)),
            )
            .child(h_flex().children(
                i18n::weekdays().map(|w| {
                    div().size(px(32.)).flex().items_center().justify_center().typeset(text::CAPTION).text_color(p.faint).child(w)
                }),
            ))
            .children(weeks)
            .child(
                div()
                    .id("today")
                    .pt(px(space::XS))
                    .typeset(text::LABEL)
                    .text_color(p.accent_text)
                    .cursor_pointer()
                    .child(t("Today", "Hoy"))
                    .on_click(cx.listener(move |this, _, w, cx| this.pick(today(), w, cx))),
            )
            .into_any_element()
    }
}

impl Render for DateField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let toggle = div()
            .id("cal-toggle")
            .flex_none()
            .size(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(RADIUS_CONTROL))
            .border_1()
            .border_color(p.control)
            .cursor_pointer()
            .hover(|s| s.bg(p.hover))
            .when(self.open, |d| d.bg(p.accent_tint).border_color(p.accent))
            .child(icon(IconName::Calendar, if self.open { p.accent_text } else { p.muted }, 15.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.open = !this.open;
                    cx.notify();
                }),
            );
        let popup = self.open.then(|| {
            deferred(
                anchored().offset(point(px(0.), px(36.))).snap_to_window_with_margin(px(8.)).child(
                    div()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.open = false;
                            cx.notify();
                        }))
                        .child(self.calendar(cx)),
                ),
            )
            .with_priority(gpui_kit::base::POPUP_PRIORITY)
        });
        v_flex()
            .w_full()
            .child(h_flex().gap(px(6.)).child(div().flex_1().min_w_0().child(Input::new(&self.input))).child(toggle))
            .children(popup)
    }
}
