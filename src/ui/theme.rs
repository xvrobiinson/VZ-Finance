//! Design tokens. Every color, size and radius in the UI comes from here.
//!
//! Light is the reference theme. Grey text tiers are tuned to stay at or
//! above 4.5:1 on white and on the app ground. Status hues are darker than
//! their fill tints so text stays legible.

use gpui_kit::*;

use crate::services::settings::ThemePref;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub ground: Hsla,
    pub surface: Hsla,
    /// Top of a card's face; cards shade from this down to `surface`
    /// (the same color in light mode, a touch lighter in dark).
    pub surface_top: Hsla,
    /// The sidebar, a step darker than the ground in dark mode.
    pub sidebar: Hsla,
    pub raised: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub border: Hsla,
    pub rule: Hsla,
    pub control: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub faint: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_tint: Hsla,
    pub accent_text: Hsla,
    pub on_accent: Hsla,
    pub positive: Hsla,
    pub positive_tint: Hsla,
    pub warning: Hsla,
    pub warning_tint: Hsla,
    pub negative: Hsla,
    pub negative_tint: Hsla,
    /// Icon chips that tell stats apart. Never a money state.
    pub info: Hsla,
    pub info_tint: Hsla,
    pub neutral_tint: Hsla,
    pub grid: Hsla,
    pub chart_reference: Hsla,
    pub chart_secondary: Hsla,
    pub shadow: Hsla,
    /// Opaque ink every elevation layer is cut from; `shadow_gain` scales
    /// its alpha so dark surfaces, which swallow shadows, still lift.
    pub shadow_ink: Hsla,
    pub shadow_gain: f32,
    /// Hairline of light along the top edge of raised surfaces.
    pub highlight: Hsla,
    pub overlay: Hsla,
    /// The sign-in screen: a sky (night in dark mode) behind a glass card.
    pub sky_top: Hsla,
    pub sky_low: Hsla,
    pub cloud: Hsla,
    pub arc: Hsla,
    pub glass_top: Hsla,
    pub glass: Hsla,
    pub glass_edge: Hsla,
    pub dark: bool,
}

fn hex(v: u32) -> Hsla {
    rgb(v).into()
}

fn hexa(v: u32, a: f32) -> Hsla {
    let mut c: Hsla = rgb(v).into();
    c.a = a;
    c
}

impl Palette {
    pub fn light() -> Self {
        Self {
            ground: hex(0xF6F7FB),
            surface: hex(0xFFFFFF),
            surface_top: hex(0xFFFFFF),
            sidebar: hex(0xFFFFFF),
            raised: hex(0xFAFAFC),
            hover: hex(0xF3F3F8),
            active: hex(0xEEEDF6),
            border: hex(0xE6E7EE),
            rule: hex(0xEEEFF4),
            control: hex(0xDCDDE6),
            text: hex(0x17181C),
            muted: hex(0x5E6270),
            faint: hex(0x6B6F7C),
            accent: hex(0x5B47E8),
            accent_hover: hex(0x4B37D6),
            accent_tint: hex(0xEEEBFF),
            accent_text: hex(0x4B37D6),
            on_accent: hex(0xFFFFFF),
            positive: hex(0x047857),
            positive_tint: hex(0xE3F7EE),
            warning: hex(0x8A5300),
            warning_tint: hex(0xFFF4DC),
            negative: hex(0xB42323),
            negative_tint: hex(0xFDECEC),
            info: hex(0x2459C9),
            info_tint: hex(0xE6F0FF),
            neutral_tint: hex(0xF1F2F6),
            grid: hex(0xEEEFF4),
            chart_reference: hex(0xF08A2C),
            chart_secondary: hex(0x0E9F6E),
            shadow: hexa(0x141E32, 0.045),
            shadow_ink: hex(0x0F1A2E),
            shadow_gain: 1.0,
            highlight: hexa(0xFFFFFF, 0.0),
            overlay: hexa(0x0B1020, 0.32),
            sky_top: hex(0x9FC9F1),
            sky_low: hex(0xF1F6FC),
            cloud: hexa(0xFFFFFF, 0.9),
            arc: hexa(0xFFFFFF, 0.55),
            glass_top: hex(0xDCF2FB),
            glass: hexa(0xFFFFFF, 0.9),
            glass_edge: hexa(0xFFFFFF, 0.95),
            dark: false,
        }
    }

