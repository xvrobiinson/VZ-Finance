//! Pointer state that outlives a frame, so components can spring between
//! rest and hover instead of snapping.

use std::sync::Arc;

use gpui_kit::*;

/// An id scoped under another ("range" / "hover"), for the extra state a
/// component keeps next to its element.
pub fn scoped(id: &ElementId, part: &'static str) -> ElementId {
    ElementId::NamedChild(Arc::new(id.clone()), part.into())
}

/// Whether the pointer is over the element with this id, and the listener
/// to hand to its `on_hover` so the answer stays current.
pub fn hover_state(id: &ElementId, window: &mut Window, cx: &mut App) -> (bool, impl Fn(&bool, &mut Window, &mut App) + 'static) {
    let state = window.use_keyed_state(scoped(id, "hover"), cx, |_, _| false);
    let now = *state.read(cx);
    let set = move |hovered: &bool, _: &mut Window, cx: &mut App| {
        state.update(cx, |s, cx| {
            if *s != *hovered {
                *s = *hovered;
                cx.notify();
            }
        });
    };
    (now, set)
}

/// A focus handle that belongs to the element with this id.
pub fn focus_state(id: &ElementId, window: &mut Window, cx: &mut App) -> FocusHandle {
    window.use_keyed_state(scoped(id, "focus"), cx, |_, cx| cx.focus_handle()).read(cx).clone()
}

/// `a` to `b` by `t`, through RGB so greys don't detour through other hues.
pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b): (Rgba, Rgba) = (a.into(), b.into());
    let t = t.clamp(0.0, 1.0);
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rgba { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }.into()
}

/// The same color with its alpha scaled, for hover fills that fade in from
/// nothing without shifting hue.
pub fn faded(c: Hsla, alpha: f32) -> Hsla {
    Hsla { a: c.a * alpha.clamp(0.0, 1.0), ..c }
}
