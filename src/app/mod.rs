//! The root view: owns the store and the current snapshot, routes between
//! screens, and hosts the sidebar. Screens render from the snapshot only.

mod dialogs;
mod sidebar;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use chrono::NaiveDate;
use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use crate::domain::portfolio::{Account, EntryKind};
use crate::i18n::{self, t};
use crate::services::settings::{CURRENCIES, Settings};
use crate::services::{CashShortfall, InsufficientCash, Invalid, Snapshot, Store};
use crate::tr;
use crate::ui::forms::loan::Choice;
use crate::ui::theme::{self, Typeset as _, palette, space};
use crate::utils::dates::today;

actions!(
    vz,
    [
        NewPayment,
        NewLoan,
        NewClient,
        NewExpense,
        ToggleSidebar,
        GoOverview,
        GoClients,
        GoLoans,
        GoPayments,
        GoAnalytics,
        GoSettings,
        GoBack,
        BackupNow,
        FocusSearch,
    ]
);

pub const CONTEXT: &str = "VzApp";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-p", NewPayment, Some(CONTEXT)),
        KeyBinding::new("ctrl-l", NewLoan, Some(CONTEXT)),
        KeyBinding::new("ctrl-n", NewClient, Some(CONTEXT)),
        KeyBinding::new("ctrl-e", NewExpense, Some(CONTEXT)),
        KeyBinding::new("ctrl-b", ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new("ctrl-1", GoOverview, Some(CONTEXT)),
        KeyBinding::new("ctrl-2", GoClients, Some(CONTEXT)),
        KeyBinding::new("ctrl-3", GoLoans, Some(CONTEXT)),
        KeyBinding::new("ctrl-4", GoPayments, Some(CONTEXT)),
        KeyBinding::new("ctrl-5", GoAnalytics, Some(CONTEXT)),
        KeyBinding::new("ctrl-6", GoSettings, Some(CONTEXT)),
        KeyBinding::new("alt-left", GoBack, Some(CONTEXT)),
        KeyBinding::new("ctrl-s", BackupNow, Some(CONTEXT)),
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
    ]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Overview,
    Clients,
    Client(i64),
    Loans,
    Loan(i64),
    Payments,
    Analytics,
    Settings,
}

impl Route {
    pub fn section(self) -> Route {
        match self {
            Route::Client(_) => Route::Clients,
            Route::Loan(_) => Route::Loans,
            r => r,
        }
    }
}

/// Company details edited in Settings besides the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompanyField {
    Address,
    Phone,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoanFilter {
    #[default]
    Open,
    Attention,
    PaidOff,
    Defaulted,
    All,
}

pub struct VzApp {
    pub(crate) store: Store,
    pub(crate) snap: Rc<Snapshot>,
    pub(crate) today: NaiveDate,
    /// Late fees were last brought up to date on this day.
    refreshed_on: NaiveDate,
    /// A Windows file window this app opened and is waiting on, if any.
    pub(crate) file_dialog: Option<&'static str>,
    /// Disk facts shown in Settings (backups, folder checks).
    pub(crate) settings_facts: Option<crate::ui::screens::settings::SettingsFacts>,
    pub(crate) route: Route,
    history: Vec<Route>,
    pub(crate) focus: FocusHandle,
    pub(crate) client_search: Entity<InputState>,
    pub(crate) payment_search: Entity<InputState>,
    pub(crate) currency_select: Entity<SelectState<Vec<Choice>>>,
    pub(crate) receipt_name_input: Entity<InputState>,
    pub(crate) company_address_input: Entity<InputState>,
    pub(crate) company_phone_input: Entity<InputState>,
    pub(crate) loan_search: Entity<InputState>,
    /// Column and direction the Clients and Loans tables are sorted by.
    pub(crate) client_sort: crate::ui::kit::Sort<crate::ui::screens::clients::ClientColumn>,
    pub(crate) loan_sort: crate::ui::kit::Sort<crate::ui::screens::loans::LoanColumn>,
    pub(crate) loan_filter: LoanFilter,
    pub(crate) show_archived: bool,
    pub(crate) chart_hover: Option<usize>,
    pub(crate) analytics_hover: Option<usize>,
    pub(crate) load_error: Option<String>,
    /// The business logo, decoded once per upload.
    pub(crate) logo: Option<Arc<Image>>,
    /// A client photo someone chose to show, with the client it belongs to.
    /// Hidden again after any change to the data.
    pub(crate) shown_photo: Option<(i64, Arc<Image>)>,
    _subscriptions: Vec<Subscription>,
}