    pub fn dark() -> Self {
        Self {
            ground: hex(0x16171B),
            surface: hex(0x1B1C21),
            surface_top: hex(0x23242A),
            sidebar: hex(0x121316),
            raised: hex(0x1F2025),
            hover: hex(0x26272E),
            active: hex(0x2C2D34),
            border: hex(0x2A2B31),
            rule: hex(0x24252A),
            control: hex(0x3A3B42),
            text: hex(0xF1F2F5),
            muted: hex(0x9A9CA7),
            faint: hex(0x8E909B),
            accent: hex(0x6E5BF6),
            accent_hover: hex(0x7C6CFF),
            accent_tint: hex(0x26213F),
            accent_text: hex(0xA99DFF),
            on_accent: hex(0xFFFFFF),
            positive: hex(0x34D399),
            positive_tint: hex(0x14291F),
            warning: hex(0xF5B451),
            warning_tint: hex(0x2E2414),
            negative: hex(0xF87171),
            negative_tint: hex(0x351C1E),
            info: hex(0x7FB2FF),
            info_tint: hex(0x1B2538),
            neutral_tint: hex(0x26272E),
            grid: hex(0x26272C),
            chart_reference: hex(0xF59E42),
            chart_secondary: hex(0x34D399),
            shadow: hexa(0x000000, 0.25),
            shadow_ink: hex(0x000000),
            shadow_gain: 4.0,
            highlight: hexa(0xFFFFFF, 0.045),
            overlay: hexa(0x000000, 0.5),
            sky_top: hex(0x091226),
            sky_low: hex(0x0E0F11),
            cloud: hexa(0x6E87C8, 0.1),
            arc: hexa(0x8CA5E6, 0.13),
            glass_top: hex(0x19223C),
            glass: hexa(0x17181B, 0.94),
            glass_edge: hexa(0xFFFFFF, 0.07),
            dark: true,
        }
    }

    pub fn for_pref(pref: ThemePref) -> Self {
        match pref {
            ThemePref::Light => Self::light(),
            ThemePref::Dark => Self::dark(),
        }
    }
}

impl Global for Palette {}

pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

/// Font families. Inter sets everything up to 18px; Inter Display, cut
/// tighter for large sizes, sets titles and figures from 20px up. GPUI has
/// no letter-spacing, so the display cut is how large type gets tracked in.
pub mod font {
    pub const TEXT: &str = "Inter";
    pub const DISPLAY: &str = "Inter Display";
}

/// Raw sizes (px), kept for code that has not moved to [`TextRole`] yet.
pub mod size {
    pub const META: f32 = 12.0;
    pub const BODY: f32 = 13.5;
    pub const SECTION: f32 = 15.0;
    pub const FIGURE: f32 = 22.0;
    pub const LEAD: f32 = 30.0;
    pub const TITLE: f32 = 24.0;
}

/// One typographic role: family, size, line height and weight together, so
/// a role can't be half-applied.
#[derive(Clone, Copy, Debug)]
pub struct TextRole {
    pub family: &'static str,
    pub size: f32,
    pub line: f32,
    pub weight: FontWeight,
}

/// The type scale (ratio about 1.125 in the text range, larger for
/// display). Pick a role, never a raw size.
pub mod text {
    use super::{FontWeight, TextRole, font};

    const fn role(family: &'static str, size: f32, line: f32, weight: FontWeight) -> TextRole {
        TextRole { family, size, line, weight }
    }

    /// The one number a screen is about (portfolio value).
    pub const DISPLAY: TextRole = role(font::DISPLAY, 30.0, 36.0, FontWeight::SEMIBOLD);
    /// Page titles.
    pub const TITLE: TextRole = role(font::DISPLAY, 24.0, 30.0, FontWeight::SEMIBOLD);
    /// Secondary headline figures (KPI values).
    pub const FIGURE: TextRole = role(font::DISPLAY, 22.0, 28.0, FontWeight::MEDIUM);
    /// Card and section headings.
    pub const HEADING: TextRole = role(font::TEXT, 15.0, 20.0, FontWeight::SEMIBOLD);
    /// Running text, table cells.
    pub const BODY: TextRole = role(font::TEXT, 13.5, 20.0, FontWeight::NORMAL);
    /// Names and values that lead a row.
    pub const BODY_STRONG: TextRole = role(font::TEXT, 13.5, 20.0, FontWeight::MEDIUM);
    /// Buttons, tabs, menu items.
    pub const LABEL: TextRole = role(font::TEXT, 13.0, 18.0, FontWeight::MEDIUM);
    /// Secondary lines, field labels, table headers.
    pub const CAPTION: TextRole = role(font::TEXT, 12.0, 16.0, FontWeight::NORMAL);
    /// Keyboard hints and dense badges.
    pub const MICRO: TextRole = role(font::TEXT, 11.0, 14.0, FontWeight::MEDIUM);
}

