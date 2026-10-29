use std::cell::Cell;
use std::path::PathBuf;

use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::{WindowExt as _, button::ButtonVariant};
use gpui_kit::*;

use super::VzApp;
use crate::domain::portfolio::{CapitalKind, EntryKind};
use crate::i18n::t;
use crate::services::Store;
use crate::sync::change::Change;
use crate::tr;
use crate::ui::forms::adjust::AdjustForm;
use crate::ui::forms::capital::CapitalForm;
use crate::ui::forms::client::{ClientForm, OnCreated};
use crate::ui::forms::entry::EntryForm;
use crate::ui::forms::focus_once;
use crate::ui::forms::loan::LoanForm;
use crate::ui::forms::payment::PaymentForm;
use crate::ui::forms::reason::{ReasonForm, ReasonKind};

/// Routes the dialog's Confirm action (Enter) to the form's submit. Returns
/// false so the dialog stays open; the form closes it after a successful save.
fn confirm<F: Submit + 'static>(form: &Entity<F>) -> impl Fn(&ClickEvent, &mut Window, &mut App) -> bool + 'static {
    let form = form.clone();
    move |_, window, cx| {
        form.update(cx, |f, cx| f.submit(window, cx));
        false
    }
}

pub trait Submit: Sized {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>);
}

macro_rules! submit_impl {
    ($($t:ty),*) => {$(
        impl Submit for $t {
            fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
                <$t>::submit(self, window, cx)
            }
        }
    )*};
}

submit_impl!(
    ClientForm,
    LoanForm,
    PaymentForm,
    CapitalForm,
    ReasonForm,
    EntryForm,
    AdjustForm,
    crate::ui::forms::reset::ResetForm,
    crate::ui::forms::receipt_text::ReceiptTextForm
);

impl VzApp {
    pub fn open_client_form(&mut self, client_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        self.client_dialog(client_id, None, window, cx);
    }

    /// A new client added from inside another form (the loan form): the
    /// dialog opens on top of it, `on_created` gets the new client's id and
    /// the app stays where it is.
    pub fn open_new_client_for(&mut self, on_created: OnCreated, window: &mut Window, cx: &mut Context<Self>) -> Entity<ClientForm> {
        self.client_dialog(None, Some(on_created), window, cx)
    }

