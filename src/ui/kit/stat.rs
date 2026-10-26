//! Stat: one headline figure with its label above and a caption below. The
//! lead stat of a strip is set larger, in the display face.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::ui::components::{icon, sel};
use crate::ui::theme::{Typeset as _, palette, space, tabular, text};

#[derive(IntoElement)]
pub struct Stat {
    base: Div,
    icon: Option<IconName>,
    label: AnyElement,
    value: AnyElement,
    tone: Option<Hsla>,
    caption: Option<AnyElement>,
    accessory: Option<AnyElement>,
    lead: bool,
}

impl Stat {
    #[track_caller]
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            base: v_flex(),
            icon: None,
            label: sel(label).into_any_element(),
            value: sel(value).into_any_element(),
            tone: None,
            caption: None,
            accessory: None,
            lead: false,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Color for the figure when it carries a state (overdue in red).
    pub fn tone(mut self, color: Hsla) -> Self {
        self.tone = Some(color);
        self
    }

    pub fn caption(mut self, el: impl IntoElement) -> Self {
        self.caption = Some(el.into_any_element());
        self
    }

    /// A small control after the label (an edit button).
    pub fn accessory(mut self, el: impl IntoElement) -> Self {
        self.accessory = Some(el.into_any_element());
        self
    }

    /// The stat the strip is about: display size.
    pub fn lead(mut self, lead: bool) -> Self {
        self.lead = lead;
        self
    }
}

impl Styled for Stat {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Stat {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        self.base
            .min_w_0()
            .gap(px(space::S))
            .child(
                h_flex()
                    .h(px(22.))
                    .gap(px(6.))
                    .children(self.icon.map(|i| icon(i, p.faint, 14.)))
                    .child(div().min_w_0().truncate().typeset(text::CAPTION).text_size(px(12.5)).text_color(p.muted).child(self.label))
                    .children(self.accessory),
            )
            .child(
                div()
                    .typeset(if self.lead { text::DISPLAY } else { text::FIGURE })
                    .font_features(tabular())
                    .whitespace_nowrap()
                    .text_color(self.tone.unwrap_or(p.text))
                    .child(self.value),
            )
            .children(self.caption.map(|c| div().min_w_0().truncate().typeset(text::CAPTION).text_color(p.muted).child(c)))
    }
}
