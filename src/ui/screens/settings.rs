use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::segmented;
use crate::app::VzApp;
use crate::domain::loans::InterestMethod;
use crate::domain::money::NumberStyle;
use crate::services::settings::{ChartRange, ThemePref};
use crate::ui::components::{card, icon, kbd};
use crate::ui::theme::{Palette, palette, size, space};
use crate::utils::dates::DateStyle;

fn section(p: &Palette, title: &str, description: &str, body: impl IntoElement) -> Div {
    h_flex()
        .items_start()
        .gap(px(40.))
        .py(px(space::XL + 4.))
        .border_b_1()
        .border_color(p.border)
        .child(
            v_flex()
                .w(px(280.))
                .flex_none()
                .gap(px(4.))
                .child(div().text_size(px(size::SECTION)).font_weight(FontWeight::MEDIUM).child(title.to_string()))
                .child(div().text_color(p.muted).child(description.to_string())),
        )
        .child(card(p).flex_1().min_w_0().p(px(space::XL)).gap(px(space::L)).child(body))
}

fn row(p: &Palette, title: &str, hint: Option<&str>, control: impl IntoElement) -> Div {
    h_flex()
        .justify_between()
        .gap(px(space::XL))
        .child(
            v_flex()
                .gap(px(2.))
                .min_w_0()
                .child(div().font_weight(FontWeight::MEDIUM).child(title.to_string()))
                .children(hint.map(|h| div().text_size(px(size::META)).text_color(p.muted).child(h.to_string()))),
        )
        .child(div().flex_none().child(control))
}

impl VzApp {
    pub(crate) fn render_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let s = self.settings().clone();
        let today = self.today;
        let db_path = self.store.path().to_path_buf();
        let db_dir = db_path.parent().map(|d| d.to_path_buf());

        let number_opts: Vec<(NumberStyle, &str)> =
            [NumberStyle::CommaDot, NumberStyle::DotComma, NumberStyle::SpaceComma].iter().map(|n| (*n, n.label())).collect();
        let date_labels: Vec<(DateStyle, String)> = DateStyle::ALL.iter().map(|d| (*d, d.format(today))).collect();
        let date_opts: Vec<(DateStyle, &str)> = date_labels.iter().map(|(d, l)| (*d, l.as_str())).collect();
        let method_opts: Vec<(InterestMethod, &str)> = InterestMethod::ALL.iter().map(|m| (*m, m.label())).collect();
        let range_opts: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let theme_opts = [(ThemePref::Light, "Light"), (ThemePref::Dark, "Dark")];

