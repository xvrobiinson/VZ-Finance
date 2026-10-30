//! Backups to the person's Google Drive, for a signed-in PC (the Drive work
//! itself is in `sync::drive`). Off until the person turns it on, which adds
//! the `drive.file` permission to their Google connection in the browser.
//! Once on, a backup goes up right away, every day after that, and after
//! each "Back up database". The setting and the last upload are kept in the
//! books open on this PC.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::*;

use super::VzApp;
use crate::i18n::t;
use crate::sync::auth::{self, AuthError, GoogleDone};
use crate::sync::drive::{self, DriveError, Uploaded};
use crate::sync::vault;
use crate::tr;

/// "on" when backups go to Drive.
const ON: &str = "drive_backups";
/// The backups folder's Drive id.
const FOLDER: &str = "drive_folder";
/// When the last backup reached Drive (RFC 3339).
const LAST: &str = "drive_last";
/// How often the automatic backup runs.
const EVERY: chrono::Duration = chrono::Duration::hours(24);
/// After a failed automatic backup, the wait before the next try.
const RETRY: Duration = Duration::from_secs(60 * 60);

/// Drive backups' state while the app runs.
#[derive(Default)]
pub(crate) struct Drive {
    /// Turning on: waiting for the browser; setting it stops the wait.
    pub(crate) linking: Option<Arc<AtomicBool>>,
    pub(crate) uploading: bool,
    /// Why the last attempt failed, shown in Settings until one succeeds.
    pub(crate) error: Option<String>,
    last_try: Option<Instant>,
}

impl VzApp {
    pub(crate) fn drive_on(&self) -> bool {
        self.store.pref(ON).ok().flatten().as_deref() == Some("on")
    }

    /// When the last backup reached Drive, in local time.
    pub(crate) fn drive_last(&self) -> Option<chrono::DateTime<chrono::Local>> {
        let stamp = self.store.pref(LAST).ok().flatten()?;
        chrono::DateTime::parse_from_rfc3339(&stamp).ok().map(|d| d.with_timezone(&chrono::Local))
    }

