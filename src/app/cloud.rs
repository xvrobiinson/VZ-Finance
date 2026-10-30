//! The cloud inside the app: signing in, following a business, and keeping
//! this PC's copy and the cloud in step (docs/DESKTOP-SYNC-PLAN.md in the PWA
//! repo).
//!
//! Signed out, nothing here runs and the app works on the books kept on this
//! PC, as before. Signed in, the sync worker runs; once the person picks a
//! business, the app switches to that business's own database file (a copy
//! of the cloud's books) and every save also goes to the cloud. Stopping
//! (or signing out) switches back to the books on this PC, untouched.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt as _;
use gpui_kit::component::input::InputState;
use gpui_kit::*;

use super::VzApp;
use crate::i18n::t;
use crate::services::{Invalid, Store};
use crate::sync::auth::{self, Endpoints};
use crate::sync::change::{Change, Saved};
use crate::sync::outbox::Outcome;
use crate::sync::replica::{self, Entity as Record};
use crate::sync::vault::{self, Stored};
use crate::sync::wire::{Membership, Need, Role, Snapshot};
use crate::sync::worker::{Config, Event, Worker};
use crate::tr;

/// The business this PC follows, kept in the PC's own books.
const LINKED: &str = "cloud_business";

/// Where the cloud is. Debug builds can point at a local copy of the
/// backend for testing (`VZ_SYNC_CLOUD`, `VZ_SYNC_ORIGIN`).
fn endpoints() -> Endpoints {
    if cfg!(debug_assertions)
        && let (Ok(cloud), Ok(origin)) = (std::env::var("VZ_SYNC_CLOUD"), std::env::var("VZ_SYNC_ORIGIN"))
    {
        return Endpoints::new(&cloud, &origin);
    }
    Endpoints::production()
}

/// The sign-in form in Settings.
pub(crate) struct SignIn {
    pub(crate) email: Entity<InputState>,
    pub(crate) code: Entity<InputState>,
    /// The code was sent to this address; the form asks for it.
    pub(crate) code_sent_to: Option<String>,
    pub(crate) busy: bool,
    pub(crate) error: Option<String>,
    /// A Google sign-in is waiting for the browser; setting it stops the wait.
    pub(crate) google: Option<Arc<AtomicBool>>,
    /// The sign-in screen is showing.
    pub(crate) open: bool,
}

impl SignIn {
    pub(crate) fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            email: cx.new(|cx| InputState::new(window, cx)),
            code: cx.new(|cx| InputState::new(window, cx)),
            code_sent_to: None,
            busy: false,
            error: None,
            google: None,
            open: false,
        }
    }
}

/// What to do once the cloud has a new record and its number.
#[derive(Clone, Copy)]
enum Then {
    PaymentReceipt,
    LoanReceipt,
    /// The record's page is open under the number this PC gave it: show it
    /// under the cloud's number instead.
    Reopen(super::Route),
}

impl Then {
    fn record(self) -> Option<Record> {
        use super::Route;
        match self {
            Then::PaymentReceipt => Some(Record::Payment),
            Then::LoanReceipt | Then::Reopen(Route::Loan(_)) => Some(Record::Loan),
            Then::Reopen(Route::Client(_)) => Some(Record::Client),
            Then::Reopen(_) => None,
        }
    }
}

/// A saved change the app waits on: its operation id until the cloud
/// answers, then the record's cloud id until the copy with it arrives.
struct Waiting {
    op: String,
    remote: Option<String>,
    then: Then,
}

/// The signed-in link to the cloud.
pub(crate) struct Cloud {
    pub(crate) endpoints: Endpoints,
    pub(crate) email: String,
    worker: Worker,
    pub(crate) online: bool,
    /// The businesses the person belongs to, once the cloud has said.
    pub(crate) businesses: Option<Vec<Membership>>,
    /// The business this PC follows, if any.
    pub(crate) linked: Option<String>,
    /// The last copy of the linked business's books.
    last: Option<Box<Snapshot>>,
    sending: bool,
    /// The operation id of the change saved last (none when it wasn't sent
    /// to the cloud), for its receipt and page.
    last_op: Option<String>,
    pub(crate) update_required: Option<String>,
    /// Receipts and pages waiting for the cloud's number.
    waiting: Vec<Waiting>,
    /// Backups to Google Drive.
    pub(crate) drive: super::drive::Drive,
    _events: Task<()>,
}

