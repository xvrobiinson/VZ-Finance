//! Cloud sync on screen: the status chip and the "Cloud sync" card in
//! Settings (sign in with an emailed code, choose the business this PC
//! shows, review changes the cloud refused). The work itself is in
//! `app::cloud`.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use crate::app::{Route, SyncStatus, VzApp};
use crate::i18n::t;
use crate::sync::wire::Role;
use crate::tr;
use crate::ui::kit::{Button, Chip, ListRow};
use crate::ui::theme::{Palette, Typeset as _, space, text};

fn role_label(r: Role) -> &'static str {
    match r {
        Role::Admin => t("Admin", "Administrador"),
        Role::Collector => t("Collector", "Cobrador"),
        Role::Viewer => t("Viewer", "Solo lectura"),
    }
}

/// What a queued change was, for the review list.
fn change_label(mutation: &str) -> &'static str {
    match mutation {
        "clients:create" => t("New client", "Cliente nuevo"),
        "clients:update" => t("Client edit", "Cambio de cliente"),
        "clients:setArchived" => t("Client archived or restored", "Cliente archivado o restaurado"),
        "clients:remove" => t("Client deleted", "Cliente eliminado"),
        "loans:create" => t("New loan", "Préstamo nuevo"),
        "loans:update" => t("Loan edit", "Cambio de préstamo"),
        "loans:remove" => t("Loan deleted", "Préstamo eliminado"),
        "loans:markDefaulted" => t("Loan marked as defaulted", "Préstamo marcado incobrable"),
        "loans:reinstate" => t("Loan reinstated", "Préstamo reactivado"),
        "loans:setLateFeeWaived" => t("Late fee waived or restored", "Cargo por mora perdonado o restaurado"),
        "payments:record" => t("New payment", "Pago nuevo"),
        "payments:update" => t("Payment edit", "Cambio de pago"),
        "payments:remove" => t("Payment deleted", "Pago eliminado"),
        "money:addCapital" => t("Capital added or withdrawn", "Capital agregado o retirado"),
        "money:updateCapital" => t("Capital edit", "Cambio de capital"),
        "money:removeCapital" => t("Capital deleted", "Capital eliminado"),
        "money:addEntry" => t("New expense, income or transfer", "Gasto, ingreso o transferencia nueva"),
        "money:adjustBalances" => t("Balance adjustment", "Ajuste de saldos"),
        "money:updateEntry" => t("Money record edit", "Cambio de movimiento"),
        "money:removeEntry" => t("Money record deleted", "Movimiento eliminado"),
        "settings:update" => t("Settings", "Configuración"),
        "settings:setLogo" => t("Logo", "Logo"),
        _ => t("Change", "Cambio"),
    }
}

impl VzApp {
    /// How the cloud sync stands, as a chip that opens Settings. None when
    /// not signed in.
    pub(crate) fn sync_chip(&self, id: &'static str, cx: &mut Context<Self>) -> Option<Chip> {
        let (icon, label, accent) = self.sync_look()?;
        Some(
            Chip::new(id, label)
                .icon(icon)
                .accent(accent)
                .tooltip(t("Cloud sync: open in Settings", "Sincronización: abrir en Configuración"))
                .on_click(cx.listener(|this, _, w, cx| this.navigate(Route::Settings, w, cx))),
        )
    }

    /// The sync status as icon, words and whether it needs the person.
    /// None when not signed in.
    pub(crate) fn sync_look(&self) -> Option<(IconName, String, bool)> {
        Some(match self.sync_status()? {
            SyncStatus::NotLinked => (IconName::Cloud, t("Choose a business", "Elige un negocio").to_string(), true),
            SyncStatus::Synced => (IconName::CloudCheck, t("Synced", "Sincronizado").to_string(), false),
            SyncStatus::Sending(n) => (IconName::CloudUpload, tr!("Sending {n}", "Enviando {n}"), false),
            SyncStatus::Offline(0) => (IconName::CloudOff, t("Offline", "Sin conexión").to_string(), false),
            SyncStatus::Offline(n) => (IconName::CloudOff, tr!("Offline · {n} waiting", "Sin conexión · {n} en espera"), false),
            SyncStatus::NeedsReview(n) => (IconName::CloudAlert, tr!("{n} to review", "{n} por revisar"), true),
            SyncStatus::UpdateRequired => (IconName::CloudAlert, t("Update needed", "Actualización necesaria").to_string(), true),
        })
    }