        let general = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                "Base currency",
                Some("Changes the symbol shown. Amounts are never converted."),
                div().w(px(260.)).child(Select::new(&self.currency_select)),
            ))
            .child(row(
                &p,
                "Number format",
                None,
                segmented(&p, "numfmt", &number_opts, s.number_style, cx, |app, v, w, cx| app.update_settings(|s| s.number_style = v, w, cx)),
            ))
            .child(row(
                &p,
                "Date format",
                None,
                segmented(&p, "datefmt", &date_opts, s.date_style, cx, |app, v, w, cx| app.update_settings(|s| s.date_style = v, w, cx)),
            ));

        let lending = v_flex().gap(px(space::L)).child(row(
            &p,
            "Default interest method",
            Some("Preselected when you create a loan"),
            segmented(&p, "method", &method_opts, s.default_method, cx, |app, v, w, cx| app.update_settings(|s| s.default_method = v, w, cx)),
        ))
        .child(
            v_flex()
                .gap(px(4.))
                .p(px(space::M))
                .rounded(px(8.))
                .bg(p.raised)
                .text_size(px(size::META + 0.5))
                .text_color(p.muted)
                .child("Payment allocation: fees first, then accrued interest, then principal. Payments above the payoff are refused so no credit balance is created.")
                .child("Interest: daily accrual on outstanding principal, Actual/365, rounded half-up to the cent per accrual period. No compounding."),
        );

        let dashboard = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                "Default chart range",
                None,
                segmented(&p, "range-set", &range_opts, s.chart_range, cx, |app, v, w, cx| app.update_settings(|s| s.chart_range = v, w, cx)),
            ))
            .child(row(
                &p,
                "Show capital invested line",
                Some("A dashed reference line on portfolio charts"),
                Switch::new("show-ref").checked(s.show_interest_line).on_click(cx.listener(|this, checked: &bool, w, cx| {
                    let v = *checked;
                    this.update_settings(|s| s.show_interest_line = v, w, cx)
                })),
            ))
            .child(row(
                &p,
                "Theme",
                Some("Light is the reference design"),
                segmented(&p, "theme", &theme_opts, s.theme, cx, |app, v, w, cx| app.update_settings(|s| s.theme = v, w, cx)),
            ));

        let path_text = db_path.display().to_string();
        let data = v_flex()
            .gap(px(space::L))
            .child(
                row(
                    &p,
                    "Database location",
                    None,
                    Button::new("open-folder").ghost().icon(IconName::FolderOpen).label("Show in Explorer").on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(d) = &db_dir {
                            cx.open_with_system(d);
                        }
                    })),
                ),
            )
            .child(div().px(px(space::M)).py(px(10.)).rounded(px(8.)).bg(p.raised).text_size(px(size::META + 0.5)).text_color(p.muted).child(path_text))
            .child(row(
                &p,
                "Back up",
                Some(match &s.last_backup_at {
                    Some(_) => "A verified copy using SQLite's online backup",
                    None => "You haven't made a backup yet",
                }),
                Button::new("backup").icon(IconName::DatabaseBackup).label("Back up now").on_click(cx.listener(|this, _, w, cx| this.backup_now(&crate::app::BackupNow, w, cx))),
            ))
            .child(row(
                &p,
                "Restore",
                Some("The backup is checked before anything is replaced; a safety copy is kept"),
                Button::new("restore").icon(IconName::ArchiveRestore).label("Restore from backup").on_click(cx.listener(|this, _, w, cx| this.choose_restore(w, cx))),
            ))
            .child(row(
                &p,
                "Export",
                Some("Clients, loans, payments and capital as CSV"),
                Button::new("export").icon(IconName::FileSpreadsheet).label("Export CSV").on_click(cx.listener(|this, _, w, cx| this.choose_export(w, cx))),
            ));

        let privacy = v_flex()
            .gap(px(space::M))
            .child(h_flex().gap(px(8.)).child(icon(IconName::ShieldCheck, p.positive, 16.)).child("Offline and private"))
            .child(div().text_color(p.muted).child(
                "VZ Finance makes no network requests, sends no telemetry and needs no account. All data stays in the database file above.",
            ))
            .child(div().text_color(p.muted).child(
                "The database is not encrypted by the app. Protect it with Windows BitLocker or device encryption and your Windows sign-in, and keep backups somewhere safe.",
            ));

        let keys = [
            ("Record payment", "Ctrl P"),
            ("New loan", "Ctrl L"),
            ("New client", "Ctrl N"),
            ("Search", "Ctrl F"),
            ("Back up", "Ctrl S"),
            ("Toggle sidebar", "Ctrl B"),
            ("Go to sections", "Ctrl 1 – 6"),
            ("Back", "Alt ←"),
        ];
        let shortcuts = v_flex().gap(px(2.)).children(keys.iter().map(|(a, k)| {
            h_flex().justify_between().min_h(px(30.)).child(div().text_color(p.muted).child(a.to_string())).child(kbd(&p, k))
        }));

        v_flex()
            .child(crate::ui::components::page_header(&p, "Settings", Some("Preferences are saved as you change them.".into_any_element()), vec![]).pb(px(space::S)))
            .child(section(&p, "General", "Currency, numbers and dates", general))
            .child(section(&p, "Lending", "Defaults and the rules every loan follows", lending))
            .child(section(&p, "Dashboard", "Charts and appearance", dashboard))
            .child(section(&p, "Data", "Your database, backups and exports", data))
            .child(section(&p, "Privacy", "How your data is handled", privacy))
            .child(section(&p, "Keyboard", "Shortcuts available everywhere", shortcuts))
            .into_any_element()
    }
}
