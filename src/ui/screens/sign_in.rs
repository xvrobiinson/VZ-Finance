//! The sign-in screen, the same design as the PWA's: one large rounded frame
//! on a soft backdrop. A dark showcase panel on the left (a headline, two
//! faint rings and a drawn phone with a made-up week of collections) and the
//! form panel on the right, overlapping it: the brand, "Back to the app", a
//! big title, the email-code steps and "Continue with Google". Narrow windows
//! show the form panel alone. The work is in `app::cloud`.

use std::sync::{Arc, LazyLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::VzApp;
use crate::i18n::t;
use crate::tr;
use crate::ui::components::icon;
use crate::ui::kit::Button;
use crate::ui::theme::{Elevation, Palette, Typeset as _, elevation, palette, space, tabular, text};

/// The frame's largest size and corner radius.
const FRAME_W: f32 = 1080.0;
const FRAME_H: f32 = 680.0;
const FRAME_R: f32 = 34.0;
/// How far the form panel overlaps the showcase.
const OVERLAP: f32 = 28.0;
/// Below this window width the showcase is left out.
const SHOWCASE_MIN_W: f32 = 900.0;
const FORM_W: f32 = 360.0;

/// Google's "G" in its own colors, as Google's sign-in branding asks.
static GOOGLE_G: LazyLock<Arc<Image>> =
    LazyLock::new(|| Arc::new(Image::from_bytes(ImageFormat::Svg, include_bytes!("../../../assets/google-g.svg").to_vec())));

/// The made-up week on the drawn phone (bar heights, 0–1); Friday stands out.
const WEEK: [f32; 7] = [0.62, 0.48, 0.80, 0.55, 0.92, 0.70, 0.38];

/// The drawn phone: decoration only, always in the light theme's colors, as a
/// phone screen would be.
fn phone() -> Div {
    let l = Palette::light();
    let d = Palette::dark();
    let days: Vec<char> = t("MTWTFSS", "LMMJVSD").chars().collect();
    let bars = WEEK.iter().enumerate().map(|(i, &h)| {
        v_flex()
            .flex_1()
            .h_full()
            .justify_end()
            .items_center()
            .gap(px(4.))
            .child(div().w_full().h(relative(h * 0.82)).rounded_t(px(6.)).rounded_b(px(3.)).bg(if i == 4 {
                l.accent
            } else {
                l.accent_tint
            }))
            .child(div().typeset(text::MICRO).text_color(l.faint).child(days.get(i).copied().unwrap_or(' ').to_string()))
    });
    let tile = |label: &'static str, value: &'static str, tone: Hsla| {
        v_flex()
            .flex_1()
            .p(px(10.))
            .rounded(px(14.))
            .bg(l.surface)
            .shadow(elevation(&l, Elevation::Raised))
            .child(div().typeset(text::MICRO).text_color(l.muted).child(label))
            .child(div().typeset(text::FIGURE).font_features(tabular()).text_color(tone).child(value))
    };
    div()
        .w(px(228.))
        .h(px(430.))
        .p(px(9.))
        .rounded(px(42.))
        .bg(d.raised)
        .border_1()
        .border_color(d.control)
        .shadow(elevation(&d, Elevation::Floating))
        .child(
            v_flex()
                .size_full()
                .relative()
                .rounded(px(34.))
                .bg(l.ground)
                .px(px(16.))
                .pt(px(42.))
                .gap(px(4.))
                .overflow_hidden()
                .child(div().absolute().top(px(10.)).left(px(68.)).w(px(76.)).h(px(22.)).rounded(px(12.)).bg(d.ground))
                .child(div().typeset(text::MICRO).text_color(l.muted).child(t("Collected this week", "Cobrado esta semana")))
                .child(div().typeset(text::FIGURE).font_features(tabular()).text_color(l.text).child("RD$ 89,700"))
                .child(h_flex().h(px(130.)).mt(px(8.)).mb(px(12.)).gap(px(7.)).items_end().children(bars))
                .child(h_flex().gap(px(8.)).child(tile(t("Active loans", "Préstamos activos"), "24", l.text)).child(tile(
                    t("Overdue", "Atrasados"),
                    "3",
                    l.negative,
                ))),
        )
}

/// The left panel.
fn showcase(p: &Palette, h: f32) -> Div {
    let ring = |d: f32| div().absolute().size(px(d)).rounded_full().border_1().border_color(p.hero_line);
    v_flex()
        .relative()
        .flex_1()
        .h_full()
        .overflow_hidden()
        .rounded_l(px(FRAME_R))
        .pl(px(40.))
        .pr(px(40. + OVERLAP))
        .pt(px(44.))
        .items_center()
        .child(
            div()
                .typeset(text::CAPTION)
                .text_color(p.hero_muted)
                .child(t("Your lending business, on every device.", "Tu negocio de préstamos, en todos tus dispositivos.")),
        )
        // The rings sit behind the headline.
        .child(
            div()
                .absolute()
                .top(px(h * 0.33 - 171.))
                .left_0()
                .right(px(OVERLAP))
                .h(px(342.))
                .flex()
                .items_center()
                .justify_center()
                .child(ring(342.))
                .child(ring(230.)),
        )
        .child(
            div()
                .mt(px(h * 0.33 - 150.))
                .max_w(px(290.))
                .text_center()
                .typeset(text::HERO)
                .text_color(p.hero_text)
                .child(t("Your loans, in order", "Tus préstamos, en orden")),
        )
        .child(div().absolute().bottom(px(-120.)).left_0().right(px(OVERLAP)).flex().justify_center().child(phone()))
}

impl VzApp {
    pub(crate) fn render_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let size = window.viewport_size();
        let (w, h) = (f32::from(size.width), f32::from(size.height));
        let wide = w >= SHOWCASE_MIN_W;
        let s = &self.sign_in;
        let error = s.error.clone().map(|e| div().text_color(p.negative).typeset(text::CAPTION).child(e));

        let (title, sub, form) = match s.code_sent_to.clone() {
            None => {
                let google = match &s.google {
                    None => Button::new("sign-in-google")
                        .prefix(img(GOOGLE_G.clone()).size(px(18.)))
                        .label(t("Continue with Google", "Continuar con Google"))
                        .wide()
                        .on_click(cx.listener(|this, _, w, cx| this.sign_in_with_google(w, cx)))
                        .into_any_element(),
                    Some(_) => v_flex()
                        .gap(px(space::S))
                        .items_center()
                        .child(
                            Button::new("sign-in-google-waiting")
                                .prefix(img(GOOGLE_G.clone()).size(px(18.)))
                                .label(t("Waiting for your browser…", "Esperando el navegador…"))
                                .loading(true)
                                .wide(),
                        )
                        .child(
                            Button::new("sign-in-google-cancel")
                                .ghost()
                                .small()
                                .label(t("Cancel", "Cancelar"))
                                .on_click(cx.listener(|this, _, _, cx| this.cancel_google(cx))),
                        )
                        .into_any_element(),
                };
                let form = v_flex()
                    .gap(px(space::M))
                    .child(Input::new(&s.email).large().prefix(icon(IconName::Mail, p.faint, 16.)))
                    .children(error)
                    .child(
                        Button::new("sign-in-send-code")
                            .primary()
                            .label(t("Send code", "Enviar código"))
                            .loading(s.busy)
                            .wide()
                            .on_click(cx.listener(|this, _, w, cx| this.send_sign_in_code(w, cx))),
                    )
                    .child(
                        h_flex()
                            .gap(px(space::M))
                            .typeset(text::CAPTION)
                            .text_color(p.faint)
                            .child(div().flex_1().h(px(1.)).bg(p.border))
                            .child(t("Or continue with", "O continúa con"))
                            .child(div().flex_1().h(px(1.)).bg(p.border)),
                    )
                    .child(google);
                (
                    t("Sign in", "Iniciar sesión").to_string(),
                    t(
                        "We'll email you a 6-digit code. No password to remember.",
                        "Te enviaremos un código de 6 dígitos. Sin contraseñas que recordar.",
                    )
                    .to_string(),
                    form,
                )
            }
            Some(email) => {
                let form = v_flex()
                    .gap(px(space::M))
                    .child(Input::new(&s.code).large().prefix(icon(IconName::KeyRound, p.faint, 16.)))
                    .child(div().typeset(text::CAPTION).text_color(p.muted).child(t(
                        "It expires in 10 minutes. Check your spam folder too.",
                        "Vence en 10 minutos. Revisa también el correo no deseado.",
                    )))
                    .children(error)
                    .child(
                        Button::new("sign-in-finish")
                            .primary()
                            .label(t("Sign in", "Iniciar sesión"))
                            .loading(s.busy)
                            .wide()
                            .on_click(cx.listener(|this, _, w, cx| this.finish_sign_in(w, cx))),
                    )
                    .child(
                        h_flex().justify_center().child(
                            Button::new("sign-in-other-email")
                                .ghost()
                                .small()
                                .label(t("Use a different email", "Usar otro correo"))
                                .on_click(cx.listener(|this, _, _, cx| this.restart_sign_in(cx))),
                        ),
                    );
                (
                    t("Enter your code", "Escribe tu código").to_string(),
                    tr!("We sent a 6-digit code to {email}.", "Enviamos un código de 6 dígitos a {email}."),
                    form,
                )
            }
        };

        let year = chrono::Local::now().format("%Y").to_string();
        let panel = v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .when(wide, |d| {
                d.ml(px(-OVERLAP))
                    .rounded(px(FRAME_R))
                    .shadow(vec![BoxShadow::new(px(-12.), px(0.), p.shadow_ink.opacity(0.35)).blur_radius(px(40.)).spread_radius(px(-18.))])
            })
            .bg(p.surface)
            .px(px(if wide { 48. } else { space::XL }))
            .pt(px(36.))
            .pb(px(28.))
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(crate::app::mark(&p))
                            .child(div().typeset(text::HEADING).text_color(p.text).child(crate::APP_NAME)),
                    )
                    .child(
                        Button::new("sign-in-back")
                            .ghost()
                            .icon(IconName::ArrowLeft)
                            .label(t("Back to the app", "Volver a la app"))
                            .on_click(cx.listener(|this, _, w, cx| this.close_sign_in(w, cx))),
                    ),
            )
            .child(
                div().id("sign-in-scroll").flex_1().min_h_0().overflow_y_scroll().child(
                    v_flex().min_h_full().justify_center().items_center().py(px(space::XL)).child(
                        v_flex()
                            .w_full()
                            .max_w(px(FORM_W))
                            .gap(px(space::L))
                            .child(
                                v_flex()
                                    .gap(px(space::S))
                                    .mb(px(space::S))
                                    .child(div().typeset(text::DISPLAY).text_color(p.text).child(title))
                                    .child(div().text_color(p.muted).child(sub)),
                            )
                            .child(form)
                            .child(div().typeset(text::CAPTION).text_color(p.faint).child(t(
                                "Without signing in, the app works only with the books on this PC.",
                                "Sin iniciar sesión, la app trabaja solo con los registros de esta PC.",
                            ))),
                    ),
                ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .gap(px(space::M))
                    .typeset(text::CAPTION)
                    .text_color(p.faint)
                    .child(format!("© {year} {}", crate::APP_NAME))
                    .child(format!("{} {}", t("Version", "Versión"), env!("CARGO_PKG_VERSION"))),
            );

        let frame = h_flex()
            .relative()
            .when(wide, |d| {
                d.w(px(FRAME_W.min(w - 48.)))
                    .h(px(FRAME_H.min(h - 48.)))
                    .rounded(px(FRAME_R))
                    .bg(p.hero)
                    .shadow(elevation(&p, Elevation::Floating))
                    .child(showcase(&p, FRAME_H.min(h - 48.)))
            })
            .when(!wide, |d| d.size_full())
            .child(panel);

        div()
            .id("sign-in")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(if wide { p.backdrop } else { p.surface })
            .text_color(p.text)
            .typeset(text::BODY)
            .child(frame)
            .into_any_element()
    }
}