/// Applies a [`TextRole`] to any styled element.
pub trait Typeset: Styled + Sized {
    fn typeset(self, role: TextRole) -> Self {
        self.font_family(role.family).text_size(px(role.size)).line_height(px(role.line)).font_weight(role.weight)
    }
}

impl<T: Styled> Typeset for T {}

/// How much room the content column has. Screens branch on this, never on
/// raw pixel widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Width {
    /// Under 760px: one column, figures two to a row.
    Compact,
    /// 760–1180px: cards stack, figures three to a row.
    Medium,
    /// 1180–1680px: side by side.
    Wide,
    /// 1680px and up: side by side with room to spare.
    Ultra,
}

impl Width {
    pub fn of(content_px: f32) -> Self {
        match content_px {
            w if w < 760.0 => Self::Compact,
            w if w < 1180.0 => Self::Medium,
            w if w < 1680.0 => Self::Wide,
            _ => Self::Ultra,
        }
    }

    /// Side gutter of the content column, from the window width.
    pub fn gutter(viewport_px: f32) -> f32 {
        match viewport_px {
            w if w < 900.0 => space::L,
            w if w < 1400.0 => space::XL,
            _ => space::XXL + 4.0,
        }
    }

    /// Gap between cards.
    pub fn gap(self) -> f32 {
        match self {
            Self::Compact => space::L,
            _ => space::XL,
        }
    }
}

/// Below this window width the sidebar collapses to icons on its own.
pub const SIDEBAR_AUTO_COLLAPSE: f32 = 1024.0;

/// Spacing on an 8px base (4px half steps for tight places).
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub const RADIUS_CARD: f32 = 14.0;
pub const RADIUS_CONTROL: f32 = 8.0;
pub const RADIUS_CHIP: f32 = 6.0;
pub const SIDEBAR_WIDTH: f32 = 232.0;
pub const SIDEBAR_COLLAPSED: f32 = 64.0;
pub const FONT: &str = font::TEXT;

/// Depth. Every level is the same three layers (contact, key, ambient) so
/// one level can spring into another; only the numbers change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Sits on the ground: no shadow.
    Flat,
    /// Cards at rest.
    Raised,
    /// An interactive card under the pointer.
    Lifted,
    /// Menus, popovers and anything that floats over content.
    Floating,
}

impl Elevation {
    /// (y, blur, spread, alpha) for the contact, key and ambient layers.
    fn layers(self) -> [(f32, f32, f32, f32); 3] {
        match self {
            Self::Flat => [(0., 0., 0., 0.), (0., 0., 0., 0.), (0., 0., 0., 0.)],
            Self::Raised => [(1., 2., 0., 0.05), (2., 6., -1., 0.04), (8., 24., -6., 0.07)],
            Self::Lifted => [(1., 2., 0., 0.06), (4., 10., -2., 0.06), (16., 36., -10., 0.12)],
            Self::Floating => [(1., 2., 0., 0.08), (8., 20., -4., 0.12), (24., 56., -12., 0.20)],
        }
    }
}

/// The shadow stack for a level.
pub fn elevation(p: &Palette, level: Elevation) -> Vec<BoxShadow> {
    elevation_between(p, level, level, 0.0)
}

/// The shadow stack part way (`t`, 0..1) from one level to another. Used
/// by springs, so it accepts slight overshoot.
pub fn elevation_between(p: &Palette, from: Elevation, to: Elevation, t: f32) -> Vec<BoxShadow> {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    let mut out: Vec<BoxShadow> = from
        .layers()
        .iter()
        .zip(to.layers().iter())
        .map(|(a, b)| {
            let mut c = p.shadow_ink;
            c.a = (lerp(a.3, b.3) * p.shadow_gain).clamp(0.0, 0.6);
            BoxShadow::new(px(0.), px(lerp(a.0, b.0)), c).blur_radius(px(lerp(a.1, b.1).max(0.))).spread_radius(px(lerp(a.2, b.2)))
        })
        .collect();
    if p.highlight.a > 0.0 && to != Elevation::Flat {
        out.push(BoxShadow::new(px(0.), px(1.), p.highlight).inset());
    }
    out
}

