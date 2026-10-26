//! Overlay: a floating layer anchored under the element it belongs to.
//! It rises into place, draws above everything (so no card clips it),
//! stays inside the window, and closes on Escape or a click outside.
//! Menus and popovers are built on it.
//!
//! Put it as a child of the element it hangs from; that element needs
//! `relative()`.

use std::rc::Rc;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::interact::scoped;
use crate::ui::theme::{Elevation, RADIUS_CONTROL, elevation, motion, palette};

/// Which edge of the anchor the layer lines up with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayAlign {
    #[default]
    Start,
    End,
}

type KeyHandler = Rc<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;
type DismissHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Overlay {
    id: ElementId,
    align: OverlayAlign,
    min_width: f32,
    focus: Option<FocusHandle>,
    on_key: Option<KeyHandler>,
    on_dismiss: DismissHandler,
    children: Vec<AnyElement>,
}

impl Overlay {
    pub fn new(id: impl Into<ElementId>, on_dismiss: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        Self {
            id: id.into(),
            align: OverlayAlign::default(),
            min_width: 180.0,
            focus: None,
            on_key: None,
            on_dismiss: Rc::new(on_dismiss),
            children: Vec::new(),
        }
    }

    pub fn align(mut self, align: OverlayAlign) -> Self {
        self.align = align;
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    /// Keys reach the layer while this handle has focus; Escape always
    /// dismisses, everything else goes to `on_key`.
    pub fn focus(mut self, handle: FocusHandle) -> Self {
        self.focus = Some(handle);
        self
    }

    pub fn on_key(mut self, handler: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_key = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Overlay {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for Overlay {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let (dismiss_out, dismiss_key) = (self.on_dismiss.clone(), self.on_dismiss);
        let on_key = self.on_key;
        let panel = v_flex()
            .id(self.id.clone())
            .occlude()
            .min_w(px(self.min_width))
            .p(px(4.))
            .gap(px(1.))
            .bg(p.surface)
            .rounded(px(RADIUS_CONTROL + 2.))
            .border_1()
            .border_color(p.border)
            .shadow(elevation(&p, Elevation::Floating))
            .when_some(self.focus, |d, f| d.track_focus(&f))
            .on_mouse_down_out(move |_, w, cx| dismiss_out(w, cx))
            .on_key_down(move |e: &KeyDownEvent, w, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    dismiss_key(w, cx);
                } else if let Some(f) = &on_key {
                    f(e, w, cx);
                }
            })
            .children(self.children)
            .with_spring(scoped(&self.id, "rise"), SpringAnimation::new(motion::RISE).to(1.0f32).from(0.0), |d, t| {
                d.opacity(t.clamp(0.0, 1.0)).mt(px(4.0 - motion::RISE_DISTANCE * (1.0 - t)))
            });
        let corner = match self.align {
            OverlayAlign::Start => Anchor::TopLeft,
            OverlayAlign::End => Anchor::TopRight,
        };
        div()
            .absolute()
            .top_full()
            .when(self.align == OverlayAlign::Start, |d| d.left_0())
            .when(self.align == OverlayAlign::End, |d| d.right_0())
            .child(deferred(anchored().anchor(corner).snap_to_window_with_margin(px(8.)).child(panel)).with_priority(1))
    }
}