    /// The body of the "Cloud sync" card in Settings.
    pub(crate) fn sync_settings(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        let body = v_flex().gap(px(space::L));
        let Some(cloud) = &self.cloud else { return self.sign_in_form(p, cx) };
        let email = cloud.email.clone();
        let signed_in = h_flex()
            .justify_between()
            .gap(px(space::M))
            .child(div().text_color(p.muted).child(tr!("Signed in as {email}", "Sesión iniciada como {email}")))
            .child(
                Button::new("sync-sign-out")
                    .ghost()
                    .label(t("Sign out", "Cerrar sesión"))
                    .on_click(cx.listener(|this, _, w, cx| this.sign_out(w, cx))),
            );

        if let Some(min) = &cloud.update_required {
            return body
                .child(signed_in)
                .child(div().text_color(p.text).child(tr!(
                    "This version of the app can't sync anymore. Install version {min} or newer; nothing is sent until then.",
                    "Esta versión de la app ya no puede sincronizar. Instala la versión {min} o una más nueva; no se envía nada hasta entonces."
                )))
                .into_any_element();
        }

        let Some(linked) = cloud.linked.clone() else {
            let rows: Vec<AnyElement> = match &cloud.businesses {
                None => vec![div().text_color(p.muted).child(t("Getting your businesses…", "Buscando tus negocios…")).into_any_element()],
                Some(list) if list.is_empty() => vec![
                    div()
                        .text_color(p.muted)
                        .child(t(
                            "You don't belong to a business yet. Create one in the phone app, or ask an admin to invite you.",
                            "Aún no perteneces a un negocio. Crea uno en la app del teléfono o pide a un administrador que te invite.",
                        ))
                        .into_any_element(),
                ],
                Some(list) => list
                    .iter()
                    .map(|b| {
                        let id = b.business_id.clone();
                        ListRow::new(SharedString::from(format!("sync-business-{id}")))
                            .title(b.name.clone())
                            .subtitle(role_label(b.role))
                            .trailing(
                                Button::new(SharedString::from(format!("sync-use-{id}")))
                                    .label(t("Use on this PC", "Usar en esta PC"))
                                    .on_click(cx.listener(move |this, _, w, cx| this.link_business(&id, w, cx))),
                            )
                            .into_any_element()
                    })
                    .collect(),
            };
            return body
                .child(signed_in)
                .child(div().text_color(p.text).child(t(
                    "Choose the business to show on this PC. Its books come from the cloud and stay in step with the phone app; the books already on this PC are kept as they are.",
                    "Elige el negocio que se verá en esta PC. Sus registros vienen de la nube y se mantienen al día con la app del teléfono; los registros que ya hay en esta PC se guardan tal cual.",
                )))
                .child(v_flex().mx(px(-space::XL)).children(rows))
                .into_any_element();
        };

        let name =
            cloud.businesses.as_ref().and_then(|l| l.iter().find(|b| b.business_id == linked)).map(|b| b.name.clone()).unwrap_or_default();
        let status = match self.sync_status() {
            Some(SyncStatus::Synced) => t("Everything is synced.", "Todo está sincronizado.").to_string(),
            Some(SyncStatus::Sending(n)) => tr!("Sending {n} changes…", "Enviando {n} cambios…"),
            Some(SyncStatus::Offline(0)) => t("Offline. You can keep working; new clients, payments, expenses and income are sent when the connection is back.", "Sin conexión. Puedes seguir trabajando; los clientes nuevos, pagos, gastos e ingresos se envían cuando vuelva la conexión.").to_string(),
            Some(SyncStatus::Offline(n)) => tr!("Offline. {n} changes wait and are sent when the connection is back.", "Sin conexión. {n} cambios esperan y se envían cuando vuelva la conexión."),
            Some(SyncStatus::NeedsReview(n)) => tr!("{n} changes weren't accepted by the cloud. Review them below.", "La nube no aceptó {n} cambios. Revísalos abajo."),
            _ => String::new(),
        };
        let refused = self.store.outbox_refused().unwrap_or_default();
        let review: Vec<AnyElement> = refused
            .into_iter()
            .map(|r| {
                let why = crate::sync::refusal::describe(&r.error, self.settings());
                let (retry, discard) = (r.op_id.clone(), r.op_id.clone());
                ListRow::new(SharedString::from(format!("sync-refused-{}", r.op_id)))
                    .title(change_label(r.op.mutation))
                    .subtitle(why)
                    .trailing(
                        Button::new(SharedString::from(format!("sync-retry-{}", r.op_id)))
                            .label(t("Send again", "Enviar de nuevo"))
                            .on_click(cx.listener(move |this, _, w, cx| this.retry_refused(&retry, w, cx))),
                    )
                    .trailing(
                        Button::new(SharedString::from(format!("sync-discard-{}", r.op_id)))
                            .danger_ghost()
                            .label(t("Discard", "Descartar"))
                            .on_click(cx.listener(move |this, _, w, cx| this.discard_refused(&discard, w, cx))),
                    )
                    .into_any_element()
            })
            .collect();
        body.child(signed_in)
            .child(
                div()
                    .typeset(text::BODY_STRONG)
                    .text_color(p.text)
                    .child(tr!("This PC shows {name} from the cloud", "Esta PC muestra {name} desde la nube")),
            )
            .child(div().text_color(p.muted).child(status))
            .children((!review.is_empty()).then(|| v_flex().mx(px(-space::XL)).children(review)))
            .child(
                h_flex().child(
                    Button::new("sync-local-books")
                        .label(t("Show the books on this PC", "Ver los registros de esta PC"))
                        .tooltip(t(
                            "The cloud business stays linked; choose it again any time.",
                            "El negocio de la nube sigue vinculado; elígelo de nuevo cuando quieras.",
                        ))
                        .on_click(cx.listener(|this, _, w, cx| this.use_local_books(true, w, cx))),
                ),
            )
            .into_any_element()
    }

    /// Signed out: what signing in does, and the button that opens the
    /// sign-in screen.
    fn sign_in_form(&mut self, p: &Palette, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .gap(px(space::M))
            .child(div().text_color(p.text).child(t(
                "Sign in to see your business from the phone app on this PC and keep both in step. Use an emailed code or your Google account.",
                "Inicia sesión para ver en esta PC tu negocio de la app del teléfono y mantener ambos al día. Usa un código por correo o tu cuenta de Google.",
            )))
            .child(
                h_flex().child(
                    Button::new("sync-open-sign-in")
                        .primary()
                        .icon(IconName::LogIn)
                        .label(t("Sign in", "Iniciar sesión"))
                        .on_click(cx.listener(|this, _, w, cx| this.open_sign_in(w, cx))),
                ),
            )
            .child(div().typeset(text::CAPTION).text_color(p.muted).child(t(
                "Without signing in, the app works only with the books on this PC and makes no network requests.",
                "Sin iniciar sesión, la app trabaja solo con los registros de esta PC y no usa internet.",
            )))
            .into_any_element()
    }
}