/// Motion. Springs, not durations: they pick up from wherever an
/// interrupted transition was, so nothing snaps. Values follow
/// interior.dev's tuning; all are slightly over-damped (no bounce) and
/// settle in roughly 150–250ms. GPUI renders the end state when the system
/// asks for reduced motion.
pub mod motion {
    use gpui_kit::SpringConfig;

    /// Hover and press feedback on controls.
    pub const CONTROL: SpringConfig = SpringConfig::new(520.0, 34.0, 0.45);
    /// Something moving to a new place (segmented thumb, chevrons).
    pub const GLIDE: SpringConfig = SpringConfig::new(520.0, 40.0, 0.75);
    /// Floating layers arriving.
    pub const RISE: SpringConfig = SpringConfig::new(560.0, 34.0, 0.6);
    /// Slow color and elevation changes on large surfaces.
    pub const FADE: SpringConfig = SpringConfig::new(260.0, 34.0, 0.8);
    /// How far a floating layer travels as it arrives (px).
    pub const RISE_DISTANCE: f32 = 6.0;
}

pub fn tabular() -> FontFeatures {
    FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1), ("cv11".into(), 1)]))
}

/// Loads bundled fonts and pushes the palette into GPUI Component's theme so
/// inputs, menus and dialogs match the app.
pub fn install(pref: ThemePref, cx: &mut App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-Medium.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/InterDisplay-Medium.ttf")),
        std::borrow::Cow::Borrowed(include_bytes!("../../assets/fonts/InterDisplay-SemiBold.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::warn!("could not load bundled fonts: {e}");
    }
    apply(pref, cx);
}

pub fn apply(pref: ThemePref, cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    let p = Palette::for_pref(pref);
    cx.set_global(p);
    Theme::change(if p.dark { ThemeMode::Dark } else { ThemeMode::Light }, None, cx);
    // `Theme::update` reconciles the semantic tokens components read with
    // the colors edited here; mutating the global directly would not.
    Theme::update(cx, |t| {
        t.font_family = FONT.into();
        t.font_size = px(size::BODY);
        t.radius = px(RADIUS_CONTROL);
        t.radius_lg = px(RADIUS_CARD);
        t.shadow = false;
        // Library transitions (dialogs, menus) use the same curves as ours:
        // a long exponential settle in, a quick accelerate out.
        if let (Ok(enter), Ok(exit)) =
            (gpui_kit::base::Easing::cubic_bezier(0.23, 1.0, 0.32, 1.0), gpui_kit::base::Easing::cubic_bezier(0.4, 0.0, 1.0, 1.0))
        {
            t.motion.easing_enter = enter;
            t.motion.easing_exit = exit;
        }
        let c = &mut t.colors;
        c.background = p.surface;
        c.foreground = p.text;
        c.border = p.border;
        c.input = p.control;
        c.ring = p.accent;
        c.caret = p.accent;
        c.selection = {
            let mut s = p.accent;
            s.a = 0.18;
            s
        };
        c.muted = p.neutral_tint;
        c.muted_foreground = p.muted;
        c.accent = p.hover;
        c.accent_foreground = p.text;
        c.primary = p.accent;
        c.primary_hover = p.accent_hover;
        c.primary_active = p.accent_hover;
        c.primary_foreground = p.on_accent;
        c.button_primary = p.accent;
        c.button_primary_hover = p.accent_hover;
        c.button_primary_active = p.accent_hover;
        c.button_primary_foreground = p.on_accent;
        c.secondary = p.surface;
        c.secondary_hover = p.hover;
        c.secondary_active = p.active;
        c.secondary_foreground = p.text;
        c.button_secondary = p.surface;
        c.button_secondary_hover = p.hover;
        c.button_secondary_active = p.active;
        c.button_secondary_foreground = p.text;
        c.danger = p.negative;
        c.danger_foreground = p.on_accent;
        c.popover = p.surface;
        c.popover_foreground = p.text;
        c.list_hover = p.hover;
        c.list_active = p.active;
        c.list_active_border = p.active;
        c.overlay = p.overlay;
        c.scrollbar = gpui_kit::transparent_black();
        c.window_border = p.border;
        c.link = p.accent_text;
        c.link_hover = p.accent;
    });
}
