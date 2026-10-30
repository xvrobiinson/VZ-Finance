//! Button: one shape for every action. Hover springs the fill in, press
//! nudges the face down a pixel, keyboard focus draws a ring, and Enter or
//! Space activate it.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ClickHandler;
use super::interact::{faded, focus_state, hover_state, mix, scoped};
use crate::ui::components::icon;
use crate::ui::theme::{Palette, RADIUS_CHIP, RADIUS_CONTROL, Typeset as _, motion, palette, text};

/// What the button is for. One `Primary` per view; most are `Secondary`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    /// The main action of the view.
    Primary,
    /// Everything else that deserves a frame.
    #[default]
    Secondary,
    /// Actions inside rows and toolbars, framed only on hover.
    Ghost,
    /// Destroys or removes something; the main action of its view.
    Danger,
    /// A destructive action that sits among others (settings, menus): red
    /// ink, framed only on hover.
    DangerGhost,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    /// 22px: an icon action inline with text (edit next to a label).
    Mini,
    /// 28px: inside rows, cards and dense toolbars.
    Small,
    /// 32px: page actions and dialogs.
    #[default]
    Medium,
}

impl ButtonSize {
    fn height(self) -> f32 {
        match self {
            Self::Mini => 22.0,
            Self::Small => 28.0,
            Self::Medium => 32.0,
        }
    }

    fn pad(self) -> f32 {
        match self {
            Self::Mini => 6.0,
            Self::Small => 10.0,
            Self::Medium => 12.0,
        }
    }

    fn icon(self) -> f32 {
        match self {
            Self::Mini => 12.0,
            Self::Small => 14.0,
            Self::Medium => 15.0,
        }
    }
}

/// Fill at rest, fill under the pointer, fill while pressed, ink, and the
/// edge color (transparent when the button has none).
struct Colors {
    rest: Hsla,
    hover: Hsla,
    press: Hsla,
    ink: Hsla,
    edge: Hsla,
}

fn colors(p: &Palette, kind: ButtonKind) -> Colors {
    match kind {
        ButtonKind::Primary => Colors {
            rest: p.accent,
            hover: p.accent_hover,
            press: mix(p.accent_hover, p.shadow_ink, 0.12),
            ink: p.on_accent,
            edge: p.accent,
        },
        ButtonKind::Secondary => Colors { rest: p.surface, hover: p.hover, press: p.active, ink: p.text, edge: p.control },
        ButtonKind::Ghost => Colors { rest: faded(p.hover, 0.0), hover: p.hover, press: p.active, ink: p.text, edge: faded(p.hover, 0.0) },
        ButtonKind::DangerGhost => Colors {
            rest: faded(p.negative_tint, 0.0),
            hover: p.negative_tint,
            press: mix(p.negative_tint, p.negative, 0.12),
            ink: p.negative,
            edge: faded(p.negative_tint, 0.0),
        },
        ButtonKind::Danger => Colors {
            rest: p.negative,
            hover: mix(p.negative, p.shadow_ink, 0.1),
            press: mix(p.negative, p.shadow_ink, 0.2),
            ink: p.on_accent,
            edge: p.negative,
        },
    }
}

/// Framed buttons sit on a contact shadow; the primary one also casts a
/// soft glow of its own color and catches light along its top edge.
fn depth(p: &Palette, kind: ButtonKind) -> Vec<BoxShadow> {
    let contact = |a: f32| BoxShadow::new(px(0.), px(1.), faded(p.shadow_ink, a * p.shadow_gain)).blur_radius(px(2.));
    match kind {
        ButtonKind::Primary | ButtonKind::Danger => {
            let glow = if kind == ButtonKind::Primary { p.accent } else { p.negative };
            vec![
                contact(0.10),
                BoxShadow::new(px(0.), px(6.), faded(glow, 0.45)).blur_radius(px(14.)).spread_radius(px(-6.)),
                BoxShadow::new(px(0.), px(1.), faded(p.on_accent, 0.16)).inset(),
            ]
        }
        ButtonKind::Secondary => vec![contact(0.05)],
        ButtonKind::Ghost | ButtonKind::DangerGhost => Vec::new(),
    }
}