/// The logo as an image the UI can draw, reusing `current` when the bytes
/// are unchanged.
fn logo_image(snap: &Snapshot, current: Option<Arc<Image>>) -> Option<Arc<Image>> {
    let bytes = snap.logo.as_ref()?;
    let fresh = Image::from_bytes(ImageFormat::Png, bytes.to_vec());
    match current {
        Some(img) if img.id() == fresh.id() => Some(img),
        _ => Some(Arc::new(fresh)),
    }
}

fn currency_choices() -> Vec<Choice> {
    CURRENCIES.iter().enumerate().map(|(i, c)| Choice { id: i as i64, title: format!("{} · {}", c.code, c.name()).into() }).collect()
}

impl VzApp {
    pub fn new(mut store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = today();
        match store.refresh(today) {
            Ok(0) => {}
            Ok(n) => log::info!("recorded {n} late fee changes at startup"),
            Err(e) => log::error!("could not bring late fees up to date: {e:#}"),
        }
        let (snap, load_error) = match store.snapshot() {
            Ok(s) => (s, None),
            Err(e) => {
                log::error!("could not load data: {e:#}");
                (Snapshot::default(), Some(format!("{e:#}")))
            }
        };
        i18n::set(snap.settings.language);
        window.set_window_title(crate::APP_NAME);
        let client_search = cx.new(|cx| InputState::new(window, cx));
        let payment_search = cx.new(|cx| InputState::new(window, cx));
        let receipt_name_input = cx.new(|cx| InputState::new(window, cx).default_value(snap.settings.receipt_name.clone()));
        let company_address_input = cx.new(|cx| InputState::new(window, cx).default_value(snap.settings.company_address.clone()));
        let company_phone_input = cx.new(|cx| InputState::new(window, cx).default_value(snap.settings.company_phone.clone()));
        let loan_search = cx.new(|cx| InputState::new(window, cx));
        let selected = CURRENCIES.iter().position(|c| c.code == snap.settings.currency_code).map(IndexPath::new);
        let currency_select = cx.new(|cx| SelectState::new(currency_choices(), selected, window, cx));
        let subs = vec![
            cx.subscribe_in(&currency_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, window, cx| {
                if let SelectEvent::Confirm(Some(i)) = ev {
                    let code = CURRENCIES[*i as usize].code.to_string();
                    this.update_settings(|s| s.currency_code = code, window, cx);
                }
            }),
            cx.subscribe_in(&receipt_name_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.save_receipt_name(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe_in(&company_address_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.save_company(CompanyField::Address, window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe_in(&company_phone_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.save_company(CompanyField::Phone, window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&loan_search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&client_search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&payment_search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let logo = logo_image(&snap, None);
        let mut app = Self {
            store,
            snap: Rc::new(snap),
            today,
            refreshed_on: today,
            file_dialog: None,
            settings_facts: None,
            route: Route::Overview,
            history: Vec::new(),
            focus,
            client_search,
            payment_search,
            currency_select,
            receipt_name_input,
            company_address_input,
            company_phone_input,
            loan_search,
            client_sort: Default::default(),
            loan_sort: Default::default(),
            loan_filter: LoanFilter::default(),
            show_archived: false,
            chart_hover: None,
            analytics_hover: None,
            load_error,
            logo,
            shown_photo: None,
            _subscriptions: subs,
        };
        app.apply_language(window, cx);
        // Debug builds only: VZ_FINANCE_START opens a screen or dialog at
        // launch, so screenshots can be taken without clicking anything.
        if cfg!(debug_assertions)
            && let Ok(start) = std::env::var("VZ_FINANCE_START")
        {
            cx.defer_in(window, move |app, window, cx| match start.as_str() {
                "clients" => app.navigate(Route::Clients, window, cx),
                "loans" => app.navigate(Route::Loans, window, cx),
                "analytics" => app.navigate(Route::Analytics, window, cx),
                "settings" => app.navigate(Route::Settings, window, cx),
                "new-record" => app.open_entry_form(EntryKind::Expense, None, window, cx),
                "new-income" => app.open_entry_form(EntryKind::Income, None, window, cx),
                "wording" => app.open_receipt_text_form(window, cx),
                _ => {}
            });
        }
        // Left open past midnight: what is overdue and the late fees change
        // with the date, so bring everything up to date when it does.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(60)).await;
                let alive = this.update(cx, |app, cx| {
                    if crate::utils::dates::today() != app.today {
                        app.reload(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        app
    }

    pub fn settings(&self) -> &Settings {
        &self.snap.settings
    }

    /// Re-labels the persistent controls after a language change.
    fn apply_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        i18n::set(self.snap.settings.language);
        self.client_search.update(cx, |s, cx| {
            s.set_placeholder(t("Search clients by name, phone or email", "Buscar clientes por nombre, teléfono o correo"), window, cx)
        });
        self.payment_search
            .update(cx, |s, cx| s.set_placeholder(t("Search payments by client or note", "Buscar pagos por cliente o nota"), window, cx));
        self.loan_search.update(cx, |s, cx| {
            s.set_placeholder(t("Search by loan #, client, amount or status", "Buscar por préstamo #, cliente, monto o estado"), window, cx)
        });
        let selected = CURRENCIES.iter().position(|c| c.code == self.snap.settings.currency_code).map(IndexPath::new);
        self.currency_select.update(cx, |s, cx| {
            s.set_items(currency_choices(), window, cx);
            if let Some(ix) = selected {
                s.set_selected_index(Some(ix), window, cx);
            }
        });
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.today = today();
        if self.refreshed_on != self.today {
            // A new day: installments may have become overdue.
            if let Err(e) = self.store.refresh(self.today) {
                log::error!("could not bring late fees up to date: {e:#}");
            }
            self.refreshed_on = self.today;
        }
        match self.store.snapshot() {
            Ok(s) => {
                self.logo = logo_image(&s, self.logo.take());
                self.shown_photo = None;
                self.snap = Rc::new(s);
                self.load_error = None;
            }
            Err(e) => self.load_error = Some(format!("{e:#}")),
        }
        i18n::set(self.snap.settings.language);
        if let Route::Client(id) = self.route
            && self.snap.client(id).is_none()
        {
            self.route = Route::Clients;
        }
        if let Route::Loan(id) = self.route
            && self.snap.loan(id).is_none()
        {
            self.route = Route::Loans;
        }
        cx.notify();
    }

    /// Width available to a screen's content, for layouts that reflow on
    /// narrow windows.
    pub fn content_width(&self, window: &Window) -> f32 {
        let side = if self.sidebar_collapsed(window) { theme::SIDEBAR_COLLAPSED } else { theme::SIDEBAR_WIDTH };
        let viewport = f32::from(window.viewport_size().width);
        viewport - side - 2.0 * theme::Width::gutter(viewport)
    }

    /// The sidebar shows icons only when the user collapsed it or the
    /// window is too narrow to spare the room.
    pub fn sidebar_collapsed(&self, window: &Window) -> bool {
        self.settings().sidebar_collapsed || f32::from(window.viewport_size().width) < theme::SIDEBAR_AUTO_COLLAPSE
    }

    /// The active loan the current screen is about, for context-aware
    /// shortcuts: the open loan, or a client's only active loan.
    pub fn context_loan(&self) -> Option<i64> {
        let active = |id: &i64| self.snap.loan(*id).is_some_and(|l| l.status == crate::domain::ledger::ContractStatus::Active);
        match self.route {
            Route::Loan(id) if active(&id) => Some(id),
            Route::Client(cid) => {
                let ids: Vec<i64> = self.snap.loans_for_client(cid).map(|l| l.id).filter(active).collect();
                (ids.len() == 1).then(|| ids[0])
            }
            _ => None,
        }
    }

    /// Opens the record behind a clicked table row. A click that ends a drag
    /// selecting text in the row is left alone so the text can be copied.
    pub fn open_row(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if !gpui_kit::base::TextSelection::selected_text(window, cx).trim().is_empty() {
            return;
        }
        self.navigate(route, window, cx);
    }

    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        if route != self.route {
            self.history.push(self.route);
            if self.history.len() > 50 {
                self.history.remove(0);
            }
            self.route = route;
            self.chart_hover = None;
            self.analytics_hover = None;
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn go_back(&mut self, _: &GoBack, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prev) = self.history.pop() {
            self.route = prev;
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    /// Message for a failed write, formatted for the user.
    pub fn error_message(&self, e: &anyhow::Error) -> String {
        if let Some(c) = e.downcast_ref::<InsufficientCash>() {
            let amount = self.settings().money().format(c.available);
            return match c.account {
                None => tr!(
                    "Only {amount} is available to lend on that date (capital first, then interest). Add capital in Analytics first.",
                    "Solo hay {amount} disponible para prestar en esa fecha (primero capital, luego interés). Agrega capital en Análisis primero."
                ),
                Some(Account::Capital) => {
                    tr!("Only {amount} of available capital on that date.", "Solo hay {amount} de capital disponible en esa fecha.")
                }
                Some(Account::Interest) => {
                    tr!("Only {amount} of available interest on that date.", "Solo hay {amount} de interés disponible en esa fecha.")
                }
            };
        }
        if let Some(CashShortfall(s)) = e.downcast_ref::<CashShortfall>() {
            let when = self.settings().date_style.format(s.date);
            let balance = self.settings().money().format(s.balance);
            let short = self.settings().money().format(s.balance.abs());
            return match s.account {
                Some(a) => {
                    let account = a.label().to_lowercase();
                    tr!(
                        "This would leave {account} at {balance} on {when}, because that money was already lent, spent or withdrawn. Change or delete that later movement first.",
                        "Esto dejaría el {account} en {balance} el {when}, porque ese dinero ya se prestó, gastó o retiró. Cambia o elimina primero ese movimiento posterior."
                    )
                }
                None => tr!(
                    "There wouldn't be enough money for the loan on {when} ({short} short). Change that loan or add capital first.",
                    "No alcanzaría el dinero para el préstamo del {when} (faltarían {short}). Cambia ese préstamo o agrega capital primero."
                ),
            };
        }
        if let Some(v) = e.downcast_ref::<Invalid>() {
            return v.0.clone();
        }
        log::error!("write failed: {e:#}");
        tr!("Something went wrong and nothing was saved. {e}", "Algo salió mal y no se guardó nada. {e}")
    }

    pub fn notify_success(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(msg.into()), cx);
    }

    pub fn notify_error(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(msg.into()), cx);
    }

    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), window: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.snap.settings.clone();
        let old = s.clone();
        f(&mut s);
        if let Err(e) = self.store.save_settings(&s) {
            let msg = self.error_message(&e);
            self.notify_error(msg, window, cx);
            return;
        }
        if s.theme != old.theme {
            theme::apply(s.theme, cx);
        }
        self.reload(cx);
        if s.language != old.language {
            self.apply_language(window, cx);
        }
    }

    /// Saves the name printed on receipts. Empty means the app name.
    pub fn save_receipt_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.receipt_name_input.read(cx).value().trim().to_string();
        if name == self.settings().receipt_name {
            return;
        }
        self.update_settings(|s| s.receipt_name = name.clone(), window, cx);
        if self.settings().receipt_name != name {
            // Refused (too long); show what is saved.
            let current = self.settings().receipt_name.clone();
            self.receipt_name_input.update(cx, |s, cx| s.set_value(current, window, cx));
            return;
        }
        let shown = self.settings().receipt_title();
        self.notify_success(tr!("Receipts will show “{shown}”", "Los recibos mostrarán “{shown}”"), window, cx);
    }

    /// Saves the company address or phone from its field. Empty is allowed:
    /// receipts then leave it out.
    pub fn save_company(&mut self, which: CompanyField, window: &mut Window, cx: &mut Context<Self>) {
        let (input, saved) = match which {
            CompanyField::Address => (&self.company_address_input, self.settings().company_address.clone()),
            CompanyField::Phone => (&self.company_phone_input, self.settings().company_phone.clone()),
        };
        let input = input.clone();
        let value = input.read(cx).value().trim().to_string();
        if value == saved {
            return;
        }
        self.update_settings(
            |s| match which {
                CompanyField::Address => s.company_address = value.clone(),
                CompanyField::Phone => s.company_phone = value.clone(),
            },
            window,
            cx,
        );
        let now = match which {
            CompanyField::Address => self.settings().company_address.clone(),
            CompanyField::Phone => self.settings().company_phone.clone(),
        };
        if now != value {
            // Refused (too long); show what is saved.
            input.update(cx, |s, cx| s.set_value(now, window, cx));
            return;
        }
        let msg = match which {
            CompanyField::Address => t("Company address saved", "Dirección de la empresa guardada"),
            CompanyField::Phone => t("Company phone saved", "Teléfono de la empresa guardado"),
        };
        self.notify_success(msg, window, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings(|s| s.sidebar_collapsed = !s.sidebar_collapsed, window, cx);
    }

    /// Opens a file or folder with Windows, or shows a file in Explorer
    /// (`reveal`). A failure is shown, not only logged.
    pub(crate) fn open_path(&mut self, path: PathBuf, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rx = if reveal { crate::shell::reveal(&path) } else { crate::shell::open(&path) };
        cx.spawn_in(window, async move |this, cx| {
            if let Err(why) = crate::shell::wait(rx, cx.background_executor().clone()).await {
                let shown = path.display().to_string();
                let _ = this.update_in(cx, |app, window, cx| {
                    app.notify_error(
                        tr!(
                            "Windows couldn't open {shown} ({why}). It is saved there; open it from File Explorer.",
                            "Windows no pudo abrir {shown} ({why}). Está guardado ahí; ábrelo desde el Explorador de archivos."
                        ),
                        window,
                        cx,
                    )
                });
            }
        })
        .detach();
    }

    // ----- receipts -------------------------------------------------------

    /// Writes a payment's receipt and opens it in the default browser, which
    /// shows the print dialog.
    pub fn open_receipt(&mut self, payment_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.write_receipt(payment_id) {
            Ok(path) => self.open_path(path, false, window, cx),
            Err(e) => {
                let msg = tr!("Could not create the receipt: {e:#}", "No se pudo crear el recibo: {e:#}");
                self.notify_error(msg, window, cx);
            }
        }
    }

    /// Writes a loan's receipt (what the client received and agreed to) and
    /// opens it for printing.
    pub fn open_loan_receipt(&mut self, loan_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.write_loan_receipt(loan_id) {
            Ok(path) => self.open_path(path, false, window, cx),
            Err(e) => {
                let msg = tr!("Could not create the receipt: {e:#}", "No se pudo crear el recibo: {e:#}");
                self.notify_error(msg, window, cx);
            }
        }
    }

    // ----- backup ---------------------------------------------------------

    pub fn default_backup_name(&self) -> String {
        format!("acervus-finance-backup-{}.db", chrono::Local::now().format("%Y-%m-%d-%H%M"))
    }

    // ----- Windows file windows -----------------------------------------
    //
    // Every Open/Save window goes through these so that one that can't open
    // (or opens behind the app) is reported instead of doing nothing.

    /// Starts waiting on a file window for `what`. Returns false, after
    /// telling the user, when another one is already open.
    pub(crate) fn begin_file_dialog(&mut self, what: &'static str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.file_dialog.is_some() {
            log::warn!("file dialog for {what} not opened: one is already open");
            self.notify_error(
                t(
                    "A file window is already open. It may be behind this window: look for it on the taskbar.",
                    "Ya hay una ventana de archivos abierta. Puede estar detrás de esta ventana: búscala en la barra de tareas.",
                ),
                window,
                cx,
            );
            return false;
        }
        log::info!("opening file dialog: {what}");
        self.file_dialog = Some(what);
        true
    }

    /// Ends the wait and returns the chosen path(s), or the reason the window
    /// failed (already logged). `Ok(None)` means it was cancelled.
    pub(crate) fn end_file_dialog<T, E>(&mut self, what: &str, result: Result<anyhow::Result<Option<T>>, E>) -> Result<Option<T>, String> {
        self.file_dialog = None;
        match result {
            Ok(Ok(Some(v))) => Ok(Some(v)),
            Ok(Ok(None)) => {
                log::info!("file dialog for {what} cancelled");
                Ok(None)
            }
            Ok(Err(e)) => {
                log::error!("file dialog for {what} failed: {e:#}");
                Err(format!("{e:#}"))
            }
            Err(_) => {
                log::error!("file dialog for {what} closed without an answer");
                Err(t("the window closed unexpectedly", "la ventana se cerró inesperadamente").to_string())
            }
        }
    }

    /// Shows the Windows Open window for `request` and calls `done` with the
    /// chosen path. A failure is logged and handed to `failed` with the
    /// reason; cancelling calls neither.
    pub(crate) fn pick_path(
        &mut self,
        what: &'static str,
        request: crate::file_picker::Request,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
        failed: impl FnOnce(&mut Self, String, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if !self.begin_file_dialog(what, window, cx) {
            return;
        }
        let rx = crate::file_picker::pick(crate::file_picker::owner_of(window), request);
        cx.spawn_in(window, async move |this, cx| {
            let result = crate::file_picker::wait(rx, cx.background_executor().clone()).await;
            let _ = this.update_in(cx, |app, window, cx| match app.end_pick(what, result) {
                Ok(Some(path)) => done(app, path, window, cx),
                Ok(None) => {}
                Err(why) => failed(app, why, window, cx),
            });
        })
        .detach();
    }

    /// `end_file_dialog` for the app's own Open window.
    pub(crate) fn end_pick(&mut self, what: &str, result: crate::file_picker::PickResult) -> Result<Option<PathBuf>, String> {
        self.end_file_dialog(what, Ok::<_, ()>(result.map_err(|e| anyhow::anyhow!(e.0))))
    }

    pub fn backup_now(&mut self, _: &BackupNow, window: &mut Window, cx: &mut Context<Self>) {
        if !self.begin_file_dialog("backup", window, cx) {
            return;
        }
        let dir = self.store.backups_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("could not create the backups folder: {e}");
        }
        let rx = cx.prompt_for_new_path(&dir, Some(&self.default_backup_name()));
        cx.spawn_in(window, async move |this, cx| {
            let result = rx.await;
            let _ = this.update_in(cx, |app, window, cx| match app.end_file_dialog("backup", result) {
                Ok(Some(path)) => app.write_backup(path, window, cx),
                Ok(None) => {}
                Err(why) => {
                    // The Save window couldn't open on this computer: save
                    // the backup where the app keeps them instead.
                    app.backup_to_folder(Some(why), window, cx);
                }
            });
        })
        .detach();
    }

    /// Saves a backup in the app's backups folder without asking where.
    /// `why` explains a Save window that failed.
    pub fn backup_to_folder(&mut self, why: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.store.backups_dir().join(self.default_backup_name());
        if let Some(why) = why {
            let folder = self.store.backups_dir().display().to_string();
            self.notify_error(
                tr!(
                    "The Save window couldn't open ({why}). The backup is being saved in {folder} instead.",
                    "La ventana Guardar no se pudo abrir ({why}). La copia se está guardando en {folder}."
                ),
                window,
                cx,
            );
        }
        self.write_backup(path, window, cx);
    }

    pub fn write_backup(&mut self, mut path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // The Save dialog may drop the suggested extension.
        if path.extension().is_none() {
            path.set_extension("db");
        }
        match self.store.backup_to(&path) {
            Ok(()) => {
                log::info!("backup written and verified");
                if let Err(e) = self.store.record_backup_time() {
                    log::warn!("could not record backup time: {e}");
                }
                let shown = path.display().to_string();
                self.notify_success(tr!("Backup saved and verified: {shown}", "Copia guardada y verificada: {shown}"), window, cx);
                self.reload(cx);
            }
            Err(e) => {
                log::error!("backup failed: {e:#}");
                let msg = tr!("Backup failed: {e:#}", "La copia falló: {e:#}");
                self.notify_error(msg, window, cx);
            }
        }
    }
}

impl Focusable for VzApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for VzApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        i18n::set(self.snap.settings.language);
        crate::ui::components::begin_selectable_frame();
        let p = palette(cx);
        let gutter = theme::Width::gutter(f32::from(window.viewport_size().width));
        let content = match self.route {
            Route::Overview => self.render_overview(window, cx),
            Route::Clients => self.render_clients(window, cx),
            Route::Client(id) => self.render_client(id, window, cx),
            Route::Loans => self.render_loans(window, cx),
            Route::Loan(id) => self.render_loan(id, window, cx),
            Route::Payments => self.render_payments(window, cx),
            Route::Analytics => self.render_analytics(window, cx),
            Route::Settings => self.render_settings(window, cx),
        };
        let banner = self.load_error.clone().map(|e| {
            div()
                .px(px(space::XXL))
                .py(px(space::M))
                .bg(p.negative_tint)
                .text_color(p.negative)
                .child(crate::ui::components::sel(tr!("Your data could not be loaded: {e}", "No se pudieron cargar tus datos: {e}")))
        });
        div()
            .id("vz-root")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewPayment, w, cx| {
                let loan = this.context_loan();
                this.open_payment_form(loan, w, cx)
            }))
            .on_action(cx.listener(|this, _: &NewLoan, w, cx| {
                let client = match this.route {
                    Route::Client(id) if this.snap.client(id).is_some_and(|c| !c.is_archived()) => Some(id),
                    _ => None,
                };
                this.open_loan_form(client, w, cx)
            }))
            .on_action(cx.listener(|this, _: &NewClient, w, cx| this.open_client_form(None, w, cx)))
            .on_action(cx.listener(|this, _: &NewExpense, w, cx| this.open_entry_form(EntryKind::Expense, None, w, cx)))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::backup_now))
            .on_action(cx.listener(|this, _: &GoOverview, w, cx| this.navigate(Route::Overview, w, cx)))
            .on_action(cx.listener(|this, _: &GoClients, w, cx| this.navigate(Route::Clients, w, cx)))
            .on_action(cx.listener(|this, _: &GoLoans, w, cx| this.navigate(Route::Loans, w, cx)))
            .on_action(cx.listener(|this, _: &GoPayments, w, cx| this.navigate(Route::Payments, w, cx)))
            .on_action(cx.listener(|this, _: &GoAnalytics, w, cx| this.navigate(Route::Analytics, w, cx)))
            .on_action(cx.listener(|this, _: &GoSettings, w, cx| this.navigate(Route::Settings, w, cx)))
            .on_action(cx.listener(|this, _: &FocusSearch, w, cx| {
                if this.route != Route::Payments {
                    this.navigate(Route::Clients, w, cx);
                    this.client_search.update(cx, |s, cx| s.focus(w, cx));
                } else {
                    this.payment_search.update(cx, |s, cx| s.focus(w, cx));
                }
            }))
            .size_full()
            .flex()
            .flex_row()
            .bg(p.ground)
            .text_color(p.text)
            .typeset(theme::text::BODY)
            .child(self.render_sidebar(window, cx))
            .child(
                v_flex().flex_1().min_w_0().h_full().children(banner).child(
                    div()
                        .id(SharedString::from(format!("scroll-{:?}", self.route.section())))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(div().w_full().px(px(gutter)).pt(px(gutter.min(space::XXL - 4.))).pb(px(48.)).child(content)),
                ),
            )
    }
}
