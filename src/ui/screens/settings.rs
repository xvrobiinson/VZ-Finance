use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::segmented;
use crate::app::VzApp;
use crate::domain::loans::{Frequency, RATE_PRESETS};
use crate::domain::money::{NumberStyle, Rate};
use crate::i18n::{Lang, t};
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
                .w(px(260.))
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
        let receipts_dir = self.store.receipts_dir();

        let lang_opts = [(Lang::En, Lang::En.label()), (Lang::Es, Lang::Es.label())];
        let number_opts: Vec<(NumberStyle, &str)> =
            [NumberStyle::CommaDot, NumberStyle::DotComma, NumberStyle::SpaceComma].iter().map(|n| (*n, n.label())).collect();
        let date_labels: Vec<(DateStyle, String)> = DateStyle::ALL.iter().map(|d| (*d, d.format(today))).collect();
        let date_opts: Vec<(DateStyle, &str)> = date_labels.iter().map(|(d, l)| (*d, l.as_str())).collect();
        let rate_labels: Vec<(Rate, String)> = RATE_PRESETS.iter().map(|r| (*r, r.percent_label())).collect();
        let rate_opts: Vec<(Rate, &str)> = rate_labels.iter().map(|(r, l)| (*r, l.as_str())).collect();
        let freq_opts: Vec<(Frequency, &str)> = Frequency::ALL.iter().map(|f| (*f, f.label())).collect();
        let range_opts: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let theme_opts = [(ThemePref::Light, t("Light", "Claro")), (ThemePref::Dark, t("Dark", "Oscuro"))];
        let name_changed = self.business_name_input.read(cx).value().trim() != s.business_name;

        let business = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Business name", "Nombre del negocio"),
                Some(t("Shown in the app and on receipts", "Aparece en la app y en los recibos")),
                h_flex().gap(px(space::S)).child(div().w(px(260.)).child(Input::new(&self.business_name_input))).child(
                    Button::new("save-name")
                        .primary()
                        .label(t("Save", "Guardar"))
                        .disabled(!name_changed)
                        .on_click(cx.listener(|this, _, w, cx| this.save_business_name(w, cx))),
                ),
            ))
            .child(row(
                &p,
                t("Logo", "Logo"),
                Some(t(
                    "Shown in the sidebar and on receipts. PNG, JPG, GIF, BMP or WebP.",
                    "Aparece en el menú y en los recibos. PNG, JPG, GIF, BMP o WebP.",
                )),
                h_flex()
                    .gap(px(space::S))
                    .child(
                        h_flex()
                            .size(px(44.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(p.border)
                            .bg(p.raised)
                            .overflow_hidden()
                            .child(match &self.logo {
                                Some(logo) => img(logo.clone()).size(px(40.)).object_fit(ObjectFit::Contain).into_any_element(),
                                None => icon(IconName::ImagePlus, p.faint, 18.).into_any_element(),
                            }),
                    )
                    .child(
                        Button::new("logo-upload")
                            .icon(IconName::Upload)
                            .label(if self.logo.is_some() { t("Change", "Cambiar") } else { t("Upload logo", "Subir logo") })
                            .on_click(cx.listener(|this, _, w, cx| this.choose_logo(w, cx))),
                    )
                    .when(self.logo.is_some(), |d| {
                        d.child(
                            Button::new("logo-remove")
                                .ghost()
                                .icon(IconName::Trash)
                                .tooltip(t("Remove logo", "Quitar logo"))
                                .on_click(cx.listener(|this, _, w, cx| this.remove_logo(w, cx))),
                        )
                    }),
            ))
            .child(row(
                &p,
                t("Language", "Idioma"),
                None,
                segmented(&p, "lang", &lang_opts, s.language, cx, |app, v, w, cx| app.update_settings(|s| s.language = v, w, cx)),
            ));

        let general = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Currency", "Moneda"),
                Some(t("Changes the symbol shown. Amounts are never converted.", "Cambia el símbolo. Los montos nunca se convierten.")),
                div().w(px(260.)).child(Select::new(&self.currency_select)),
            ))
            .child(row(
                &p,
                t("Number format", "Formato de números"),
                None,
                segmented(&p, "numfmt", &number_opts, s.number_style, cx, |app, v, w, cx| {
                    app.update_settings(|s| s.number_style = v, w, cx)
                }),
            ))
            .child(row(
                &p,
                t("Date format", "Formato de fecha"),
                None,
                segmented(&p, "datefmt", &date_opts, s.date_style, cx, |app, v, w, cx| app.update_settings(|s| s.date_style = v, w, cx)),
            ));

        let lending = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Default rate", "Tasa predeterminada"),
                Some(t("Preselected for new loans", "Se preselecciona en préstamos nuevos")),
                segmented(&p, "rate", &rate_opts, s.default_rate, cx, |app, v, w, cx| app.update_settings(|s| s.default_rate = v, w, cx)),
            ))
            .child(row(
                &p,
                t("Default payments", "Pagos predeterminados"),
                None,
                segmented(&p, "freq", &freq_opts, s.default_frequency, cx, |app, v, w, cx| app.update_settings(|s| s.default_frequency = v, w, cx)),
            ))
            .child(
                v_flex()
                    .gap(px(4.))
                    .p(px(space::M))
                    .rounded(px(8.))
                    .bg(p.raised)
                    .text_size(px(size::META + 0.5))
                    .text_color(p.muted)
                    .child(t(
                        "Each period's interest is the rate times the capital owed when the period begins. Payments cover any fee, then interest owed, then capital.",
                        "El interés de cada período es la tasa por el capital adeudado al empezar el período. Los pagos cubren primero el cargo, luego el interés y después el capital.",
                    ))
                    .child(t(
                        "Example: 1,000 at 10% is 100 of interest. Paying 600 covers the 100 and lowers the capital to 500, so the next interest is 50.",
                        "Ejemplo: 1,000 al 10% son 100 de interés. Si paga 600, cubre los 100 y el capital baja a 500, así que el próximo interés es 50.",
                    )),
            );

        let receipts = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Open the receipt after each payment", "Abrir el recibo después de cada pago"),
                Some(t("It opens in your browser, ready to print", "Se abre en tu navegador, listo para imprimir")),
                Switch::new("rcpt-after").checked(s.receipt_after_payment).on_click(cx.listener(|this, checked: &bool, w, cx| {
                    let v = *checked;
                    this.update_settings(|s| s.receipt_after_payment = v, w, cx)
                })),
            ))
            .child(row(
                &p,
                t("Open the loan receipt after each new loan", "Abrir el recibo del préstamo al crear cada préstamo"),
                Some(t(
                    "Amount, rate, payment dates and signature lines for the client",
                    "Monto, tasa, fechas de pago y líneas de firma para el cliente",
                )),
                Switch::new("rcpt-loan").checked(s.receipt_after_loan).on_click(cx.listener(|this, checked: &bool, w, cx| {
                    let v = *checked;
                    this.update_settings(|s| s.receipt_after_loan = v, w, cx)
                })),
            ))
            .child(row(
                &p,
                t("Receipts folder", "Carpeta de recibos"),
                Some(t("Every receipt is also saved here", "Cada recibo también se guarda aquí")),
                Button::new("open-receipts").ghost().icon(IconName::FolderOpen).label(t("Open folder", "Abrir carpeta")).on_click(
                    cx.listener(move |_, _, _, cx| {
                        let _ = std::fs::create_dir_all(&receipts_dir);
                        cx.open_with_system(&receipts_dir);
                    }),
                ),
            ));

        let dashboard = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Default chart range", "Rango del gráfico"),
                None,
                segmented(&p, "range-set", &range_opts, s.chart_range, cx, |app, v, w, cx| {
                    app.update_settings(|s| s.chart_range = v, w, cx)
                }),
            ))
            .child(row(
                &p,
                t("Show capital invested line", "Mostrar línea de capital invertido"),
                Some(t("A dashed reference line on portfolio charts", "Una línea punteada de referencia en los gráficos")),
                Switch::new("show-ref").checked(s.show_interest_line).on_click(cx.listener(|this, checked: &bool, w, cx| {
                    let v = *checked;
                    this.update_settings(|s| s.show_interest_line = v, w, cx)
                })),
            ))
            .child(row(
                &p,
                t("Theme", "Tema"),
                None,
                segmented(&p, "theme", &theme_opts, s.theme, cx, |app, v, w, cx| app.update_settings(|s| s.theme = v, w, cx)),
            ));

        let path_text = db_path.display().to_string();
        let data = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Database location", "Ubicación de la base de datos"),
                None,
                Button::new("open-folder")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .label(t("Show in Explorer", "Mostrar en el Explorador"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(d) = &db_dir {
                            cx.open_with_system(d);
                        }
                    })),
            ))
            .child(
                div()
                    .px(px(space::M))
                    .py(px(10.))
                    .rounded(px(8.))
                    .bg(p.raised)
                    .text_size(px(size::META + 0.5))
                    .text_color(p.muted)
                    .child(path_text),
            )
            .child(row(
                &p,
                t("Back up", "Copia de seguridad"),
                Some(match &s.last_backup_at {
                    Some(_) => t("A verified copy of all your data", "Una copia verificada de todos tus datos"),
                    None => t("You haven't made a backup yet", "Aún no has hecho una copia"),
                }),
                Button::new("backup")
                    .icon(IconName::DatabaseBackup)
                    .label(t("Back up now", "Hacer copia ahora"))
                    .on_click(cx.listener(|this, _, w, cx| this.backup_now(&crate::app::BackupNow, w, cx))),
            ))
            .child(row(
                &p,
                t("Restore", "Restaurar"),
                Some(t("The backup is checked first and a safety copy is kept", "La copia se verifica primero y se guarda un respaldo")),
                Button::new("restore")
                    .icon(IconName::ArchiveRestore)
                    .label(t("Restore from backup", "Restaurar una copia"))
                    .on_click(cx.listener(|this, _, w, cx| this.choose_restore(w, cx))),
            ))
            .child(row(
                &p,
                t("Export", "Exportar"),
                Some(t("Clients, loans, payments and capital as CSV", "Clientes, préstamos, pagos y capital en CSV")),
                Button::new("export")
                    .icon(IconName::FileSpreadsheet)
                    .label(t("Export CSV", "Exportar CSV"))
                    .on_click(cx.listener(|this, _, w, cx| this.choose_export(w, cx))),
            ));

        let privacy = v_flex()
            .gap(px(space::M))
            .child(h_flex().gap(px(8.)).child(icon(IconName::ShieldCheck, p.positive, 16.)).child(t("Offline and private", "Sin internet y privado")))
            .child(div().text_color(p.muted).child(t(
                "The app makes no network requests, sends no telemetry and needs no account. All data stays in the database file above.",
                "La app no usa internet, no envía datos y no necesita cuenta. Todo queda en el archivo de base de datos de arriba.",
            )))
            .child(div().text_color(p.muted).child(t(
                "The database is not encrypted by the app. Protect it with Windows BitLocker or device encryption, and keep backups somewhere safe.",
                "La app no cifra la base de datos. Protégela con BitLocker o el cifrado del dispositivo y guarda las copias en un lugar seguro.",
            )));

        let keys = [
            (t("Record payment", "Registrar pago"), "Ctrl P"),
            (t("New loan", "Nuevo préstamo"), "Ctrl L"),
            (t("New client", "Nuevo cliente"), "Ctrl N"),
            (t("New expense", "Nuevo gasto"), "Ctrl E"),
            (t("Search", "Buscar"), "Ctrl F"),
            (t("Back up", "Copia de seguridad"), "Ctrl S"),
            (t("Toggle sidebar", "Mostrar u ocultar menú"), "Ctrl B"),
            (t("Go to sections", "Ir a las secciones"), "Ctrl 1 – 6"),
            (t("Back", "Atrás"), "Alt ←"),
        ];
        let shortcuts = v_flex().gap(px(2.)).children(keys.iter().map(|(a, k)| {
            h_flex().justify_between().min_h(px(30.)).child(div().text_color(p.muted).child(a.to_string())).child(kbd(&p, k))
        }));

        v_flex()
            .child(
                crate::ui::components::page_header(
                    &p,
                    t("Settings", "Configuración"),
                    Some(t("Changes are saved right away.", "Los cambios se guardan al instante.").into_any_element()),
                    vec![],
                )
                .pb(px(space::S)),
            )
            .child(section(&p, t("Business", "Negocio"), t("Your name and language", "Tu nombre e idioma"), business))
            .child(section(&p, t("General", "General"), t("Currency, numbers and dates", "Moneda, números y fechas"), general))
            .child(section(
                &p,
                t("Lending", "Préstamos"),
                t("Defaults and how interest works", "Valores iniciales y cómo funciona el interés"),
                lending,
            ))
            .child(section(&p, t("Receipts", "Recibos"), t("Printing payment receipts", "Impresión de recibos de pago"), receipts))
            .child(section(&p, t("Dashboard", "Panel"), t("Charts and appearance", "Gráficos y apariencia"), dashboard))
            .child(section(
                &p,
                t("Data", "Datos"),
                t("Your database, backups and exports", "Tu base de datos, copias y exportaciones"),
                data,
            ))
            .child(section(&p, t("Privacy", "Privacidad"), t("How your data is handled", "Cómo se manejan tus datos"), privacy))
            .child(section(
                &p,
                t("Keyboard", "Teclado"),
                t("Shortcuts available everywhere", "Atajos disponibles en toda la app"),
                shortcuts,
            ))
            .into_any_element()
    }
}