    /// Turns Drive backups on: the browser asks Google for the permission,
    /// then the first backup goes up.
    pub(crate) fn turn_on_drive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = &mut self.cloud else { return };
        if c.drive.linking.is_some() {
            return;
        }
        let Some(stored) = vault::load(&c.endpoints.cloud) else { return };
        let stop = Arc::new(AtomicBool::new(false));
        c.drive.linking = Some(stop.clone());
        c.drive.error = None;
        cx.notify();
        let ep = c.endpoints.clone();
        cx.spawn_in(window, async move |this, cx| {
            let (tx, rx) = futures::channel::oneshot::channel();
            let stop2 = stop.clone();
            // The wait can take minutes: a thread of its own.
            let spawned = std::thread::Builder::new().name("drive-link".into()).spawn(move || {
                let result = auth::start_google_link(&ep, &stored.session, &[drive::SCOPE]).and_then(|g| {
                    if let Err(why) = crate::shell::browse(&g.url).recv().unwrap_or_else(|_| Err("stopped".into())) {
                        log::warn!("browser not opened: {why}");
                        return Err(AuthError::Refused(t("The browser could not be opened", "No se pudo abrir el navegador").into()));
                    }
                    match auth::finish_google(&ep, g, &stop2)? {
                        GoogleDone::Linked => {}
                        GoogleDone::SignedIn(..) => return Err(AuthError::GoogleNotFinished),
                    }
                    // The permission must really be there (the person can
                    // untick it on Google's page).
                    let token = auth::google_token(&ep, &stored.session)?;
                    if token.has(drive::SCOPE) { Ok(()) } else { Err(AuthError::GoogleNotLinked) }
                });
                let _ = tx.send(result);
            });
            let result = match spawned {
                Ok(_) => rx.await.unwrap_or(Err(AuthError::GoogleNotFinished)),
                Err(e) => Err(AuthError::Refused(e.to_string())),
            };
            let _ = this.update_in(cx, |app, window, cx| {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let Some(c) = &mut app.cloud else { return };
                c.drive.linking = None;
                match result {
                    Ok(()) => {
                        if let Err(e) = app.store.set_pref(ON, Some("on")) {
                            log::error!("Drive backups not turned on: {e:#}");
                        }
                        app.drive_backup(false, window, cx);
                    }
                    Err(e) => c.drive.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Stops waiting for the browser while turning Drive backups on.
    pub(crate) fn cancel_drive_link(&mut self, cx: &mut Context<Self>) {
        if let Some(stop) = self.cloud.as_mut().and_then(|c| c.drive.linking.take()) {
            stop.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Turns Drive backups off. Backups already in Drive stay there.
    pub(crate) fn turn_off_drive(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.store.set_pref(ON, None) {
            log::error!("Drive backups not turned off: {e:#}");
        }
        if let Some(c) = &mut self.cloud {
            c.drive.error = None;
        }
        cx.notify();
    }

    /// The daily backup, when it's due (called every minute).
    pub(crate) fn maybe_drive_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = &self.cloud else { return };
        if !c.online || c.drive.uploading || c.drive.linking.is_some() || c.drive.last_try.is_some_and(|t| t.elapsed() < RETRY) {
            return;
        }
        if !self.drive_on() || self.drive_last().is_some_and(|last| chrono::Local::now() - last < EVERY) {
            return;
        }
        self.drive_backup(true, window, cx);
    }

    /// Sends a backup of the books open on this PC to Drive now. `quiet`
    /// (the daily one) shows no message unless it fails.
    pub(crate) fn drive_backup(&mut self, quiet: bool, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.default_backup_name();
        let Some(c) = &mut self.cloud else { return };
        if c.drive.uploading {
            return;
        }
        let Some(stored) = vault::load(&c.endpoints.cloud) else { return };
        // A verified copy in the temp folder, deleted once sent.
        let file = std::env::temp_dir().join(format!("acervus-drive-{}", name));
        if let Err(e) = self.store.backup_to(&file) {
            log::error!("Drive backup not made: {e:#}");
            let msg = tr!("Backup failed: {e:#}", "La copia falló: {e:#}");
            self.notify_error(msg, window, cx);
            return;
        }
        let Some(c) = &mut self.cloud else { return };
        c.drive.uploading = true;
        c.drive.last_try = Some(Instant::now());
        cx.notify();
        let ep = c.endpoints.clone();
        let folder = self.store.pref(FOLDER).ok().flatten();
        let task = cx.background_spawn(async move {
            let result = auth::google_token(&ep, &stored.session)
                .map_err(|e| match e {
                    AuthError::Offline => DriveError::Offline,
                    AuthError::GoogleNotLinked => DriveError::NotConnected,
                    e => DriveError::Refused(e.to_string()),
                })
                .and_then(|token| drive::backup(&token, &file, &name, folder.as_deref()));
            if let Err(e) = std::fs::remove_file(&file) {
                log::warn!("temporary backup not deleted: {e}");
            }
            result
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |app, window, cx| app.drive_backup_done(result, quiet, window, cx));
        })
        .detach();
    }

    fn drive_backup_done(&mut self, result: Result<Uploaded, DriveError>, quiet: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(c) = &mut self.cloud else { return };
        c.drive.uploading = false;
        match result {
            Ok(up) => {
                c.drive.error = None;
                let now = chrono::Local::now().to_rfc3339();
                for (key, value) in [(LAST, now.as_str()), (FOLDER, up.folder.as_str())] {
                    if let Err(e) = self.store.set_pref(key, Some(value)) {
                        log::warn!("Drive backup not recorded: {e:#}");
                    }
                }
                if let Err(e) = self.store.record_backup_time() {
                    log::warn!("could not record backup time: {e}");
                }
                log::info!("backup sent to Google Drive ({} old ones deleted)", up.pruned);
                if !quiet {
                    self.notify_success(t("Backup saved to Google Drive", "Copia guardada en Google Drive"), window, cx);
                }
                self.reload(cx);
            }
            Err(e) => {
                log::warn!("Drive backup failed: {e}");
                let msg = e.to_string();
                c.drive.error = Some(msg.clone());
                // Offline is expected now and then; the next try is soon.
                if !quiet || e != DriveError::Offline {
                    self.notify_error(msg, window, cx);
                }
            }
        }
        cx.notify();
    }
}
