//! Dropdown: a button that opens a menu of choices in an [`Overlay`].
//! Works as a select (one item `selected`, trigger shows its label) or as
//! an action menu. Arrow keys move, Enter picks, Escape closes; the chevron
//! turns as it opens.

use std::f32::consts::PI;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::interact::{faded, hover_state, mix, scoped};
use super::{Button, ButtonKind, ButtonSize, ClickHandler, Overlay, OverlayAlign};
use crate::ui::components::icon;
use crate::ui::theme::{RADIUS_CHIP, Typeset as _, motion, palette, space, text};

pub struct DropdownItem {
    label: SharedString,
    icon: Option<IconName>,
    hint: Option<SharedString>,
    selected: bool,
    danger: bool,
    disabled: bool,
    on_select: ClickHandler,
}

impl DropdownItem {
    pub fn new(label: impl Into<SharedString>, on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self { label: label.into(), icon: None, hint: None, selected: false, danger: false, disabled: false, on_select: Rc::new(on_select) }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Faint text on the right, usually the shortcut.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Deletes or removes something: red ink.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    /// Shown but not pickable; give the reason in `hint`.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl FluentBuilder for DropdownItem {}

#[derive(Default)]
struct MenuState {
    open: bool,
    highlight: Option<usize>,
}

#[derive(IntoElement)]
pub struct Dropdown {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    kind: ButtonKind,
    size: ButtonSize,
    align: OverlayAlign,
    tooltip: Option<SharedString>,
    items: Vec<DropdownItem>,
}

impl Dropdown {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            kind: ButtonKind::Secondary,
            size: ButtonSize::Medium,
            align: OverlayAlign::Start,
            tooltip: None,
            items: Vec::new(),
        }
    }

    /// Trigger text. For a select, the selected item's label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Trigger icon. With no label the trigger is a square icon button
    /// without a chevron (a "more" menu).
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn small(mut self) -> Self {
        self.size = ButtonSize::Small;
        self
    }

    /// Line the menu up with the trigger's right edge.
    pub fn align_end(mut self) -> Self {
        self.align = OverlayAlign::End;
        self
    }

    pub fn tooltip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tip.into());
        self
    }

    pub fn item(mut self, item: DropdownItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = DropdownItem>) -> Self {
        self.items.extend(items);
        self
    }
}

