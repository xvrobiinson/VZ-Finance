//! The root view: owns the store and the current snapshot, routes between
//! screens, and hosts the sidebar. Screens render from the snapshot only.

mod dialogs;
mod sidebar;

use std::path::PathBuf;
use std::rc::Rc;

use chrono::NaiveDate;
use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use crate::services::settings::{CURRENCIES, Settings};
use crate::ui::forms::loan::Choice;
use crate::services::{InsufficientCash, Invalid, Snapshot, Store};
use crate::ui::theme::{self, FONT, palette, space};
use crate::utils::dates::today;

actions!(
    vz,
    [
        NewPayment,
        NewLoan,
        NewClient,
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
    pub(crate) loan_filter: LoanFilter,
    pub(crate) show_archived: bool,
    pub(crate) chart_hover: Option<usize>,
    pub(crate) analytics_hover: Option<usize>,
    pub(crate) load_error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl VzApp {
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (snap, load_error) = match store.snapshot() {
            Ok(s) => (s, None),
            Err(e) => (Snapshot::default(), Some(format!("{e:#}"))),
        };
        let client_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search clients by name, phone or email"));
        let payment_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search payments by client or note"));
        let currency_choices: Vec<Choice> = CURRENCIES
            .iter()
            .enumerate()
            .map(|(i, c)| Choice { id: i as i64, title: format!("{} · {}", c.code, c.name).into() })
            .collect();
        let selected = CURRENCIES.iter().position(|c| c.code == snap.settings.currency_code).map(IndexPath::new);
        let currency_select = cx.new(|cx| SelectState::new(currency_choices, selected, window, cx));
        let subs = vec![
            cx.subscribe_in(&currency_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, window, cx| {
                if let SelectEvent::Confirm(Some(i)) = ev {
                    let code = CURRENCIES[*i as usize].code.to_string();
                    this.update_settings(|s| s.currency_code = code, window, cx);
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
        Self {
            store,
            snap: Rc::new(snap),
            today: today(),
            route: Route::Overview,
            history: Vec::new(),
            focus,
            client_search,
            payment_search,
            currency_select,
            loan_filter: LoanFilter::default(),
            show_archived: false,
            chart_hover: None,
            analytics_hover: None,
            load_error,
            _subscriptions: subs,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.snap.settings
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.today = today();
        match self.store.snapshot() {
            Ok(s) => {
                self.snap = Rc::new(s);
                self.load_error = None;
            }
            Err(e) => self.load_error = Some(format!("{e:#}")),
        }
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
            return format!(
                "Only {} of capital is available on that date. Add capital in Analytics first.",
                self.settings().money().format(c.available)
            );
        }
        if let Some(v) = e.downcast_ref::<Invalid>() {
            return v.0.clone();
        }
        log::error!("write failed: {e:#}");
        format!("Something went wrong and nothing was saved. {e}")
    }

    pub fn notify_success(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::success(msg.into()), cx);
    }

    pub fn notify_error(&self, msg: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
        window.push_notification(Notification::error(msg.into()), cx);
    }

    pub fn update_settings(&mut self, f: impl FnOnce(&mut Settings), window: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.snap.settings.clone();
        let old_theme = s.theme;
        f(&mut s);
        if let Err(e) = self.store.save_settings(&s) {
            let msg = self.error_message(&e);
            self.notify_error(msg, window, cx);
            return;
        }
        if s.theme != old_theme {
            theme::apply(s.theme, cx);
        }
        self.reload(cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.update_settings(|s| s.sidebar_collapsed = !s.sidebar_collapsed, window, cx);
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

    pub fn write_backup(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.backup_to(&path) {
            Ok(()) => {
                if let Err(e) = self.store.record_backup_time() {
                    log::warn!("could not record backup time: {e}");
                }
                self.notify_success(format!("Backup saved and verified: {}", path.display()), window, cx);
                self.reload(cx);
            }
            Err(e) => {
                let msg = format!("Backup failed: {e:#}");
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
                .child(format!("Your data could not be loaded: {e}"))
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
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .children(banner)
                    .child(
                        div().id(SharedString::from(format!("scroll-{:?}", self.route.section()))).flex_1().min_h_0().overflow_y_scroll().child(
                            div()
                                .w_full()
                                .max_w(px(1440.))
                                .mx_auto()
                                .px(px(space::XXL + 4.))
                                .pt(px(space::XXL - 4.))
                                .pb(px(48.))
                                .child(content),
                        ),
                    ),
            )
    }
}
