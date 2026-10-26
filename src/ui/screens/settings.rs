use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::VzApp;
use crate::domain::loans::{Frequency, RATE_PRESETS};
use crate::domain::money::{NumberStyle, Rate};
use crate::i18n::{Lang, t};
use crate::services::settings::{ChartRange, ThemePref};
use crate::ui::components::{icon, kbd, page_header, sel};
use crate::ui::kit::{Button, Card, Dropdown, DropdownItem, Segmented};
use crate::ui::theme::{Palette, RADIUS_CONTROL, Typeset as _, Width, palette, space, tabular, text};
use crate::utils::dates::DateStyle;

/// A client detail that can be printed on receipts.
#[derive(Clone, Copy)]
enum ClientField {
    Name,
    IdNumber,
    Phone,
    Address,
}

/// Settings read as a form; past this width lines get too long to scan.
const FORM_MAX: f32 = 760.0;
/// Narrowest a setting's name column gets before its control moves to the
/// next line.
const LABEL_MIN: f32 = 240.0;
/// Width of text fields and the currency select.
const FIELD_W: f32 = 260.0;

/// One group of settings: a card with its title, a line on what the group
/// covers, then its rows.
fn section(p: &Palette, title: &str, description: &str, body: impl IntoElement) -> Card {
    Card::new()
        .title(title.to_string())
        .child(div().px(px(space::XL)).typeset(text::CAPTION).text_color(p.muted).child(description.to_string()))
        .child(div().p(px(space::XL)).pt(px(space::L)).child(body))
}

/// A setting: name and hint on the left, its control on the right. When
/// the card is too narrow for both, the control drops below the name.
fn row(p: &Palette, title: &str, hint: Option<&str>, control: impl IntoElement) -> Div {
    h_flex()
        .flex_wrap()
        .justify_between()
        .gap_x(px(space::XL))
        .gap_y(px(space::S))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(LABEL_MIN))
                .gap(px(2.))
                .child(div().typeset(text::BODY_STRONG).text_color(p.text).child(title.to_string()))
                .children(hint.map(|h| div().typeset(text::CAPTION).text_color(p.muted).child(h.to_string()))),
        )
        .child(div().flex_none().child(control))
}

/// A select over app settings, for option sets with too many or too long
/// labels for a segmented control. `options` are (key, label).
fn select<K: Copy + PartialEq + 'static>(
    id: &str,
    options: &[(K, &str)],
    selected: K,
    cx: &Context<VzApp>,
    on_select: impl Fn(&mut VzApp, K, &mut Window, &mut Context<VzApp>) + Clone + 'static,
) -> Dropdown {
    let current = options.iter().find(|(k, _)| *k == selected).map(|(_, l)| l.to_string()).unwrap_or_default();
    Dropdown::new(SharedString::from(id.to_string())).label(current).align_end().items(options.iter().map(|(k, label)| {
        let (k, on_select) = (*k, on_select.clone());
        DropdownItem::new(label.to_string(), cx.listener(move |this, _, w, cx| on_select(this, k, w, cx))).selected(k == selected)
    }))
}

/// Facts read from disk for the Settings screen, refreshed every few seconds
/// rather than on every frame.
#[derive(Clone)]
pub(crate) struct SettingsFacts {
    at: std::time::Instant,
    backups: Vec<(std::path::PathBuf, std::time::SystemTime)>,
    checks: Vec<crate::diagnostics::FolderCheck>,
    windows: Option<String>,
}

impl VzApp {
    fn settings_facts(&mut self) -> &SettingsFacts {
        let stale = self.settings_facts.as_ref().is_none_or(|f| f.at.elapsed() > std::time::Duration::from_secs(5));
        if stale {
            let data_dir = self.store.path().parent().map(|d| d.to_path_buf()).unwrap_or_default();
            self.settings_facts = Some(SettingsFacts {
                at: std::time::Instant::now(),
                backups: self.store.list_backups(),
                checks: crate::diagnostics::check_folders(&[
                    (t("Data", "Datos"), data_dir),
                    (t("Backups", "Copias"), self.store.backups_dir()),
                    (t("Receipts", "Recibos"), self.store.receipts_dir()),
                ]),
                windows: crate::diagnostics::windows_version(),
            });
        }
        self.settings_facts.as_ref().expect("just set")
    }