impl RenderOnce for Dropdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let state = window.use_keyed_state(scoped(&self.id, "menu"), cx, |_, _| MenuState::default());
        let focus = window.use_keyed_state(scoped(&self.id, "menu-focus"), cx, |_, cx| cx.focus_handle()).read(cx).clone();
        // Pressing the trigger while open must close the menu, not let the
        // outside-click close it and the click reopen it.
        let (over_trigger, on_trigger_hover) = hover_state(&scoped(&self.id, "trigger"), window, cx);
        let open = state.read(cx).open;
        let highlight = state.read(cx).highlight;
        let selected = self.items.iter().position(|i| i.selected);
        let handlers: Rc<Vec<Option<ClickHandler>>> =
            Rc::new(self.items.iter().map(|i| (!i.disabled).then(|| i.on_select.clone())).collect());
        let count = self.items.len();

        let chevron = (self.label.is_some()).then(|| {
            icon(IconName::ChevronDown, p.muted, 14.).with_spring(
                scoped(&self.id, "chevron"),
                SpringAnimation::new(motion::GLIDE).to(open),
                |i, t| i.rotate(radians(PI * t.0)),
            )
        });

        let toggle = {
            let state = state.clone();
            let focus = focus.clone();
            move |_: &ClickEvent, w: &mut Window, cx: &mut App| {
                let now_open = state.update(cx, |s, cx| {
                    s.open = !s.open;
                    s.highlight = selected.or(Some(0));
                    cx.notify();
                    s.open
                });
                if now_open {
                    w.focus(&focus, cx);
                }
            }
        };

        let mut trigger = Button::new(scoped(&self.id, "button")).kind(self.kind).size(self.size).selected(open).on_click(toggle);
        if let Some(i) = self.icon {
            trigger = trigger.icon(i);
        }
        if let Some(l) = self.label {
            trigger = trigger.label(l);
        }
        if let Some(c) = chevron {
            trigger = trigger.suffix(c);
        }
        if let Some(t) = self.tooltip.filter(|_| !open) {
            trigger = trigger.tooltip(t);
        }

        let menu = open.then(|| {
            let close = {
                let state = state.clone();
                move |_: &mut Window, cx: &mut App| {
                    state.update(cx, |s, cx| {
                        s.open = false;
                        cx.notify();
                    })
                }
            };
            let close_unless_trigger = {
                let close = close.clone();
                move |w: &mut Window, cx: &mut App| {
                    if !over_trigger {
                        close(w, cx)
                    }
                }
            };
            let keys = {
                let (state, handlers, close) = (state.clone(), handlers.clone(), close.clone());
                move |e: &KeyDownEvent, w: &mut Window, cx: &mut App| {
                    let step = |h: Option<usize>, d: isize| -> Option<usize> {
                        let n = count as isize;
                        (n > 0).then(|| (h.map_or(if d > 0 { -1 } else { 0 }, |h| h as isize) + d).rem_euclid(n) as usize)
                    };
                    match e.keystroke.key.as_str() {
                        "down" => state.update(cx, |s, cx| {
                            s.highlight = step(s.highlight, 1);
                            cx.notify();
                        }),
                        "up" => state.update(cx, |s, cx| {
                            s.highlight = step(s.highlight, -1);
                            cx.notify();
                        }),
                        "enter" | "space" => {
                            if let Some(f) = state.read(cx).highlight.and_then(|h| handlers.get(h).cloned().flatten()) {
                                close(w, cx);
                                f(&ClickEvent::default(), w, cx);
                            }
                        }
                        _ => return,
                    }
                    cx.stop_propagation();
                }
            };
            let rows = self.items.into_iter().enumerate().map(|(ix, item)| {
                let lit = highlight == Some(ix) && !item.disabled;
                let (state_move, close, f) = (state.clone(), close.clone(), item.on_select.clone());
                let (ink, fill) = if item.danger { (p.negative, p.negative_tint) } else { (p.text, p.hover) };
                let quiet = if item.danger { p.negative } else { p.muted };
                h_flex()
                    .id(ElementId::NamedChild(std::sync::Arc::new(scoped(&self.id, "item")), ix.to_string().into()))
                    .h(px(30.))
                    .px(px(space::S))
                    .gap(px(space::S))
                    .rounded(px(RADIUS_CHIP))
                    .typeset(text::LABEL)
                    .font_weight(if item.selected { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                    .text_color(ink)
                    .children(item.icon.map(|i| icon(i, if lit { ink } else { quiet }, 15.)))
                    .child(div().flex_1().min_w_0().truncate().child(item.label))
                    .children(item.hint.map(|h| div().typeset(text::CAPTION).text_color(p.faint).child(h)))
                    .child(div().w(px(16.)).flex_none().children(item.selected.then(|| icon(IconName::Check, p.accent_text, 14.))))
                    .when(item.disabled, |d| d.opacity(0.45))
                    .when(!item.disabled, |d| {
                        d.cursor_pointer()
                            .on_mouse_move(move |_, _, cx| {
                                state_move.update(cx, |s, cx| {
                                    if s.highlight != Some(ix) {
                                        s.highlight = Some(ix);
                                        cx.notify();
                                    }
                                })
                            })
                            .on_click(move |e, w, cx| {
                                close(w, cx);
                                f(e, w, cx);
                            })
                    })
                    .with_spring(
                        ElementId::NamedChild(std::sync::Arc::new(scoped(&self.id, "lit")), ix.to_string().into()),
                        SpringAnimation::new(motion::CONTROL).to(lit),
                        move |d, t| d.bg(mix(faded(fill, 0.0), fill, t.0)),
                    )
            });
            Overlay::new(scoped(&self.id, "overlay"), close_unless_trigger)
                .align(self.align)
                .focus(focus.clone())
                .on_key(keys)
                .children(rows)
        });

        // A row of its own so the trigger keeps its width when a column
        // parent stretches this wrapper.
        h_flex().id(scoped(&self.id, "trigger")).relative().flex_none().on_hover(on_trigger_hover).child(trigger).children(menu)
    }
}
