//! The change history in words: what was edited or deleted, field by field.

use chrono::{DateTime, Local, Utc};

use crate::database::repositories::changes::{Action, Change, Entity};
use crate::domain::loans::{Frequency, Method};
use crate::domain::money::Rate;
use crate::domain::portfolio::{Account, CapitalKind, EntryKind};
use crate::i18n::t;
use crate::services::Snapshot;
use crate::tr;
use crate::utils::dates::parse_iso;

pub struct Described {
    pub title: String,
    /// "Field: old → new", or "Field: value" for a deletion.
    pub lines: Vec<String>,
    pub reason: String,
    /// When the change was made, in local time.
    pub when: String,
}

fn field_label(field: &str) -> &'static str {
    match field {
        "client" => t("Client", "Cliente"),
        "principal" => t("Amount lent", "Monto prestado"),
        "rate" => t("Rate", "Tasa"),
        "frequency" => t("Payments", "Pagos"),
        "start_date" => t("Loan date", "Fecha del préstamo"),
        "amount" => t("Amount", "Monto"),
        "fee" => t("Fee", "Cargo"),
        "interest" => t("Interest", "Interés"),
        "paid_on" | "occurred_on" => t("Date", "Fecha"),
        "kind" => t("Type", "Tipo"),
        "account" => t("Account", "Cuenta"),
        "description" => t("Description", "Descripción"),
        "full_name" => t("Name", "Nombre"),
        "phone" => t("Phone", "Teléfono"),
        "email" => t("Email", "Correo"),
        "payments" => t("Payments recorded", "Pagos registrados"),
        "received" => t("Received", "Recibido"),
        "interest_period" => t("Interest period", "Período del interés"),
        "method" => t("Loan type", "Tipo de préstamo"),
        "installments" => t("Installments", "Cuotas"),
        "late_fee_rate" => t("Late fee", "Cargo por mora"),
        "schedule" => t("Schedule", "Calendario"),
        "schedule_edited" => t("Schedule edited by hand", "Calendario editado a mano"),
        "allocation" => t("Applied", "Aplicado"),
        "fee_base" => t("Unpaid when assessed", "Pendiente al calcularlo"),
        "waived" => t("Waived", "Perdonado"),
        _ => t("Notes", "Notas"),
    }
}

fn field_value(field: &str, raw: &str, snap: &Snapshot) -> String {
    if raw.is_empty() {
        return "—".into();
    }
    let money = snap.settings.money();
    match field {
        "principal" | "amount" | "fee" | "interest" | "received" | "fee_base" => {
            raw.parse().map(|c| money.format(c)).unwrap_or_else(|_| raw.into())
        }
        "rate" | "late_fee_rate" => raw.parse().map(|ppm| Rate(ppm).percent_label()).unwrap_or_else(|_| raw.into()),
        "frequency" | "interest_period" => Frequency::from_key(raw).map(|f| f.label().to_string()).unwrap_or_else(|| raw.into()),
        "method" => Method::from_key(raw).map(|m| m.short_label().to_string()).unwrap_or_else(|| raw.into()),
        "schedule" => match raw.split_once('|') {
            Some((n, total)) => {
                let total = total.parse().map(|c| money.format(c)).unwrap_or_default();
                crate::tr!("{n} installments · {total}", "{n} cuotas · {total}")
            }
            None => raw.into(),
        },
        "schedule_edited" | "waived" => if raw == "true" { t("Yes", "Sí") } else { t("No", "No") }.into(),
        "allocation" => match raw.split_once(' ') {
            Some(("installment", seq)) => crate::tr!("to installment {seq}", "a la cuota {seq}"),
            Some(("spread", _)) => t("spread over installments", "repartido entre cuotas").into(),
            Some(("custom", _)) => t("custom split", "reparto personalizado").into(),
            _ => t("oldest first", "lo más antiguo primero").into(),
        },
        "start_date" | "paid_on" | "occurred_on" => {
            parse_iso(raw).map(|d| snap.settings.date_style.format(d)).unwrap_or_else(|| raw.into())
        }
        "client" => {
            raw.parse::<i64>().ok().and_then(|id| snap.client(id).map(|c| c.full_name.clone())).unwrap_or_else(|| format!("#{raw}"))
        }
        "kind" => {
            CapitalKind::from_key(raw).map(|k| k.label()).or_else(|| EntryKind::from_key(raw).map(|k| k.label())).unwrap_or(raw).to_string()
        }
        "account" => Account::from_key(raw).map(|a| a.label().to_string()).unwrap_or_else(|| raw.into()),
        _ => raw.into(),
    }
}

