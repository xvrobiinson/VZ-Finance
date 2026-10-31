//! The sign-in screen: a sky (night in dark mode) with soft clouds and two
//! faint arcs, the brand in the corner and one glass card holding the email
//! code steps and "Continue with Google". It takes the whole window while
//! open; "Back to the app" closes it. The work is in `app::cloud`.

use std::sync::{Arc, LazyLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::app::VzApp;
use crate::i18n::t;
use crate::tr;
use crate::ui::components::icon;
use crate::ui::kit::Button;
use crate::ui::kit::interact::faded;
use crate::ui::theme::{Elevation, Palette, RADIUS_CONTROL, Typeset as _, elevation, palette, space, text};

const CARD_W: f32 = 380.0;

/// Google's "G" in its own colors, as Google's sign-in branding asks.
static GOOGLE_G: LazyLock<Arc<Image>> =
    LazyLock::new(|| Arc::new(Image::from_bytes(ImageFormat::Svg, include_bytes!("../../../assets/google-g.svg").to_vec())));

/// Soft cloud puffs along the bottom: (center across the width 0–1, height
/// of the center above the bottom edge, diameter).
const PUFFS: [(f32, f32, f32); 9] = [
    (0.02, 40., 260.),
    (0.16, -10., 300.),
    (0.30, 30., 220.),
    (0.44, -40., 340.),
    (0.58, 10., 260.),
    (0.72, -30., 320.),
    (0.86, 40., 240.),
    (0.98, -20., 300.),
    (0.50, -120., 520.),
];

/// The sky behind the card (also tried behind Overview), `w` × `h` px.
pub(crate) fn sky(p: &Palette, w: f32, h: f32) -> Div {
    let (cx_, cy) = (w / 2., h * 0.58);
    let arc =
        |d: f32| div().absolute().left(px(cx_ - d / 2.)).top(px(cy - d / 2.)).size(px(d)).rounded_full().border_1().border_color(p.arc);
    let puffs = PUFFS.iter().map(|&(x, lift, d)| {
        div()
            .absolute()
            .left(px(x * w - d / 2.))
            .top(px(h - lift - d / 2.))
            .size(px(d))
            .rounded_full()
            .shadow(vec![BoxShadow::new(px(0.), px(0.), p.cloud).blur_radius(px(60.)).spread_radius(px(20.))])
    });
    div()
        .absolute()
        .inset_0()
        .overflow_hidden()
        .bg(linear_gradient(180., linear_color_stop(p.sky_top, 0.), linear_color_stop(p.sky_low, 1.)))
        .child(arc(1500.))
        .child(arc(1920.))
        .children(puffs)
}

/// The square tile with the step's icon at the top of the card.
fn tile(p: &Palette, name: IconName) -> Div {
    div()
        .size(px(52.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(14.))
        .bg(p.surface)
        .border_1()
        .border_color(p.glass_edge)
        .shadow(elevation(p, Elevation::Raised))
        .child(icon(name, p.text, 22.))
}

impl VzApp {
    pub(crate) fn render_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let size = window.viewport_size();
        let (w, h) = (f32::from(size.width), f32::from(size.height));
        let s = &self.sign_in;
        let error = s.error.clone().map(|e| div().text_color(p.negative).typeset(text::CAPTION).child(e));

        let (glyph, title, sub, form) = match s.code_sent_to.clone() {
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
                    IconName::LogIn,
                    t("Sign in to Acervus", "Inicia sesión en Acervus").to_string(),
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
                    IconName::KeyRound,
                    t("Enter your code", "Escribe tu código").to_string(),
                    tr!("We sent a 6-digit code to {email}.", "Enviamos un código de 6 dígitos a {email}."),
                    form,
                )
            }
        };

        let card = v_flex()
            .w(px(CARD_W.min(w - 2. * space::L)))
            .px(px(28.))
            .pt(px(32.))
            .pb(px(28.))
            .gap(px(space::L))
            .rounded(px(22.))
            .border_1()
            .border_color(p.glass_edge)
            .bg(linear_gradient(180., linear_color_stop(p.glass_top, 0.), linear_color_stop(p.glass, 0.4)))
            .shadow(elevation(&p, Elevation::Floating))
            .child(
                v_flex()
                    .items_center()
                    .gap(px(6.))
                    .child(tile(&p, glyph))
                    .child(div().mt(px(10.)).typeset(text::TITLE).text_color(p.text).child(title))
                    .child(div().text_center().text_color(p.muted).max_w(px(290.)).child(sub)),
            )
            .child(form)
            .child(div().text_center().typeset(text::CAPTION).text_color(p.faint).child(t(
                "Without signing in, the app works only with the books on this PC.",
                "Sin iniciar sesión, la app trabaja solo con los registros de esta PC.",
            )));

        let top = h_flex()
            .justify_between()
            .px(px(space::XXL))
            .pt(px(space::L))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(crate::app::mark(&p))
                    .child(div().typeset(text::HEADING).text_color(p.text).child(crate::APP_NAME)),
            )
            .child(
                div().rounded(px(RADIUS_CONTROL)).bg(faded(p.surface, 0.35)).child(
                    Button::new("sign-in-back")
                        .ghost()
                        .icon(IconName::ArrowLeft)
                        .label(t("Back to the app", "Volver a la app"))
                        .on_click(cx.listener(|this, _, w, cx| this.close_sign_in(w, cx))),
                ),
            );

        div()
            .id("sign-in")
            .size_full()
            .relative()
            .text_color(p.text)
            .typeset(text::BODY)
            .child(sky(&p, w, h))
            .child(
                v_flex().absolute().inset_0().child(top).child(
                    div()
                        .id("sign-in-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(v_flex().min_h_full().items_center().justify_center().py(px(space::XL)).child(card)),
                ),
            )
            .into_any_element()
    }
}
