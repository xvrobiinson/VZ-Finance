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
use crate::ui::theme::{self, FONT, palette, space};
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
    pub(crate) route: Route,
    history: Vec<Route>,
    pub(crate) focus: FocusHandle,
    pub(crate) client_search: Entity<InputState>,
    pub(crate) payment_search: Entity<InputState>,
    pub(crate) currency_select: Entity<SelectState<Vec<Choice>>>,
    pub(crate) business_name_input: Entity<InputState>,
    pub(crate) loan_filter: LoanFilter,
    pub(crate) show_archived: bool,
    pub(crate) chart_hover: Option<usize>,
    pub(crate) analytics_hover: Option<usize>,
    pub(crate) load_error: Option<String>,
    /// The business logo, decoded once per upload.
    pub(crate) logo: Option<Arc<Image>>,
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
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (snap, load_error) = match store.snapshot() {
            Ok(s) => (s, None),
            Err(e) => (Snapshot::default(), Some(format!("{e:#}"))),
        };
        i18n::set(snap.settings.language);
        window.set_window_title(&snap.settings.business_name);
        let client_search = cx.new(|cx| InputState::new(window, cx));
        let payment_search = cx.new(|cx| InputState::new(window, cx));
        let business_name_input = cx.new(|cx| InputState::new(window, cx).default_value(snap.settings.business_name.clone()));
        let selected = CURRENCIES.iter().position(|c| c.code == snap.settings.currency_code).map(IndexPath::new);
        let currency_select = cx.new(|cx| SelectState::new(currency_choices(), selected, window, cx));
        let subs = vec![
            cx.subscribe_in(&currency_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, window, cx| {
                if let SelectEvent::Confirm(Some(i)) = ev {
                    let code = CURRENCIES[*i as usize].code.to_string();
                    this.update_settings(|s| s.currency_code = code, window, cx);
                }
            }),
            cx.subscribe_in(&business_name_input, window, |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } | InputEvent::Blur => this.save_business_name(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
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
            today: today(),
            route: Route::Overview,
            history: Vec::new(),
            focus,
            client_search,
            payment_search,
            currency_select,
            business_name_input,
            loan_filter: LoanFilter::default(),
            show_archived: false,
            chart_hover: None,
            analytics_hover: None,
            load_error,
            logo,
            _subscriptions: subs,
        };
        app.apply_language(window, cx);
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
        match self.store.snapshot() {
            Ok(s) => {
                self.logo = logo_image(&s, self.logo.take());
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
        let side = if self.settings().sidebar_collapsed { theme::SIDEBAR_COLLAPSED } else { theme::SIDEBAR_WIDTH };
        let viewport = f32::from(window.viewport_size().width);
        (viewport - side - 2.0 * (space::XXL + 4.0)).min(1440.0 - 2.0 * (space::XXL + 4.0))
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
        if s.business_name != old.business_name {
            window.set_window_title(&s.business_name);
        }
    }

    pub fn save_business_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.business_name_input.read(cx).value().trim().to_string();
        if name == self.settings().business_name {
            return;
        }
        if name.is_empty() {
            let current = self.settings().business_name.clone();
            self.business_name_input.update(cx, |s, cx| s.set_value(current, window, cx));
            return;
        }
        self.update_settings(|s| s.business_name = name, window, cx);
        self.notify_success(t("Business name saved", "Nombre del negocio guardado"), window, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings(|s| s.sidebar_collapsed = !s.sidebar_collapsed, window, cx);
    }

    // ----- receipts -------------------------------------------------------

    /// Writes a payment's receipt and opens it in the default browser, which
    /// shows the print dialog.
    pub fn open_receipt(&mut self, payment_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.write_receipt(payment_id) {
            Ok(path) => cx.open_with_system(&path),
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
            Ok(path) => cx.open_with_system(&path),
            Err(e) => {
                let msg = tr!("Could not create the receipt: {e:#}", "No se pudo crear el recibo: {e:#}");
                self.notify_error(msg, window, cx);
            }
        }
    }

    // ----- backup ---------------------------------------------------------

    pub fn default_backup_name(&self) -> String {
        format!("vz-finance-backup-{}.db", chrono::Local::now().format("%Y-%m-%d-%H%M"))
    }

    pub fn backup_now(&mut self, _: &BackupNow, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.store.backups_dir();
        let _ = std::fs::create_dir_all(&dir);
        let rx = cx.prompt_for_new_path(&dir, Some(&self.default_backup_name()));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let _ = this.update_in(cx, |app, window, cx| app.write_backup(path, window, cx));
        })
        .detach();
    }

    pub fn write_backup(&mut self, mut path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // The Save dialog may drop the suggested extension.
        if path.extension().is_none() {
            path.set_extension("db");
        }
        match self.store.backup_to(&path) {
            Ok(()) => {
                if let Err(e) = self.store.record_backup_time() {
                    log::warn!("could not record backup time: {e}");
                }
                let shown = path.display().to_string();
                self.notify_success(tr!("Backup saved and verified: {shown}", "Copia guardada y verificada: {shown}"), window, cx);
                self.reload(cx);
            }
            Err(e) => {
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
        let p = palette(cx);
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
                .child(tr!("Your data could not be loaded: {e}", "No se pudieron cargar tus datos: {e}"))
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
            .font_family(FONT)
            .text_size(px(theme::size::BODY))
            .line_height(px(20.))
            .child(self.render_sidebar(window, cx))
            .child(v_flex().flex_1().min_w_0().h_full().children(banner).child(
                div().id(SharedString::from(format!("scroll-{:?}", self.route.section()))).flex_1().min_h_0().overflow_y_scroll().child(
                    div().w_full().max_w(px(1440.)).mx_auto().px(px(space::XXL + 4.)).pt(px(space::XXL - 4.)).pb(px(48.)).child(content),
                ),
            ))
    }
}