    fn client_dialog(
        &mut self,
        client_id: Option<i64>,
        on_created: Option<OnCreated>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ClientForm> {
        let existing = client_id.and_then(|id| self.snap.client(id).cloned());
        // The photo is read only for the form that edits it.
        let photo = existing.as_ref().filter(|c| c.has_photo).and_then(|c| match self.store.client_photo(c.id) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("client photo not loaded: {e:#}");
                None
            }
        });
        let app = cx.entity().downgrade();
        let form = cx.new(|cx| ClientForm::new(app, existing.clone(), photo, on_created, window, cx));
        let title = if existing.is_some() { t("Edit client", "Editar cliente") } else { t("New client", "Nuevo cliente") };
        let first = Cell::new(true);
        let dialog_form = form.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = dialog_form.read(cx).name.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(560.)).child(dialog_form.clone()).on_ok(confirm(&dialog_form))
        });
        form
    }

    pub fn open_loan_form(&mut self, client_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        self.loan_dialog(client_id, None, window, cx);
    }

    pub fn edit_loan(&mut self, loan_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.loan_dialog(None, Some(loan_id), window, cx);
    }

    /// Opens the loan form and returns it.
    pub(crate) fn loan_dialog(
        &mut self,
        client_id: Option<i64>,
        editing: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<LoanForm> {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| LoanForm::new(app, snap, client_id, editing, window, cx));
        let title = match editing {
            Some(id) => tr!("Edit loan #{id}", "Editar préstamo #{id}"),
            None => t("New loan", "Nuevo préstamo").to_string(),
        };
        let first = Cell::new(true);
        let dialog_form = form.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = dialog_form.read(cx).first_focus(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title.clone()).w(px(1120.)).child(dialog_form.clone()).on_ok(confirm(&dialog_form))
        });
        form
    }

    pub fn open_payment_form(&mut self, loan_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        self.payment_dialog(loan_id, None, window, cx)
    }

    pub fn edit_payment(&mut self, payment_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.payment_dialog(None, Some(payment_id), window, cx)
    }

    fn payment_dialog(&mut self, loan_id: Option<i64>, editing: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| PaymentForm::new(app, snap, loan_id, editing, window, cx));
        let title = match editing {
            Some(_) => t("Edit payment", "Editar pago"),
            None => t("Record payment", "Registrar pago"),
        };
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).first_focus(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(600.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_capital_form(&mut self, kind: CapitalKind, window: &mut Window, cx: &mut Context<Self>) {
        self.capital_dialog(kind, None, window, cx)
    }

    pub fn edit_capital(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.capital_dialog(CapitalKind::Contribution, Some(id), window, cx)
    }

    fn capital_dialog(&mut self, kind: CapitalKind, editing: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| CapitalForm::new(app, snap, kind, editing, window, cx));
        let title = match (editing, kind) {
            (Some(_), _) => t("Edit capital entry", "Editar movimiento de capital"),
            (None, CapitalKind::Contribution) => t("Add capital", "Agregar capital"),
            (None, CapitalKind::Withdrawal) => t("Withdraw capital", "Retirar capital"),
        };
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).amount.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(520.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    /// A new expense or transfer, or (with `editing`) any cash entry.
    pub fn open_entry_form(&mut self, kind: EntryKind, editing: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let kind = editing.and_then(|id| self.snap.entry(id)).map_or(kind, |e| e.kind);
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| EntryForm::new(app, snap, kind, editing, window, cx));
        let title = EntryForm::title(kind, editing.is_some());
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).amount.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(560.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_adjust_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| AdjustForm::new(app, snap, window, cx));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).capital.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(t("Adjust balances", "Ajustar saldos")).w(px(600.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_reason_form(&mut self, kind: ReasonKind, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let form = cx.new(|cx| ReasonForm::new(app, kind, window, cx));
        let title = kind.title();
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).reason.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(480.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn confirm_archive(&mut self, client_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.snap.client(client_id).cloned() else { return };
        let archiving = !client.is_archived();
        let app = cx.entity().downgrade();
        let open_loans =
            self.snap.loans_for_client(client_id).filter(|l| l.status == crate::domain::ledger::ContractStatus::Active).count();
        let name = client.full_name.clone();
        let loans = crate::i18n::count(open_loans, "active loan", "active loans", "préstamo activo", "préstamos activos");
        let description = if archiving {
            if open_loans > 0 {
                tr!(
                    "{name} has {loans}. Archiving hides the client from lists and new loans; their loans and payment history are kept and still count in your portfolio.",
                    "{name} tiene {loans}. Archivar oculta al cliente de las listas y de nuevos préstamos; sus préstamos e historial se conservan y siguen contando en tu cartera."
                )
            } else {
                tr!(
                    "{name} will be hidden from lists and new loans. Their history is kept and can be restored at any time.",
                    "{name} quedará oculto de las listas y de nuevos préstamos. Su historial se conserva y puedes restaurarlo cuando quieras."
                )
            }
        } else {
            tr!(
                "{name} will appear in lists again and can receive new loans.",
                "{name} volverá a aparecer en las listas y podrá recibir préstamos."
            )
        };
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            alert
                .title(if archiving { t("Archive client?", "¿Archivar cliente?") } else { t("Restore client?", "¿Restaurar cliente?") })
                .description(description.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(if archiving { t("Archive", "Archivar") } else { t("Restore", "Restaurar") })
                        .ok_variant(ButtonVariant::Primary)
                        .cancel_text(t("Cancel", "Cancelar"))
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let _ = app.update(cx, |app, cx| {
                        match app.save(Change::ArchiveClient { id: client_id, archived: archiving }, window, cx) {
                            Ok(_) => {
                                app.reload(cx);
                                app.notify_success(
                                    if archiving {
                                        t("Client archived", "Cliente archivado")
                                    } else {
                                        t("Client restored", "Cliente restaurado")
                                    },
                                    window,
                                    cx,
                                );
                            }
                            Err(e) => {
                                let m = app.error_message(&e);
                                app.notify_error(m, window, cx);
                            }
                        }
                    });
                    true
                })
        });
    }

    pub fn reinstate_loan(&mut self, loan_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        match self.save(Change::Reinstate { id: loan_id }, window, cx) {
            Ok(_) => {
                self.reload(cx);
                self.notify_success(t("Loan reinstated.", "Préstamo reactivado."), window, cx);
            }
            Err(e) => {
                let m = self.error_message(&e);
                self.notify_error(m, window, cx);
            }
        }
    }

    // ----- files ----------------------------------------------------------

    /// Picks a picture to use as the business logo. If the Open window can't
    /// open, says so; a picture can also be dragged onto the logo box.
    pub fn choose_logo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = crate::file_picker::Request::picture(t("Choose a logo", "Elige un logo"), t("Use as logo", "Usar como logo"));
        self.pick_path(
            "logo",
            request,
            window,
            cx,
            |app, path, window, cx| app.use_logo_file(&path, window, cx),
            |app, why, window, cx| {
                app.notify_error(
                    tr!(
                        "The Open window couldn't open ({why}). Drag the picture onto the logo box in Settings instead.",
                        "La ventana Abrir no se pudo abrir ({why}). Arrastra la imagen al cuadro del logo en Configuración."
                    ),
                    window,
                    cx,
                )
            },
        );
    }

    /// Reads a picture file and saves it as the logo.
    pub fn use_logo_file(&mut self, path: &std::path::Path, window: &mut Window, cx: &mut Context<Self>) {
        let result = std::fs::read(path)
            .map_err(|e| anyhow::Error::from(e).context(tr!("Could not read the file", "No se pudo leer el archivo")))
            .and_then(|bytes| self.save(Change::Logo(Some(bytes)), window, cx));
        match result {
            Ok(_) => {
                log::info!("logo saved");
                self.reload(cx);
                self.notify_success(
                    t("Logo saved. It now appears in the app and on receipts.", "Logo guardado. Ahora aparece en la app y en los recibos."),
                    window,
                    cx,
                );
            }
            Err(e) => {
                log::warn!("logo not saved: {e:#}");
                let m = match e.downcast_ref::<crate::services::Invalid>() {
                    Some(v) => v.0.clone(),
                    None => format!("{e:#}"),
                };
                self.notify_error(m, window, cx);
            }
        }
    }

    pub fn remove_logo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.save(Change::Logo(None), window, cx) {
            Ok(_) => {
                self.reload(cx);
                self.notify_success(t("Logo removed", "Logo eliminado"), window, cx);
            }
            Err(e) => {
                let m = self.error_message(&e);
                self.notify_error(m, window, cx);
            }
        }
    }

    /// Restoring a backup or resetting would replace books the cloud keeps;
    /// on a linked PC they are refused. Returns true when refused.
    fn refuse_when_linked(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.is_linked() {
            return false;
        }
        self.notify_error(
            t(
                "This PC shows a business from the cloud. To restore or reset, first switch to the books on this PC in Settings.",
                "Esta PC muestra un negocio de la nube. Para restaurar o reiniciar, primero cambia a los registros de esta PC en Configuración.",
            ),
            window,
            cx,
        );
        true
    }

    pub fn choose_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_when_linked(window, cx) {
            return;
        }
        let request = crate::file_picker::Request {
            title: t("Choose a backup to restore", "Elige una copia para restaurar").into(),
            ok_label: t("Restore", "Restaurar").into(),
            pick: crate::file_picker::Pick::File { type_name: t("Backups", "Copias de seguridad").into(), patterns: vec!["*.db"] },
            start: vec![self.store.backups_dir()],
        };
        self.pick_path(
            "restore",
            request,
            window,
            cx,
            |app, path, window, cx| app.confirm_restore(path, window, cx),
            |app, why, window, cx| {
                app.notify_error(
                    tr!(
                        "The Open window couldn't open ({why}). Pick one of the backups listed in Settings → Data instead.",
                        "La ventana Abrir no se pudo abrir ({why}). Elige una de las copias listadas en Configuración → Datos."
                    ),
                    window,
                    cx,
                )
            },
        );
    }

    pub(crate) fn confirm_restore(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let info = match Store::inspect_backup(&path) {
            Ok(i) => i,
            Err(e) => {
                self.notify_error(tr!("This file can't be restored: {e:#}", "Este archivo no se puede restaurar: {e:#}"), window, cx);
                return;
            }
        };
        let app = cx.entity().downgrade();
        let (clients, loans, payments) = (info.clients, info.loans, info.payments);
        let dir = self.store.backups_dir().display().to_string();
        let description = tr!(
            "This backup contains {clients} clients, {loans} loans and {payments} payments. Your current data will be replaced; a safety copy is saved first in {dir}.",
            "Esta copia tiene {clients} clientes, {loans} préstamos y {payments} pagos. Tus datos actuales se reemplazarán; antes se guarda una copia de seguridad en {dir}."
        );
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            let path = path.clone();
            alert
                .title(t("Restore this backup?", "¿Restaurar esta copia?"))
                .description(description.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t("Restore", "Restaurar"))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t("Cancel", "Cancelar"))
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let _ = app.update(cx, |app, cx| match app.store.restore_from(&path) {
                        Ok(safety) => {
                            app.reload(cx);
                            crate::ui::theme::apply(app.settings().theme, cx);
                            let saved = safety.display().to_string();
                            app.notify_success(
                                tr!(
                                    "Backup restored. Previous data saved to {saved}",
                                    "Copia restaurada. Los datos anteriores se guardaron en {saved}"
                                ),
                                window,
                                cx,
                            );
                        }
                        Err(e) => app.notify_error(
                            tr!("Restore failed; your data was not changed. {e:#}", "La restauración falló; tus datos no cambiaron. {e:#}"),
                            window,
                            cx,
                        ),
                    });
                    true
                })
        });
    }

    pub fn choose_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let exports = self.store.exports_dir();
        let request = crate::file_picker::Request {
            title: t("Choose a folder for the CSV files", "Elige una carpeta para los archivos CSV").into(),
            ok_label: t("Export here", "Exportar aquí").into(),
            pick: crate::file_picker::Pick::Folder,
            start: [dirs::document_dir(), dirs::home_dir()].into_iter().flatten().collect(),
        };
        self.pick_path(
            "export",
            request,
            window,
            cx,
            |app, dir, window, cx| app.export_to(dir, window, cx),
            move |app, why, window, cx| {
                let shown = exports.display().to_string();
                app.notify_error(
                    tr!(
                        "The folder window couldn't open ({why}). Exporting to {shown} instead.",
                        "La ventana de carpetas no se pudo abrir ({why}). Se exporta a {shown}."
                    ),
                    window,
                    cx,
                );
                app.export_to(exports, window, cx);
            },
        );
    }

    fn export_to(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.export_csv(&dir) {
            Ok(files) => {
                let (n, shown) = (files.len(), dir.display().to_string());
                self.notify_success(tr!("Exported {n} CSV files to {shown}", "Se exportaron {n} archivos CSV a {shown}"), window, cx);
                self.open_path(dir, false, window, cx);
            }
            Err(e) => {
                log::error!("export failed: {e:#}");
                self.notify_error(tr!("Export failed: {e:#}", "La exportación falló: {e:#}"), window, cx)
            }
        }
    }

    /// Edits the wording printed on receipts.
    pub fn open_receipt_text_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let settings = self.settings().clone();
        let form = cx.new(|cx| crate::ui::forms::receipt_text::ReceiptTextForm::new(app, &settings, window, cx));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).heading.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(t("Receipt wording", "Texto de los recibos")).w(px(720.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    /// Asks for a typed confirmation, saves a copy and deletes every client,
    /// loan, payment and money movement.
    pub fn open_reset_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.refuse_when_linked(window, cx) {
            return;
        }
        let app = cx.entity().downgrade();
        let form = cx.new(|cx| crate::ui::forms::reset::ResetForm::new(app, window, cx));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).confirm.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(t("Reset all data", "Borrar todos los datos")).w(px(520.)).child(form.clone()).on_ok(confirm(&form))
        });
    }
}
