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
    pub neutral_tint: Hsla,
    pub grid: Hsla,
    pub chart_reference: Hsla,
    pub chart_secondary: Hsla,
    pub shadow: Hsla,
    pub overlay: Hsla,
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
            ground: hex(0xF7F8FA),
            surface: hex(0xFFFFFF),
            raised: hex(0xF9FAFB),
            hover: hex(0xF3F4F7),
            active: hex(0xEEF0F4),
            border: hex(0xE9EBEF),
            rule: hex(0xF0F1F4),
            control: hex(0xDCDFE5),
            text: hex(0x17191C),
            muted: hex(0x666C77),
            faint: hex(0x737985),
            accent: hex(0x365CF5),
            accent_hover: hex(0x2C4FE0),
            accent_tint: hex(0xEEF2FF),
            accent_text: hex(0x2B4BD6),
            on_accent: hex(0xFFFFFF),
            positive: hex(0x137A60),
            positive_tint: hex(0xE7F5F0),
            warning: hex(0x9A5F0E),
            warning_tint: hex(0xFDF3E3),
            negative: hex(0xC2353D),
            negative_tint: hex(0xFDECEC),
            neutral_tint: hex(0xF1F2F5),
            grid: hex(0xEDEEF2),
            chart_reference: hex(0x9AA0AA),
            chart_secondary: hex(0x16856A),
            shadow: hexa(0x141E32, 0.045),
            overlay: hexa(0x0B1020, 0.32),
            dark: false,
        }
    }

    pub fn dark() -> Self {
        Self {
            ground: hex(0x0E0F11),
            surface: hex(0x17181B),
            raised: hex(0x1C1D21),
            hover: hex(0x212328),
            active: hex(0x272930),
            border: hex(0x26282D),
            rule: hex(0x212328),
            control: hex(0x3A3D44),
            text: hex(0xF2F3F5),
            muted: hex(0xA6ABB4),
            faint: hex(0x9298A2),
            accent: hex(0x5B7CFA),
            accent_hover: hex(0x6E8BFB),
            accent_tint: hex(0x1C2340),
            accent_text: hex(0x8EA6FF),
            on_accent: hex(0xFFFFFF),
            positive: hex(0x3CC195),
            positive_tint: hex(0x132A23),
            warning: hex(0xE6AE5A),
            warning_tint: hex(0x2E2414),
            negative: hex(0xF07077),
            negative_tint: hex(0x33191B),
            neutral_tint: hex(0x23252A),
            grid: hex(0x24262B),
            chart_reference: hex(0x6B717B),
            chart_secondary: hex(0x3CC195),
            shadow: hexa(0x000000, 0.25),
            overlay: hexa(0x000000, 0.5),
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

/// Type scale (px). Five sizes do the work; KPI figures use LEAD/FIGURE.
pub mod size {
    pub const META: f32 = 12.0;
    pub const BODY: f32 = 13.5;
    pub const SECTION: f32 = 15.0;
    pub const FIGURE: f32 = 22.0;
    pub const LEAD: f32 = 30.0;
    pub const TITLE: f32 = 22.0;
}

/// Spacing on an 8px base (4px half steps for tight places).
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

pub const RADIUS_CARD: f32 = 12.0;
pub const RADIUS_CONTROL: f32 = 8.0;
pub const RADIUS_CHIP: f32 = 6.0;
pub const SIDEBAR_WIDTH: f32 = 232.0;
pub const SIDEBAR_COLLAPSED: f32 = 64.0;
pub const FONT: &str = "Inter";

/// Card elevation: soft, diffused, barely there.
pub fn card_shadow(p: &Palette) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(px(0.), px(4.), p.shadow).blur_radius(px(18.)),
        BoxShadow::new(px(0.), px(1.), {
            let mut c = p.shadow;
            c.a *= 0.8;
            c
        })
        .blur_radius(px(2.)),
    ]
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