/// The kind of a cash entry: from the entry if it still exists, else from
/// what was recorded when it was deleted.
fn entry_kind(c: &Change, snap: &Snapshot) -> Option<EntryKind> {
    snap.entry(c.entity_id)
        .map(|e| e.kind)
        .or_else(|| c.fields.iter().find(|f| f.field == "kind").and_then(|f| EntryKind::from_key(&f.old)))
}

pub fn describe(c: &Change, snap: &Snapshot) -> Described {
    let edited = c.action == Action::Update;
    let id = c.entity_id;
    let title = match c.entity {
        Entity::Loan if edited => tr!("Loan #{id} edited", "Préstamo #{id} editado"),
        Entity::Loan => tr!("Loan #{id} deleted", "Préstamo #{id} eliminado"),
        Entity::Payment => {
            let loan = c.loan_id.unwrap_or_default();
            if edited {
                tr!("Payment #{id} on loan #{loan} edited", "Pago #{id} del préstamo #{loan} editado")
            } else {
                tr!("Payment #{id} on loan #{loan} deleted", "Pago #{id} del préstamo #{loan} eliminado")
            }
        }
        Entity::LateFee => {
            let loan = c.loan_id.unwrap_or_default();
            if edited {
                tr!("Late fee on installment {id} of loan #{loan} changed", "Cargo por mora de la cuota {id} del préstamo #{loan} cambiado")
            } else {
                tr!(
                    "Late fee on installment {id} of loan #{loan} removed",
                    "Cargo por mora de la cuota {id} del préstamo #{loan} eliminado"
                )
            }
        }
        Entity::Capital if edited => t("Capital entry edited", "Movimiento de capital editado").into(),
        Entity::Capital => t("Capital entry deleted", "Movimiento de capital eliminado").into(),
        Entity::Entry => {
            let what = entry_kind(c, snap).map_or(t("Movement", "Movimiento"), |k| k.label());
            if edited { tr!("{what} edited", "{what} editado") } else { tr!("{what} deleted", "{what} eliminado") }
        }
        Entity::Client => {
            let name = snap
                .client(id)
                .map(|x| x.full_name.clone())
                .or_else(|| c.fields.iter().find(|f| f.field == "full_name").map(|f| f.old.clone()))
                .unwrap_or_default();
            if edited {
                tr!("Client {name} edited", "Cliente {name} editado")
            } else {
                tr!("Client {name} deleted", "Cliente {name} eliminado")
            }
        }
    };
    let lines = c
        .fields
        .iter()
        .filter(|f| edited || !f.old.is_empty())
        .map(|f| {
            let label = field_label(&f.field);
            let old = field_value(&f.field, &f.old, snap);
            if edited {
                let new = field_value(&f.field, &f.new, snap);
                format!("{label}: {old} → {new}")
            } else {
                format!("{label}: {old}")
            }
        })
        .collect();
    let when = DateTime::parse_from_rfc3339(&c.created_at)
        .map(|d| {
            let local = d.with_timezone(&Utc).with_timezone(&Local);
            format!("{} {}", snap.settings.date_style.format(local.date_naive()), local.format("%H:%M"))
        })
        .unwrap_or_else(|_| c.created_at.clone());
    Described { title, lines, reason: c.reason.clone(), when }
}
