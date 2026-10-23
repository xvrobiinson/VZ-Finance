use std::cell::Cell;
use std::path::PathBuf;

use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::{WindowExt as _, button::ButtonVariant};
use gpui_kit::*;

use super::VzApp;
use crate::domain::portfolio::CapitalKind;
use crate::i18n::t;
use crate::tr;
use crate::services::Store;
use crate::ui::forms::capital::CapitalForm;
use crate::ui::forms::client::ClientForm;
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

submit_impl!(ClientForm, LoanForm, PaymentForm, CapitalForm, ReasonForm);

impl VzApp {
    pub fn open_client_form(&mut self, client_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let existing = client_id.and_then(|id| self.snap.client(id).cloned());
        let app = cx.entity().downgrade();
        let form = cx.new(|cx| ClientForm::new(app, existing.clone(), window, cx));
        let title = if existing.is_some() { t("Edit client", "Editar cliente") } else { t("New client", "Nuevo cliente") };
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).name.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(560.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_loan_form(&mut self, client_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| LoanForm::new(app, snap, client_id, window, cx));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).first_focus(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(t("New loan", "Nuevo préstamo")).w(px(920.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_payment_form(&mut self, loan_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| PaymentForm::new(app, snap, loan_id, window, cx));
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).first_focus(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(t("Record payment", "Registrar pago")).w(px(600.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_capital_form(&mut self, kind: CapitalKind, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let snap = self.snap.clone();
        let form = cx.new(|cx| CapitalForm::new(app, snap, kind, window, cx));
        let title = match kind {
            CapitalKind::Contribution => t("Add capital", "Agregar capital"),
            CapitalKind::Withdrawal => t("Withdraw capital", "Retirar capital"),
        };
        let first = Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            let handle = form.read(cx).amount.focus_handle(cx);
            focus_once(&first, &handle, window, cx);
            dialog.title(title).w(px(520.)).child(form.clone()).on_ok(confirm(&form))
        });
    }

    pub fn open_reason_form(&mut self, kind: ReasonKind, window: &mut Window, cx: &mut Context<Self>) {
        let app = cx.entity().downgrade();
        let form = cx.new(|cx| ReasonForm::new(app, kind, window, cx));
        let title = match kind {
            ReasonKind::ReversePayment(_) => t("Reverse payment", "Anular pago"),
            ReasonKind::MarkDefaulted(_) => t("Mark loan as defaulted", "Marcar préstamo como incobrable"),
        };
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
        let open_loans = self
            .snap
            .loans_for_client(client_id)
            .filter(|l| l.status == crate::domain::ledger::ContractStatus::Active)
            .count();
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
            tr!("{name} will appear in lists again and can receive new loans.", "{name} volverá a aparecer en las listas y podrá recibir préstamos.")
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
                    let _ = app.update(cx, |app, cx| match app.store.set_client_archived(client_id, archiving) {
                        Ok(()) => {
                            app.reload(cx);
                            app.notify_success(if archiving { t("Client archived", "Cliente archivado") } else { t("Client restored", "Cliente restaurado") }, window, cx);
                        }
                        Err(e) => {
                            let m = app.error_message(&e);
                            app.notify_error(m, window, cx);
                        }
                    });
                    true
                })
        });
    }

    pub fn reinstate_loan(&mut self, loan_id: i64, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.reinstate(loan_id, self.today) {
            Ok(()) => {
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

    pub fn choose_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("Restore", "Restaurar").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(path) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |app, window, cx| app.confirm_restore(path, window, cx));
        })
        .detach();
    }

    fn confirm_restore(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
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
                            app.notify_success(tr!("Backup restored. Previous data saved to {saved}", "Copia restaurada. Los datos anteriores se guardaron en {saved}"), window, cx);
                        }
                        Err(e) => app.notify_error(tr!("Restore failed; your data was not changed. {e:#}", "La restauración falló; tus datos no cambiaron. {e:#}"), window, cx),
                    });
                    true
                })
        });
    }

    pub fn choose_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t("Export here", "Exportar aquí").into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |app, window, cx| match app.store.export_csv(&dir) {
                Ok(files) => {
                    let (n, shown) = (files.len(), dir.display().to_string());
                    app.notify_success(tr!("Exported {n} CSV files to {shown}", "Se exportaron {n} archivos CSV a {shown}"), window, cx);
                    cx.reveal_path(&dir);
                }
                Err(e) => app.notify_error(tr!("Export failed: {e:#}", "La exportación falló: {e:#}"), window, cx),
            });
        })
        .detach();
    }
}
