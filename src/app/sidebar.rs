use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Route, VzApp};
use crate::i18n::t;
use crate::tr;
use crate::ui::components::{Tone, icon, pill};
use crate::ui::kit::interact::{faded, hover_state, scoped};
use crate::ui::kit::{Button, ClickHandler};
use crate::ui::theme::{Palette, RADIUS_CONTROL, SIDEBAR_COLLAPSED, SIDEBAR_WIDTH, Typeset as _, motion, palette, space, text};

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

/// One sidebar destination: icon, label and an optional badge (just the
/// icon when collapsed). The hover fill springs in like the kit's rows.
#[derive(IntoElement)]
struct NavItem {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    tip: SharedString,
    active: bool,
    collapsed: bool,
    badge: Option<AnyElement>,
    on_click: ClickHandler,
}

impl NavItem {
    fn new(
        id: impl Into<ElementId>,
        icon: IconName,
        label: impl Into<SharedString>,
        tip: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            tip: tip.into(),
            active: false,
            collapsed: false,
            badge: None,
            on_click: Rc::new(on_click),
        }
    }

    fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    fn badge(mut self, badge: Option<impl IntoElement>) -> Self {
        self.badge = badge.map(|b| b.into_any_element());
        self
    }
}

impl RenderOnce for NavItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = palette(cx);
        let (hovered, on_hover) = hover_state(&self.id, window, cx);
        let (active, tip, f) = (self.active, self.tip, self.on_click);
        let ink = if active { p.text } else { p.muted };
        h_flex()
            .id(self.id.clone())
            .h(px(36.))
            .mx(px(10.))
            .px(px(10.))
            .gap(px(10.))
            .rounded(px(RADIUS_CONTROL))
            .cursor_pointer()
            .when(self.collapsed, |d| d.justify_center().px_0())
            .child(icon(self.icon, ink, 16.))
            .when(!self.collapsed, |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .typeset(if active { text::BODY_STRONG } else { text::BODY })
                        .text_color(ink)
                        .child(self.label),
                )
                .children(self.badge)
            })
            .on_hover(on_hover)
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(move |e, w, cx| f(e, w, cx))
            .with_spring(scoped(&self.id, "fill"), SpringAnimation::new(motion::CONTROL).to(hovered && !active), move |d, t| {
                d.bg(if active { p.active } else { faded(p.hover, t.0) })
            })
    }
}

impl VzApp {
    pub(crate) fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let collapsed = self.sidebar_collapsed(window);
        let section = self.route.section();
        let summary = self.snap.book.summary(self.today);
        let overdue = summary.overdue_loans;

        let business = crate::APP_NAME;
        let items = nav();
        let nav = items.iter().map(|(route, icon_name, label, keys)| {
            let route = *route;
            let badge = (route == Route::Loans && overdue > 0).then(|| pill(&p, overdue.to_string(), Tone::Negative).h(px(20.)).px(px(6.)));
            NavItem::new(
                SharedString::from(format!("nav-{:?}", route)),
                *icon_name,
                label.to_string(),
                format!("{label}  {keys}"),
                cx.listener(move |this, _, window, cx| this.navigate(route, window, cx)),
            )
            .active(section == route)
            .collapsed(collapsed)
            .badge(badge)
        });

        let backup_label = match &self.settings().last_backup_at {
            Some(when) => tr!("Last backup {when}", "Última copia {when}"),
            None => t("Not backed up yet", "Sin copia de seguridad").to_string(),
        };
        let backup_tone = if self.settings().last_backup_at.is_some() { p.faint } else { p.warning };
        let toggle = |id: &'static str, icon_name: IconName, tip: &'static str| {
            Button::new(id)
                .ghost()
                .small()
                .icon(icon_name)
                .tooltip(tip)
                .on_click(cx.listener(|this, _, w, cx| this.toggle_sidebar(&super::ToggleSidebar, w, cx)))
        };

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
                        d.child(div().flex_1().min_w_0().line_clamp(2).typeset(text::HEADING).text_color(p.text).child(business))
                            .child(toggle("collapse", IconName::PanelLeftClose, t("Collapse sidebar  Ctrl+B", "Ocultar menú  Ctrl+B")))
                    }),
            )
            .when(collapsed, |d| {
                d.child(h_flex().justify_center().pb(px(space::S)).child(toggle(
                    "expand",
                    IconName::PanelLeftOpen,
                    t("Expand sidebar  Ctrl+B", "Mostrar menú  Ctrl+B"),
                )))
            })
            .child(v_flex().gap(px(2.)).pt(px(space::XS)).children(nav))
            .child(div().flex_1())
            .child(
                v_flex()
                    .gap(px(2.))
                    .pb(px(space::L))
                    .when_some(collapsed.then(|| self.sync_look()).flatten(), |d, (icon_name, label, _)| {
                        d.child(
                            NavItem::new(
                                "sync",
                                icon_name,
                                label.clone(),
                                tr!("Cloud sync: {label}", "Sincronización: {label}"),
                                cx.listener(|this, _, w, cx| this.navigate(Route::Settings, w, cx)),
                            )
                            .collapsed(true),
                        )
                    })
                    .child(
                        NavItem::new(
                            "backup",
                            IconName::DatabaseBackup,
                            t("Back up database", "Copia de seguridad"),
                            t("Back up database  Ctrl+S", "Copia de seguridad  Ctrl+S"),
                            cx.listener(|this, _, w, cx| this.backup_now(&super::BackupNow, w, cx)),
                        )
                        .collapsed(collapsed),
                    )
                    .when(!collapsed, |d| {
                        d.child(
                            v_flex()
                                .mx(px(20.))
                                .mt(px(10.))
                                .pt(px(space::M))
                                .border_t_1()
                                .border_color(p.border)
                                .gap(px(2.))
                                .typeset(text::CAPTION)
                                .text_color(p.faint)
                                // Signed in, the sync status takes the place of "stored on this PC".
                                .child(match self.sync_chip("sidebar-sync", cx) {
                                    Some(chip) => h_flex().pb(px(space::XS)).child(chip).into_any_element(),
                                    None => h_flex()
                                        .gap(px(6.))
                                        .child(icon(IconName::ShieldCheck, p.faint, 13.))
                                        .child(t("Private · stored on this PC", "Privado · guardado en esta PC"))
                                        .into_any_element(),
                                })
                                .child(div().text_color(backup_tone).child(backup_label))
                                .child(format!("{} {}", t("Version", "Versión"), env!("CARGO_PKG_VERSION"))),
                        )
                    }),
            )
            .into_any_element()
    }
}