/// What the sidebar and Settings show about the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// Signed in, no business chosen on this PC yet.
    NotLinked,
    Synced,
    Sending(usize),
    /// Offline; `n` changes wait to be sent.
    Offline(usize),
    /// Changes the cloud refused or may not have received.
    NeedsReview(usize),
    UpdateRequired,
}

impl VzApp {
    /// Starts syncing at launch when this PC has a remembered sign-in.
    pub(crate) fn resume_cloud(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ep = endpoints();
        let Some(stored) = vault::load(&ep.cloud) else { return };
        let linked = self.store.pref(LINKED).ok().flatten();
        self.start_cloud(ep, stored, linked, window, cx);
    }

    fn start_cloud(&mut self, ep: Endpoints, stored: Stored, linked: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let config = Config { endpoints: ep.clone(), client_version: crate::sync::CLIENT_VERSION.into() };
        let email = stored.email.clone();
        let (worker, mut rx) = match Worker::start(config, stored) {
            Ok(w) => w,
            Err(e) => {
                log::error!("sync could not start: {e}");
                return;
            }
        };
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Some(ev) = rx.next().await {
                if this.update_in(cx, |app, window, cx| app.on_cloud_event(ev, window, cx)).is_err() {
                    break;
                }
            }
        });
        self.cloud = Some(Cloud {
            endpoints: ep,
            email,
            worker,
            online: false,
            businesses: None,
            linked: None,
            last: None,
            sending: false,
            last_op: None,
            update_required: None,
            waiting: Vec::new(),
            drive: Default::default(),
            _events: events,
        });
        if let Some(id) = linked {
            self.link_business(&id, window, cx);
        }
    }

    /// True when this PC follows a business in the cloud.
    pub(crate) fn is_linked(&self) -> bool {
        self.cloud.as_ref().is_some_and(|c| c.linked.is_some())
    }

    /// The person's role in the business this PC follows; `None` on this
    /// PC's own books, where everything is allowed.
    pub(crate) fn role(&self) -> Option<Role> {
        let c = self.cloud.as_ref().filter(|c| c.linked.is_some())?;
        match &c.last {
            Some(snap) => Some(snap.me.role),
            // Before the first copy arrives: the role kept with the books.
            None => Some(self.store.cloud_link().ok().flatten().map_or(Role::Viewer, |l| l.role)),
        }
    }

    /// Whether the person may make changes that need `need`. Screens hide
    /// what it doesn't allow (the cloud refuses it anyway).
    pub(crate) fn can(&self, need: Need) -> bool {
        self.role().is_none_or(|r| r.allows(need))
    }

    pub(crate) fn sync_status(&self) -> Option<SyncStatus> {
        let c = self.cloud.as_ref()?;
        if c.update_required.is_some() {
            return Some(SyncStatus::UpdateRequired);
        }
        if c.linked.is_none() {
            return Some(SyncStatus::NotLinked);
        }
        let refused = self.store.outbox_refused().map(|r| r.len()).unwrap_or(0);
        if refused > 0 {
            return Some(SyncStatus::NeedsReview(refused));
        }
        let waiting = self.store.outbox_waiting().map(|w| w.len()).unwrap_or(0);
        Some(match (c.online, waiting) {
            (false, n) => SyncStatus::Offline(n),
            (true, 0) => SyncStatus::Synced,
            (true, n) => SyncStatus::Sending(n),
        })
    }

    // ----- signing in -----------------------------------------------------

    pub(crate) fn send_sign_in_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let email = self.sign_in.email.read(cx).value().trim().to_string();
        if !email.contains('@') {
            self.sign_in.error = Some(t("Enter your email address", "Escribe tu correo").into());
            cx.notify();
            return;
        }
        self.sign_in.busy = true;
        self.sign_in.error = None;
        cx.notify();
        let ep = endpoints();
        let sending = email.clone();
        let task = cx.background_spawn(async move { auth::send_code(&ep, &sending) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.sign_in.busy = false;
                match result {
                    Ok(()) => {
                        app.sign_in.code_sent_to = Some(email);
                        app.sign_in.code.update(cx, |s, cx| {
                            s.set_value("", window, cx);
                            s.focus(window, cx);
                        });
                    }
                    Err(e) => app.sign_in.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn finish_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(email) = self.sign_in.code_sent_to.clone() else { return };
        let code: String = self.sign_in.code.read(cx).value().chars().filter(char::is_ascii_digit).collect();
        if code.len() != 6 {
            self.sign_in.error = Some(t("Enter the 6-digit code from the email", "Escribe el código de 6 dígitos del correo").into());
            cx.notify();
            return;
        }
        self.sign_in.busy = true;
        self.sign_in.error = None;
        cx.notify();
        let ep = endpoints();
        let (e2, ep2) = (email.clone(), ep.clone());
        let task = cx.background_spawn(async move { auth::sign_in(&ep2, &e2, &code) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |app, window, cx| {
                app.sign_in.busy = false;
                match result {
                    Ok(session) => {
                        let stored = Stored { email, session };
                        if let Err(e) = vault::save(&ep.cloud, &stored) {
                            log::warn!("sign-in not remembered: {e}");
                        }
                        app.sign_in.code_sent_to = None;
                        app.start_cloud(ep, stored, None, window, cx);
                        app.signed_in(window, cx);
                    }
                    Err(e) => app.sign_in.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Signs in with Google in the person's browser; the app waits until the
    /// browser comes back (or the person cancels).
    pub(crate) fn sign_in_with_google(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sign_in.google.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.sign_in.google = Some(stop.clone());
        self.sign_in.error = None;
        cx.notify();
        let ep = endpoints();
        cx.spawn_in(window, async move |this, cx| {
            let (tx, rx) = futures::channel::oneshot::channel();
            let (ep2, stop2) = (ep.clone(), stop.clone());
            // The wait can take minutes, so it gets its own thread rather than
            // one of the app's shared ones.
            let spawned = std::thread::Builder::new().name("google-sign-in".into()).spawn(move || {
                let result = auth::start_google(&ep2).and_then(|g| {
                    let opened = crate::shell::browse(&g.url).recv().unwrap_or_else(|_| Err("stopped".into()));
                    if let Err(why) = opened {
                        log::warn!("browser not opened: {why}");
                        return Err(auth::AuthError::Refused(t("The browser could not be opened", "No se pudo abrir el navegador").into()));
                    }
                    auth::finish_google(&ep2, g, &stop2).and_then(|done| match done {
                        auth::GoogleDone::SignedIn(email, session) => Ok((email, session)),
                        auth::GoogleDone::Linked => Err(auth::AuthError::GoogleNotFinished),
                    })
                });
                let _ = tx.send(result);
            });
            let result = match spawned {
                Ok(_) => rx.await.unwrap_or(Err(auth::AuthError::GoogleNotFinished)),
                Err(e) => Err(auth::AuthError::Refused(e.to_string())),
            };
            let _ = this.update_in(cx, |app, window, cx| {
                // Cancelled: this attempt's result no longer matters.
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                app.sign_in.google = None;
                match result {
                    Ok((email, session)) => {
                        let stored = Stored { email, session };
                        if let Err(e) = vault::save(&ep.cloud, &stored) {
                            log::warn!("sign-in not remembered: {e}");
                        }
                        app.sign_in.code_sent_to = None;
                        app.start_cloud(ep, stored, None, window, cx);
                        app.signed_in(window, cx);
                    }
                    Err(e) => app.sign_in.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops waiting for a Google sign-in.
    pub(crate) fn cancel_google(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.sign_in.google.take() {
            stop.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Shows the sign-in screen.
    pub(crate) fn open_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cloud.is_some() {
            return;
        }
        self.sign_in.open = true;
        self.sign_in.error = None;
        self.sign_in.email.update(cx, |s, cx| s.focus(window, cx));
        cx.notify();
    }

    /// Leaves the sign-in screen; a Google sign-in still waiting is stopped.
    pub(crate) fn close_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_google(cx);
        self.sign_in.open = false;
        self.sign_in.busy = false;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// After signing in: back to the app, in Settings, where the person
    /// picks the business for this PC.
    fn signed_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sign_in.open = false;
        self.navigate(super::Route::Settings, window, cx);
    }

    /// Back to the email step (wrong address, or no code arrived).
    pub(crate) fn restart_sign_in(&mut self, cx: &mut Context<Self>) {
        self.sign_in.code_sent_to = None;
        self.sign_in.error = None;
        cx.notify();
    }

    /// Signs out on this PC: the session and the ID photos kept here are
    /// deleted, and the app goes back to the books kept on this PC. Changes
    /// not yet sent stay in the linked copy for next time.
    pub(crate) fn sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cloud) = self.cloud.take() else { return };
        if let Some(stored) = vault::load(&cloud.endpoints.cloud) {
            let ep = cloud.endpoints.clone();
            cx.background_spawn(async move { auth::sign_out(&ep, &stored.session) }).detach();
        }
        vault::forget(&cloud.endpoints.cloud);
        if cloud.linked.is_some() {
            if let Err(e) = self.store.forget_cloud_photos() {
                log::error!("photos not deleted on sign-out: {e:#}");
            }
            self.use_local_books(false, window, cx);
        }
        self.notify_success(t("Signed out on this PC", "Sesión cerrada en esta PC"), window, cx);
        cx.notify();
    }

    // ----- choosing a business ---------------------------------------------

    /// Switches this PC to a business's copy of the books.
    pub(crate) fn link_business(&mut self, business_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let opened = replica::path_for(business_id).and_then(Store::open);
        let store = match opened {
            Ok(s) => s,
            Err(e) => {
                let msg = tr!("Couldn't open this business's copy: {e:#}", "No se pudo abrir la copia de este negocio: {e:#}");
                self.notify_error(msg, window, cx);
                return;
            }
        };
        if let Err(e) = self.local_books().and_then(|mut s| s.set_pref(LINKED, Some(business_id))) {
            log::warn!("linked business not remembered: {e:#}");
        }
        self.swap_store(store, window, cx);
        if let Some(c) = &mut self.cloud {
            c.linked = Some(business_id.to_string());
            c.last = None;
            c.worker.open(business_id);
        }
        self.pump(window, cx);
    }

    /// Stops following the cloud on this PC and shows the books kept here.
    /// `forget`: also stop opening the business at the next launch.
    pub(crate) fn use_local_books(&mut self, forget: bool, window: &mut Window, cx: &mut Context<Self>) {
        let local = match self.local_books() {
            Ok(s) => s,
            Err(e) => {
                let msg = tr!("Couldn't open the books on this PC: {e:#}", "No se pudieron abrir los registros de esta PC: {e:#}");
                self.notify_error(msg, window, cx);
                return;
            }
        };
        let mut local = local;
        if forget && let Err(e) = local.set_pref(LINKED, None) {
            log::warn!("linked business not forgotten: {e:#}");
        }
        if let Some(c) = &mut self.cloud {
            c.linked = None;
            c.last = None;
        }
        self.swap_store(local, window, cx);
    }

    /// The books kept on this PC (the database the app opened at launch).
    fn local_books(&self) -> anyhow::Result<Store> {
        Store::open(self.local_path.clone())
    }

    fn swap_store(&mut self, store: Store, window: &mut Window, cx: &mut Context<Self>) {
        self.store = store;
        self.history.clear();
        self.route = super::Route::Overview;
        self.reload(cx);
        self.reset_setting_inputs(window, cx);
        crate::ui::theme::apply(self.settings().theme, cx);
    }

    // ----- saving -----------------------------------------------------------

    /// Saves a change: here at once and, when this PC follows a business,
    /// for the cloud too. Offline, only the changes the cloud takes later can
    /// be saved (new clients, payments, expenses and income).
    pub(crate) fn save(&mut self, change: Change, window: &mut Window, cx: &mut Context<Self>) -> anyhow::Result<Saved> {
        if let Some(c) = &self.cloud
            && c.linked.is_some()
        {
            if c.update_required.is_some() {
                return Err(Invalid(
                    t("Update the app to save changes to the cloud.", "Actualiza la app para guardar cambios en la nube.").into(),
                )
                .into());
            }
            if !c.online && !change.works_offline() {
                return Err(Invalid(
                    t(
                        "This change needs a connection to the cloud. New clients, payments, expenses and income can be saved offline.",
                        "Este cambio necesita conexión con la nube. Sin conexión puedes guardar clientes nuevos, pagos, gastos e ingresos.",
                    )
                    .into(),
                )
                .into());
            }
        }
        if !self.can(change.need()) {
            return Err(Invalid(crate::sync::refusal::forbidden().into()).into());
        }
        let (saved, queued) = self.store.save(&change, self.today)?;
        if let Some(c) = &mut self.cloud {
            c.last_op = queued.as_ref().map(|q| q.op_id.clone());
        }
        if queued.is_some() {
            self.pump(window, cx);
        }
        Ok(saved)
    }

    /// Opens a new payment's receipt: at once on this PC's own books, or once
    /// the cloud has the payment and its number.
    pub(crate) fn payment_receipt_after_save(&mut self, payment_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.receipt_after_save(payment_id, Then::PaymentReceipt, window, cx);
    }

    pub(crate) fn loan_receipt_after_save(&mut self, loan_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.receipt_after_save(loan_id, Then::LoanReceipt, window, cx);
    }

    fn receipt_after_save(&mut self, local_id: i64, then: Then, window: &mut Window, cx: &mut Context<Self>) {
        if !self.wait_for_cloud(then) {
            self.then(then, local_id, window, cx);
            return;
        }
        if self.cloud.as_ref().is_some_and(|c| !c.online) {
            self.notify_success(
                t("The receipt will open once this reaches the cloud.", "El recibo se abrirá cuando esto llegue a la nube."),
                window,
                cx,
            );
        }
    }

    /// Opens the page of a record just saved. On a linked PC the record has
    /// this PC's number until the cloud accepts it; the page then follows it
    /// to the cloud's number (or goes back to the list if it's refused).
    pub(crate) fn open_saved(&mut self, route: super::Route, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(route, window, cx);
        self.wait_for_cloud(Then::Reopen(route));
    }

    /// Waits for the cloud's answer to the change saved last, when it went to
    /// the cloud. False when there is nothing to wait for.
    fn wait_for_cloud(&mut self, then: Then) -> bool {
        let Some(c) = self.cloud.as_mut().filter(|c| c.linked.is_some()) else { return false };
        let Some(op) = c.last_op.clone() else { return false };
        c.waiting.push(Waiting { op, remote: None, then });
        true
    }

    fn then(&mut self, then: Then, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        use super::Route;
        match then {
            Then::PaymentReceipt => self.open_receipt(id, window, cx),
            Then::LoanReceipt => self.open_loan_receipt(id, window, cx),
            Then::Reopen(old) => {
                let new = match old {
                    Route::Loan(_) => Route::Loan(id),
                    Route::Client(_) => Route::Client(id),
                    other => other,
                };
                self.replace_route(old, new);
            }
        }
    }

    /// Points the page and the back history at `new` where they showed `old`.
    fn replace_route(&mut self, old: super::Route, new: super::Route) {
        if self.route == old {
            self.route = new;
        }
        for r in self.history.iter_mut().filter(|r| **r == old) {
            *r = new;
        }
    }

    // ----- the worker's reports ---------------------------------------------

    fn on_cloud_event(&mut self, ev: Event, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = &mut self.cloud else { return };
        match ev {
            Event::Online(on) => {
                c.online = on;
                if on {
                    self.pump(window, cx);
                }
            }
            Event::UpdateRequired { min_version } => c.update_required = Some(min_version),
            Event::SignedOut => {
                self.notify_error(
                    t(
                        "You were signed out. Sign in again to keep syncing.",
                        "Se cerró tu sesión. Inicia sesión de nuevo para seguir sincronizando.",
                    ),
                    window,
                    cx,
                );
                self.sign_out(window, cx);
            }
            Event::Businesses(list) => c.businesses = Some(list),
            Event::Snapshot(snap) => {
                if c.linked.as_deref() != Some(snap.business.id.as_str()) {
                    return;
                }
                c.last = Some(snap);
                self.apply_last(window, cx);
                let Some(c) = &self.cloud else { return };
                if let Some(snap) = &c.last {
                    match self.store.cloud_downloads(snap) {
                        Ok(files) => c.worker.download(files),
                        Err(e) => log::error!("downloads not listed: {e:#}"),
                    }
                }
            }
            Event::Downloaded { file, bytes } => {
                if c.linked.is_some() {
                    match self.store.save_download(&file, &bytes) {
                        Ok(()) => self.reload(cx),
                        Err(e) => log::warn!("download not kept: {e:#}"),
                    }
                }
            }
            Event::Problem(p) => log::warn!("sync: {p}"),
        }
        cx.notify();
    }

    /// Applies the last copy from the cloud (with the changes still waiting
    /// on top) and opens receipts whose records have arrived.
    fn apply_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.cloud.as_ref().and_then(|c| c.last.clone()) else { return };
        if let Err(e) = self.store.apply_cloud(&snap) {
            log::error!("cloud copy not applied: {e:#}");
            let msg = tr!(
                "The latest books from the cloud couldn't be shown: {e:#}",
                "No se pudieron mostrar los registros más recientes de la nube: {e:#}"
            );
            self.notify_error(msg, window, cx);
            return;
        }
        self.reload(cx);
        let Some(c) = &mut self.cloud else { return };
        let mut ready = Vec::new();
        for w in std::mem::take(&mut c.waiting) {
            let local = w.remote.as_deref().zip(w.then.record()).and_then(|(r, e)| self.store.cloud_local_id(e, r).ok().flatten());
            match local {
                Some(id) => ready.push((w.then, id)),
                None => c.waiting.push(w),
            }
        }
        for (then, id) in ready {
            self.then(then, id, window, cx);
        }
    }

    /// Sends the oldest waiting change, then the next, until none wait or the
    /// connection is gone.
    pub(crate) fn pump(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = &mut self.cloud else { return };
        if c.sending || !c.online || c.linked.is_none() || c.update_required.is_some() {
            return;
        }
        let Some(q) = self.store.outbox_waiting().ok().and_then(|w| w.into_iter().next()) else { return };
        c.sending = true;
        let answer = c.worker.send(&q);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = answer.await.unwrap_or(Outcome::NotSent);
            let _ = this.update_in(cx, |app, window, cx| {
                if let Some(c) = &mut app.cloud {
                    c.sending = false;
                }
                let changed = match app.store.outbox_finish(&q, &outcome) {
                    Ok(changed) => changed,
                    Err(e) => {
                        log::error!("outbox not updated: {e:#}");
                        return;
                    }
                };
                match &outcome {
                    Outcome::Accepted(result) => {
                        if result["photoNotSaved"] == true {
                            app.notify_error(
                                t(
                                    "The client was saved, but the photo wasn't. Add it again.",
                                    "El cliente se guardó, pero la foto no. Agrégala de nuevo.",
                                ),
                                window,
                                cx,
                            );
                        }
                        let remote = ["paymentId", "loanId", "clientId"]
                            .iter()
                            .find_map(|k| result[*k].as_str())
                            .or(result.as_str())
                            .map(str::to_string);
                        if let Some(c) = &mut app.cloud {
                            for w in c.waiting.iter_mut().filter(|w| w.op == q.op_id) {
                                w.remote = remote.clone();
                            }
                        }
                    }
                    Outcome::Refused(e) => {
                        let why = crate::sync::refusal::describe(e, app.settings());
                        app.notify_error(tr!("The cloud didn't accept a change: {why}", "La nube no aceptó un cambio: {why}"), window, cx);
                        let gone: Vec<Then> = app.cloud.as_mut().map_or(Vec::new(), |c| {
                            let (gone, keep) = std::mem::take(&mut c.waiting).into_iter().partition(|w| w.op == q.op_id);
                            c.waiting = keep;
                            gone.into_iter().map(|w: Waiting| w.then).collect()
                        });
                        // A page showing the refused record goes back to its list.
                        for then in gone {
                            if let Then::Reopen(route) = then {
                                app.replace_route(route, route.section());
                            }
                        }
                    }
                    Outcome::NoAnswer | Outcome::NotSent => {}
                }
                if changed {
                    app.apply_last(window, cx);
                }
                // Keep going while the cloud answers; a lost answer waits for
                // the next connection (or the next minute).
                if matches!(outcome, Outcome::Accepted(_) | Outcome::Refused(_)) || (changed && matches!(outcome, Outcome::NoAnswer)) {
                    app.pump(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Sends a change the cloud refused again (after the person checked it).
    pub(crate) fn retry_refused(&mut self, op_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.store.outbox_retry(op_id, None) {
            self.notify_error(format!("{e:#}"), window, cx);
            return;
        }
        self.apply_last(window, cx);
        self.pump(window, cx);
    }

    /// Drops a refused change for good.
    pub(crate) fn discard_refused(&mut self, op_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.store.outbox_discard(op_id) {
            self.notify_error(format!("{e:#}"), window, cx);
            return;
        }
        cx.notify();
    }
}
