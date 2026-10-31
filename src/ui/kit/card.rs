//! Card: the raised surface content sits on, with an optional header
//! (title on the left, one element on the right).

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::ui::components::sel;
use crate::ui::theme::{Elevation, Palette, RADIUS_CARD, Typeset as _, elevation, palette, space, text};

#[derive(IntoElement)]
pub struct Card {
    base: Div,
    title: Option<AnyElement>,
    action: Option<AnyElement>,
    elevation: Elevation,
    children: Vec<AnyElement>,
}

impl Card {
    pub fn new() -> Self {
        Self { base: v_flex(), title: None, action: None, elevation: Elevation::Raised, children: Vec::new() }
    }

    /// Heading in the card's header. The text is selectable.
    #[track_caller]
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(sel(title).into_any_element());
        self
    }

    /// One element on the right of the header: a link, a control, a count.
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }

    pub fn elevation(mut self, level: Elevation) -> Self {
        self.elevation = level;
        self
    }
}

/// A card's face: `surface_top` shading down to `surface` (flat in light
/// mode, where the two match).
pub fn face(p: &Palette) -> Background {
    linear_gradient(180., linear_color_stop(p.surface_top, 0.), linear_color_stop(p.surface, 1.))
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let header = (self.title.is_some() || self.action.is_some()).then(|| {
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(space::M))
                .px(px(space::XL))
                .pt(px(space::L + 4.))
                .pb(px(space::M))
                .child(div().min_w_0().truncate().typeset(text::HEADING).text_color(p.text).children(self.title))
                .children(self.action)
        });
        self.base
            .bg(face(&p))
            .rounded(px(RADIUS_CARD))
            .border_1()
            .border_color(p.border)
            .shadow(elevation(&p, self.elevation))
            .children(header)
            .children(self.children)
    }
}