    pub(crate) fn render_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let w = Width::of(self.content_width(window));
        let s = self.settings().clone();
        let today = self.today;
        let db_path = self.store.path().to_path_buf();
        let db_dir = db_path.parent().map(|d| d.to_path_buf());
        let receipts_dir = self.store.receipts_dir();
        let facts = self.settings_facts().clone();
        let backups = facts.backups.clone();

        let lang_opts = [(Lang::En, Lang::En.label()), (Lang::Es, Lang::Es.label())];
        let number_opts: Vec<(NumberStyle, &str)> =
            [NumberStyle::CommaDot, NumberStyle::DotComma, NumberStyle::SpaceComma].iter().map(|n| (*n, n.label())).collect();
        let date_labels: Vec<(DateStyle, String)> = DateStyle::ALL.iter().map(|d| (*d, d.format(today))).collect();
        let date_opts: Vec<(DateStyle, &str)> = date_labels.iter().map(|(d, l)| (*d, l.as_str())).collect();
        let rate_labels: Vec<(Rate, String)> = RATE_PRESETS.iter().map(|r| (*r, r.percent_label())).collect();
        let rate_opts: Vec<(Rate, &str)> = rate_labels.iter().map(|(r, l)| (*r, l.as_str())).collect();
        let freq_opts: Vec<(Frequency, &str)> = Frequency::ALL.iter().map(|f| (*f, f.label())).collect();
        let fee_labels: Vec<(Rate, String)> = [0, 30_000, 50_000, 100_000]
            .iter()
            .map(|v| (Rate(*v), if *v == 0 { t("None", "Ninguno").to_string() } else { Rate(*v).percent_label() }))
            .collect();
        let fee_opts: Vec<(Rate, &str)> = fee_labels.iter().map(|(r, l)| (*r, l.as_str())).collect();
        let range_opts: Vec<(ChartRange, &str)> = ChartRange::ALL.iter().map(|r| (*r, r.label())).collect();
        let theme_opts = [(ThemePref::Light, t("Light", "Claro")), (ThemePref::Dark, t("Dark", "Oscuro"))];
        let name_changed = self.receipt_name_input.read(cx).value().trim() != s.receipt_name;
        let accent = p.accent;