/// The focus ring, outside the button's edge.
fn ring(p: &Palette) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), faded(p.accent, 0.35)).spread_radius(px(3.))]
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    wide: bool,
    kind: ButtonKind,
    size: ButtonSize,
    tooltip: Option<SharedString>,
    disabled: bool,
    loading: bool,
    selected: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            prefix: None,
            suffix: None,
            wide: false,
            kind: ButtonKind::default(),
            size: ButtonSize::default(),
            tooltip: None,
            disabled: false,
            loading: false,
            selected: false,
            on_click: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Leading icon. Without a label the button becomes a square.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Anything after the label, such as a dropdown's chevron.
    /// Something drawn before the label in its own colors (a brand mark),
    /// where `.icon()` takes the text color.
    pub fn prefix(mut self, el: impl IntoElement) -> Self {
        self.prefix = Some(el.into_any_element());
        self
    }

    /// Fills the width of its column, label centered (a form standing alone,
    /// like signing in).
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    pub fn suffix(mut self, el: impl IntoElement) -> Self {
        self.suffix = Some(el.into_any_element());
        self
    }

    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn primary(self) -> Self {
        self.kind(ButtonKind::Primary)
    }

    pub fn ghost(self) -> Self {
        self.kind(ButtonKind::Ghost)
    }

    pub fn danger(self) -> Self {
        self.kind(ButtonKind::Danger)
    }

    pub fn danger_ghost(self) -> Self {
        self.kind(ButtonKind::DangerGhost)
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Small)
    }

    /// Held down: the button shows its hover fill while what it controls is
    /// open or on (a dropdown's menu, a toggle).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Shown after the pointer rests; put the shortcut here ("Ctrl+P").
    /// Disabled buttons keep it, so it can say why.
    pub fn tooltip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tip.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Working on it: a spinner takes the icon's place, the label and
    /// width stay put, and clicks are ignored until it finishes.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let c = colors(&p, self.kind);
        let (hovered, on_hover) = hover_state(&self.id, window, cx);
        let focus = focus_state(&self.id, window, cx);
        let live = !self.disabled && !self.loading;
        let size = self.size;
        let square = self.label.is_none() && self.suffix.is_none() && self.prefix.is_none();
        let lit = (hovered && live) || self.selected;
        let ink = if self.kind == ButtonKind::Ghost && !lit && self.label.is_none() { p.muted } else { c.ink };
        let shadows = depth(&p, self.kind);
        let focus_shadows = [shadows.clone(), ring(&p)].concat();

        let face = h_flex()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .h(px(size.height()))
            .when(square, |d| d.w(px(size.height())).justify_center())
            .when(!square, |d| d.px(px(size.pad())))
            .when(self.wide, |d| d.w_full().justify_center())
            .gap(px(6.))
            .rounded(px(if size == ButtonSize::Mini { RADIUS_CHIP } else { RADIUS_CONTROL }))
            .border_1()
            .border_color(c.edge)
            .shadow(shadows)
            .text_color(ink)
            .typeset(text::LABEL)
            .whitespace_nowrap()
            .when(self.loading, |d| d.child(Spinner::new().color(ink).with_size(px(size.icon()))))
            .when(!self.loading, |d| d.children(self.icon.map(|i| icon(i, ink, size.icon()))).children(self.prefix))
            .children(self.label)
            .children(self.suffix)
            .when(live, |d| {
                d.track_focus(&focus.tab_stop(true))
                    .cursor_pointer()
                    .on_hover(on_hover)
                    .active(move |s| s.bg(c.press).top(px(1.)))
                    .focus_visible(move |s| s.border_color(p.accent).shadow(focus_shadows))
            })
            .when(self.disabled, |d| d.opacity(0.5))
            .when_some(self.tooltip.filter(|_| !self.loading), |d, tip| d.tooltip(move |w, cx| Tooltip::new(tip.clone()).build(w, cx)))
            .when_some(self.on_click.filter(|_| live), |d, f| d.on_click(move |e, w, cx| f(e, w, cx)));

        face.with_spring(scoped(&self.id, "fill"), SpringAnimation::new(motion::CONTROL).to(lit), move |d, t| {
            d.bg(mix(c.rest, c.hover, t.0))
        })
    }
}
