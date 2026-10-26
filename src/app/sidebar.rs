use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Route, VzApp};
use crate::i18n::t;
use crate::tr;
use crate::ui::components::{Tone, icon, pill};
use crate::ui::theme::{Palette, SIDEBAR_COLLAPSED, SIDEBAR_WIDTH, palette, size, space};

fn nav() -> [(Route, IconName, &'static str, &'static str); 6] {
    [
        (Route::Overview, IconName::LayoutDashboard, t("Overview", "Resumen"), "Ctrl+1"),
        (Route::Clients, IconName::Users, t("Clients", "Clientes"), "Ctrl+2"),
        (Route::Loans, IconName::HandCoins, t("Loans", "Préstamos"), "Ctrl+3"),
        (Route::Payments, IconName::ArrowLeftRight, t("Payments", "Pagos"), "Ctrl+4"),
        (Route::Analytics, IconName::ChartLine, t("Portfolio Analytics", "Análisis"), "Ctrl+5"),
        (Route::Settings, IconName::Settings, t("Settings", "Configuración"), "Ctrl+6"),
    ]
}

/// The product mark: three ascending bars, the shape of a growing portfolio.
fn mark(p: &Palette) -> Div {
    let mut faded = p.accent;
    faded.a = 0.45;
    let mut mid = p.accent;
    mid.a = 0.72;
    h_flex()
        .flex_none()
        .size(px(24.))
        .rounded(px(7.))
        .bg(p.accent_tint)
        .items_end()
        .justify_center()
        .gap(px(2.))
        .pb(px(6.))
        .child(div().w(px(3.)).h(px(5.)).rounded(px(1.)).bg(faded))
        .child(div().w(px(3.)).h(px(8.)).rounded(px(1.)).bg(mid))
        .child(div().w(px(3.)).h(px(12.)).rounded(px(1.)).bg(p.accent))
}

impl VzApp {
    pub(crate) fn render_sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let collapsed = self.settings().sidebar_collapsed;
        let section = self.route.section();
        let summary = self.snap.book.summary(self.today);
        let overdue = summary.overdue_loans;

        let business = crate::APP_NAME;
        let items = nav();
        let nav = items.iter().map(|(route, icon_name, label, keys)| {
            let active = section == *route;
            let route = *route;
            let tip = format!("{label}  {keys}");
            let badge = (route == Route::Loans && overdue > 0).then(|| pill(&p, overdue.to_string(), Tone::Negative).h(px(20.)).px(px(6.)));
            h_flex()
                .id(SharedString::from(format!("nav-{:?}", route)))
                .h(px(36.))
                .mx(px(10.))
                .px(px(10.))
                .gap(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(collapsed, |d| d.justify_center().px_0())
                .when(active, |d| d.bg(p.active))
                .when(!active, |d| d.hover(|s| s.bg(p.hover)))
                .child(icon(*icon_name, if active { p.text } else { p.muted }, 16.))
                .when(!collapsed, |d| {
                    d.child(
                        div()
                            .flex_1()
                            .text_color(if active { p.text } else { p.muted })
                            .font_weight(if active { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                            .whitespace_nowrap()
                            .child(label.to_string()),
                    )
                    .children(badge)
                })
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| this.navigate(route, window, cx)))
        });

        let backup_label = match &self.settings().last_backup_at {
            Some(when) => tr!("Last backup {when}", "Última copia {when}"),
            None => t("Not backed up yet", "Sin copia de seguridad").to_string(),
        };
        let backup_tone = if self.settings().last_backup_at.is_some() { p.faint } else { p.warning };

        v_flex()
            .flex_none()
            .w(px(if collapsed { SIDEBAR_COLLAPSED } else { SIDEBAR_WIDTH }))
            .h_full()
            .bg(p.surface)
            .border_r_1()
            .border_color(p.border)
            .child(
                h_flex()
                    .h(px(64.))
                    .px(px(if collapsed { 0. } else { 20. }))
                    .when(collapsed, |d| d.justify_center())
                    .gap(px(10.))
                    .child(match &self.logo {
                        Some(logo) => div()
                            .flex_none()
                            .size(px(32.))
                            .rounded(px(7.))
                            .overflow_hidden()
                            .child(img(logo.clone()).size_full().object_fit(ObjectFit::Contain))
                            .into_any_element(),
                        None => mark(&p).into_any_element(),
                    })
                    .when(!collapsed, |d| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .line_clamp(2)
                                .text_size(px(size::SECTION))
                                .line_height(px(19.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(business),
                        )
                    })
                    .when(!collapsed, |d| {
                        d.child(
                            h_flex()
                                .id("collapse")
                                .size(px(28.))
                                .rounded(px(6.))
                                .justify_center()
                                .cursor_pointer()
                                .hover(|s| s.bg(p.hover))
                                .child(icon(IconName::PanelLeftClose, p.faint, 16.))
                                .tooltip(|window, cx| Tooltip::new(t("Collapse sidebar  Ctrl+B", "Ocultar menú  Ctrl+B")).build(window, cx))
                                .on_click(cx.listener(|this, _, w, cx| this.toggle_sidebar(&super::ToggleSidebar, w, cx))),
                        )
                    }),
            )
            .when(collapsed, |d| {
                d.child(
                    h_flex().justify_center().pb(px(8.)).child(
                        h_flex()
                            .id("expand")
                            .size(px(28.))
                            .rounded(px(6.))
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(p.hover))
                            .child(icon(IconName::PanelLeftOpen, p.faint, 16.))
                            .tooltip(|window, cx| Tooltip::new(t("Expand sidebar  Ctrl+B", "Mostrar menú  Ctrl+B")).build(window, cx))
                            .on_click(cx.listener(|this, _, w, cx| this.toggle_sidebar(&super::ToggleSidebar, w, cx))),
                    ),
                )
            })
            .child(v_flex().gap(px(2.)).pt(px(4.)).children(nav))
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap(px(2.))
                    .pb(px(space::L))
                    .child(
                        h_flex()
                            .id("backup")
                            .h(px(36.))
                            .mx(px(10.))
                            .px(px(10.))
                            .gap(px(10.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .when(collapsed, |d| d.justify_center().px_0())
                            .hover(|s| s.bg(p.hover))
                            .child(icon(IconName::DatabaseBackup, p.muted, 16.))
                            .when(!collapsed, |d| d.child(div().text_color(p.muted).child(t("Back up database", "Copia de seguridad"))))
                            .tooltip(|window, cx| {
                                Tooltip::new(t("Back up database  Ctrl+S", "Copia de seguridad  Ctrl+S")).build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, w, cx| this.backup_now(&super::BackupNow, w, cx))),
                    )
                    .when(!collapsed, |d| {
                        d.child(
                            v_flex()
                                .mx(px(20.))
                                .mt(px(10.))
                                .pt(px(12.))
                                .border_t_1()
                                .border_color(p.border)
                                .gap(px(2.))
                                .child(
                                    h_flex().gap(px(6.)).child(icon(IconName::ShieldCheck, p.faint, 13.)).child(
                                        div()
                                            .text_size(px(size::META))
                                            .text_color(p.faint)
                                            .child(t("Private · stored on this PC", "Privado · guardado en esta PC")),
                                    ),
                                )
                                .child(div().text_size(px(size::META)).text_color(backup_tone).child(backup_label))
                                .child(div().text_size(px(size::META)).text_color(p.faint).child(format!(
                                    "{} {}",
                                    t("Version", "Versión"),
                                    env!("CARGO_PKG_VERSION")
                                ))),
                        )
                    }),
            )
            .into_any_element()
    }
}