        let business = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Name on receipts", "Nombre en los recibos"),
                Some(t(
                    "Printed at the top of every receipt. Leave it empty to print “Acervus Finance”.",
                    "Se imprime arriba en cada recibo. Déjalo vacío para imprimir “Acervus Finance”.",
                )),
                h_flex().gap(px(space::S)).child(div().w(px(FIELD_W)).child(Input::new(&self.receipt_name_input))).child(
                    Button::new("save-name")
                        .primary()
                        .label(t("Save", "Guardar"))
                        .disabled(!name_changed)
                        .on_click(cx.listener(|this, _, w, cx| this.save_receipt_name(w, cx))),
                ),
            ))
            .child(row(
                &p,
                t("Logo", "Logo"),
                Some(t(
                    "Shown in the sidebar and on receipts. PNG, JPG, GIF, BMP or WebP, up to 20 MB. You can also drag a picture onto the box.",
                    "Aparece en el menú y en los recibos. PNG, JPG, GIF, BMP o WebP, hasta 20 MB. También puedes arrastrar una imagen al cuadro.",
                )),
                h_flex()
                    .gap(px(space::S))
                    .child(
                        h_flex()
                            .id("logo-drop")
                            .size(px(56.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(RADIUS_CONTROL))
                            .border_1()
                            .border_color(p.border)
                            .bg(p.raised)
                            .overflow_hidden()
                            .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_color(accent).border_2())
                            .on_drop(cx.listener(|this, paths: &ExternalPaths, w, cx| {
                                if let Some(path) = paths.paths().first() {
                                    let path = path.clone();
                                    this.use_logo_file(&path, w, cx);
                                }
                            }))
                            .child(match &self.logo {
                                Some(logo) => img(logo.clone()).size(px(52.)).object_fit(ObjectFit::Contain).into_any_element(),
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
                Segmented::pick("lang", lang_opts.iter().copied(), s.language, cx, |app, v, w, cx| app.update_settings(|s| s.language = v, w, cx)),
            ));

        let general = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Currency", "Moneda"),
                Some(t("Changes the symbol shown. Amounts are never converted.", "Cambia el símbolo. Los montos nunca se convierten.")),
                div().w(px(FIELD_W)).child(Select::new(&self.currency_select)),
            ))
            .child(row(
                &p,
                t("Number format", "Formato de números"),
                None,
                Segmented::pick("numfmt", number_opts.iter().copied(), s.number_style, cx, |app, v, w, cx| {
                    app.update_settings(|s| s.number_style = v, w, cx)
                }),
            ))
            .child(row(
                &p,
                t("Date format", "Formato de fecha"),
                None,
                select("datefmt", &date_opts, s.date_style, cx, |app, v, w, cx| app.update_settings(|s| s.date_style = v, w, cx)),
            ));

        let lending = v_flex()
            .gap(px(space::L))
            .child(row(
                &p,
                t("Default rate", "Tasa predeterminada"),
                Some(t("Preselected for new loans", "Se preselecciona en préstamos nuevos")),
                Segmented::pick("rate", rate_opts.iter().copied(), s.default_rate, cx, |app, v, w, cx| app.update_settings(|s| s.default_rate = v, w, cx)),
            ))
            .child(row(
                &p,
                t("Default payments", "Pagos predeterminados"),
                None,
                select("freq", &freq_opts, s.default_frequency, cx, |app, v, w, cx| app.update_settings(|s| s.default_frequency = v, w, cx)),
            ))
            .child(row(
                &p,
                t("Late fee for new installment loans", "Cargo por mora para préstamos por cuotas"),
                Some(t(
                    "Charged once on what is unpaid when an installment becomes overdue. Each loan keeps the rate it was made with.",
                    "Se cobra una vez sobre lo que falte cuando una cuota se atrasa. Cada préstamo conserva la tasa con que se hizo.",
                )),
                Segmented::pick("fee", fee_opts.iter().copied(), s.late_fee_rate, cx, |app, v, w, cx| app.update_settings(|s| s.late_fee_rate = v, w, cx)),
            ))
            .child(
                v_flex()
                    .gap(px(space::XS))
                    .p(px(space::M))
                    .rounded(px(RADIUS_CONTROL))
                    .bg(p.raised)
                    .typeset(text::CAPTION)
                    .text_color(p.muted)
                    .font_features(tabular())
                    .child(t(
                        "Installment loans: each installment repays an equal part of the capital plus the period's interest (on the capital still owed, or on the amount lent with flat interest). Example: 10,000 at 10% a month in 5 installments is 3,000, 2,800, 2,600, 2,400 and 2,200.",
                        "Préstamos por cuotas: cada cuota paga una parte igual del capital más el interés del período (sobre el capital adeudado, o sobre el monto prestado con interés fijo). Ejemplo: 10,000 al 10% mensual en 5 cuotas son 3,000, 2,800, 2,600, 2,400 y 2,200.",
                    ))
                    .child(t(
                        "Interest only: 1,000 at 10% is 100 of interest. Paying 600 covers the 100 and lowers the capital to 500, so the next interest is 50.",
                        "Solo interés: 1,000 al 10% son 100 de interés. Si paga 600, cubre los 100 y el capital baja a 500, así que el próximo interés es 50.",
                    ))
                    .child(t(
                        "Late fees and interest rates may be limited by law where you lend. Check the rules that apply to you.",
                        "Los cargos por mora y las tasas pueden estar limitados por la ley donde prestas. Revisa las reglas que te aplican.",
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
                    "Amount, rate, the installment schedule and signature lines for the client",
                    "Monto, tasa, el calendario de cuotas y líneas de firma para el cliente",
                )),
                Switch::new("rcpt-loan").checked(s.receipt_after_loan).on_click(cx.listener(|this, checked: &bool, w, cx| {
                    let v = *checked;
                    this.update_settings(|s| s.receipt_after_loan = v, w, cx)
                })),
            ))
            .child(row(
                &p,
                t("Client info to add to every receipt", "Datos del cliente en cada recibo"),
                Some(t(
                    "A detail the client doesn't have is left out. Photos are never printed.",
                    "Si el cliente no tiene un dato, se omite. Las fotos nunca se imprimen.",
                )),
                v_flex().gap(px(space::S)).children(
                    [
                        ("rcpt-client-name", t("Client name", "Nombre del cliente"), s.receipt_client.name, ClientField::Name),
                        ("rcpt-client-id", t("ID number", "Número de identificación"), s.receipt_client.id_number, ClientField::IdNumber),
                        ("rcpt-client-phone", t("Phone number", "Teléfono"), s.receipt_client.phone, ClientField::Phone),
                        ("rcpt-client-address", t("Address", "Dirección"), s.receipt_client.address, ClientField::Address),
                    ]
                    .map(|(id, label, checked, which)| {
                        Checkbox::new(id).label(label).checked(checked).on_click(cx.listener(move |this, checked: &bool, w, cx| {
                            let v = *checked;
                            this.update_settings(
                                |s| {
                                    let f = &mut s.receipt_client;
                                    *match which {
                                        ClientField::Name => &mut f.name,
                                        ClientField::IdNumber => &mut f.id_number,
                                        ClientField::Phone => &mut f.phone,
                                        ClientField::Address => &mut f.address,
                                    } = v
                                },
                                w,
                                cx,
                            )
                        }))
                    }),
                ),
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
                Segmented::pick("range-set", range_opts.iter().copied(), s.chart_range, cx, |app, v, w, cx| {
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
                Segmented::pick("theme", theme_opts.iter().copied(), s.theme, cx, |app, v, w, cx| {
                    app.update_settings(|s| s.theme = v, w, cx)
                }),
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
                    .py(px(space::S + 2.))
                    .rounded(px(RADIUS_CONTROL))
                    .bg(p.raised)
                    .typeset(text::CAPTION)
                    .text_color(p.muted)
                    .child(sel(path_text)),
            )
            .child(row(
                &p,
                t("Back up", "Copia de seguridad"),
                Some(match &s.last_backup_at {
                    Some(_) => t("A verified copy of all your data", "Una copia verificada de todos tus datos"),
                    None => t("You haven't made a backup yet", "Aún no has hecho una copia"),
                }),
                h_flex()
                    .gap(px(space::S))
                    .child(
                        Button::new("backup-folder")
                            .ghost()
                            .label(t("Save in backups folder", "Guardar en la carpeta de copias"))
                            .tooltip(t(
                                "No window: saves straight into the app's backups folder",
                                "Sin ventana: se guarda directo en la carpeta de copias",
                            ))
                            .on_click(cx.listener(|this, _, w, cx| this.backup_to_folder(None, w, cx))),
                    )
                    .child(
                        Button::new("backup")
                            .icon(IconName::DatabaseBackup)
                            .label(t("Back up to…", "Copiar en…"))
                            .on_click(cx.listener(|this, _, w, cx| this.backup_now(&crate::app::BackupNow, w, cx))),
                    ),
            ))
            .child(row(
                &p,
                t("Restore", "Restaurar"),
                Some(t("The backup is checked first and a safety copy is kept", "La copia se verifica primero y se guarda un respaldo")),
                Button::new("restore")
                    .icon(IconName::ArchiveRestore)
                    .label(t("Restore from a file…", "Restaurar un archivo…"))
                    .on_click(cx.listener(|this, _, w, cx| this.choose_restore(w, cx))),
            ))
            .when(!backups.is_empty(), |d| {
                d.child(
                    v_flex()
                        .gap(px(2.))
                        .child(
                            div()
                                .typeset(text::CAPTION)
                                .text_color(p.muted)
                                .child(t("Backups in the backups folder", "Copias en la carpeta de copias")),
                        )
                        .children(backups.iter().take(5).enumerate().map(|(i, (path, when))| {
                            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            let when = chrono::DateTime::<chrono::Local>::from(*when).format("%Y-%m-%d %H:%M").to_string();
                            let path = path.clone();
                            h_flex()
                                .justify_between()
                                .gap(px(space::M))
                                .min_h(px(space::XXL))
                                .child(div().min_w_0().truncate().typeset(text::CAPTION).text_color(p.text).child(name))
                                .child(
                                    h_flex()
                                        .gap(px(space::M))
                                        .flex_none()
                                        .child(div().typeset(text::CAPTION).font_features(tabular()).text_color(p.faint).child(when))
                                        .child(
                                            Button::new(("restore-file", i))
                                                .ghost()
                                                .small()
                                                .label(t("Restore", "Restaurar"))
                                                .on_click(cx.listener(move |this, _, w, cx| this.confirm_restore(path.clone(), w, cx))),
                                        ),
                                )
                        })),
                )
            })
            .child(row(
                &p,
                t("Export", "Exportar"),
                Some(t(
                    "Clients, loans, installments, payments, fees and capital as CSV",
                    "Clientes, préstamos, cuotas, pagos, cargos y capital en CSV",
                )),
                Button::new("export")
                    .icon(IconName::FileSpreadsheet)
                    .label(t("Export CSV", "Exportar CSV"))
                    .on_click(cx.listener(|this, _, w, cx| this.choose_export(w, cx))),
            ))
            .child(row(
                &p,
                t("Reset all data", "Borrar todos los datos"),
                Some(t(
                    "Deletes clients, loans, payments and money movements to start clean. A copy is saved first.",
                    "Borra clientes, préstamos, pagos y movimientos para empezar de cero. Antes se guarda una copia.",
                )),
                Button::new("reset-data")
                    .danger_ghost()
                    .icon(IconName::Trash)
                    .label(t("Reset…", "Borrar…"))
                    .on_click(cx.listener(|this, _, w, cx| this.open_reset_form(w, cx))),
            ));

        let checks = facts.checks.clone();
        let windows = facts.windows.clone();
        let log_dir = crate::diagnostics::log_dir();
        let diagnostics = v_flex()
            .gap(px(space::M))
            .child(row(
                &p,
                t("Windows", "Windows"),
                None,
                div().text_color(p.muted).child(sel(windows.unwrap_or_else(|| t("Unknown", "Desconocido").into()))),
            ))
            .children(checks.into_iter().map(|c| {
                let ok = c.result.is_ok();
                let status = match &c.result {
                    Ok(()) => t("Can be written", "Se puede escribir").to_string(),
                    Err(e) => crate::tr!("Can't be written: {e}", "No se puede escribir: {e}"),
                };
                h_flex()
                    .justify_between()
                    .gap(px(space::L))
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(div().typeset(text::BODY_STRONG).child(c.label))
                            .child(div().typeset(text::CAPTION).text_color(p.muted).truncate().child(sel(c.path.display().to_string()))),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap(px(6.))
                            .child(icon(
                                if ok { IconName::CircleCheck } else { IconName::CircleAlert },
                                if ok { p.positive } else { p.negative },
                                14.,
                            ))
                            .child(div().typeset(text::CAPTION).text_color(if ok { p.muted } else { p.negative }).child(status)),
                    )
            }))
            .child(row(
                &p,
                t("Log file", "Archivo de registro"),
                Some(t(
                    "Records problems such as a file window that couldn't open. It never contains names or amounts.",
                    "Registra problemas como una ventana de archivos que no se pudo abrir. Nunca contiene nombres ni montos.",
                )),
                Button::new("open-logs").ghost().icon(IconName::FolderOpen).label(t("Open folder", "Abrir carpeta")).on_click(cx.listener(
                    move |this, _, w, cx| match &log_dir {
                        Some(d) if d.exists() => cx.open_with_system(d),
                        _ => this.notify_error(
                            t(
                                "There is no log yet: it is written by the installed app.",
                                "Aún no hay registro: lo escribe la app instalada.",
                            ),
                            w,
                            cx,
                        ),
                    },
                )),
            ));

        let about = v_flex()
            .gap(px(space::S))
            .child(h_flex().flex_wrap().gap(px(space::M)).child(div().typeset(text::HEADING).child(crate::APP_NAME)).child(
                div().text_color(p.muted).font_features(tabular()).child(format!(
                    "{} {}",
                    t("Version", "Versión"),
                    env!("CARGO_PKG_VERSION")
                )),
            ))
            .child(div().text_color(p.muted).child(format!("Developed by {}", crate::DEVELOPER)))
            .child(div().typeset(text::CAPTION).text_color(p.faint).child(t(
                "Supported on Windows 10 (version 1809 or later) and Windows 11, 64-bit.",
                "Compatible con Windows 10 (versión 1809 o posterior) y Windows 11, de 64 bits.",
            )));

        let privacy = v_flex()
            .gap(px(space::M))
            .child(
                h_flex()
                    .gap(px(space::S))
                    .child(icon(IconName::ShieldCheck, p.positive, 16.))
                    .child(div().typeset(text::BODY_STRONG).child(t("Offline and private", "Sin internet y privado"))),
            )
            .child(div().text_color(p.muted).child(t(
                "The app makes no network requests, sends no telemetry and needs no account. All data stays in the database file above.",
                "La app no usa internet, no envía datos y no necesita cuenta. Todo queda en el archivo de base de datos de arriba.",
            )))
            .child(div().text_color(p.muted).child(t(
                "The database is not encrypted by the app. Protect it with Windows BitLocker or device encryption, and keep backups somewhere safe.",
                "La app no cifra la base de datos. Protégela con BitLocker o el cifrado del dispositivo y guarda las copias en un lugar seguro.",
            )))
            .child(div().text_color(p.muted).child(t(
                "Client photos, which may show an ID, are kept in the database, so backups include them: treat backups as private as the photos. Photos are never printed on receipts, exported to CSV or written to the log.",
                "Las fotos de los clientes, que pueden mostrar una identificación, se guardan en la base de datos, así que las copias las incluyen: trátalas con la misma privacidad. Las fotos nunca se imprimen en recibos, se exportan a CSV ni se escriben en el registro.",
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
            h_flex()
                .justify_between()
                .gap(px(space::M))
                .min_h(px(30.))
                .child(div().text_color(p.muted).child(a.to_string()))
                .child(kbd(&p, k))
        }));

        v_flex()
            .w_full()
            .max_w(px(FORM_MAX))
            .gap(px(w.gap()))
            .child(
                page_header(
                    &p,
                    t("Settings", "Configuración"),
                    Some(t("Changes are saved right away.", "Los cambios se guardan al instante.").into_any_element()),
                    vec![],
                )
                .pb_0(),
            )
            .child(section(
                &p,
                t("Business", "Negocio"),
                t("Name on receipts, logo and language", "Nombre en los recibos, logo e idioma"),
                business,
            ))
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
            .child(section(
                &p,
                t("Diagnostics", "Diagnóstico"),
                t("What to check when something doesn't work on a computer", "Qué revisar si algo no funciona en una computadora"),
                diagnostics,
            ))
            .child(section(&p, t("About", "Acerca de"), t("Version and developer", "Versión y desarrollador"), about))
            .into_any_element()
    }
}
