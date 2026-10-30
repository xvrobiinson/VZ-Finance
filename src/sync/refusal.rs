//! Why the cloud refused a change, as a sentence. The cloud answers with a
//! code and figures (`convex/lib/errors.ts` in the PWA repo); the wording
//! follows the PWA's `src/lib/errors.ts` so both apps say the same thing.

use serde_json::Value;

use crate::i18n::t;
use crate::services::settings::Settings;
use crate::tr;

pub fn describe(e: &Value, s: &Settings) -> String {
    let money = |k: &str| s.money().format(e[k].as_i64().unwrap_or(0));
    let day = |k: &str| {
        e[k].as_str()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .map(|d| s.date_style.format(d))
            .unwrap_or_default()
    };
    let text = match e["code"].as_str().unwrap_or_default() {
        "unauthenticated" => t("Your session ended. Sign in again.", "Tu sesión terminó. Inicia sesión de nuevo."),
        "forbidden" => forbidden(),
        "not_found" => t("That record no longer exists.", "Ese registro ya no existe."),
        "invalid" => invalid(e),
        "conflict" => t(
            "Someone changed this record while you were editing it. Review the current values and try again.",
            "Alguien cambió este registro mientras lo editabas. Revisa los valores actuales e inténtalo de nuevo.",
        ),
        "stale" => t(
            "The loan changed after this payment was entered (another payment may have been recorded). Review the loan, then send it again or discard it.",
            "El préstamo cambió después de anotar este pago (quizá se registró otro pago). Revisa el préstamo y luego envíalo de nuevo o descártalo.",
        ),
        "insufficient_cash" => {
            let available = money("available");
            return tr!("Only {available} is available on that date.", "Solo hay {available} disponible en esa fecha.");
        }
        "cash_shortfall" => {
            let when = day("date");
            return tr!(
                "This would leave an account below zero on {when}, because the money was already lent, spent or withdrawn.",
                "Esto dejaría una cuenta por debajo de cero el {when}, porque el dinero ya se prestó, gastó o retiró."
            );
        }
        "payment" | "plan" => {
            let (amount, when) = (money("amount"), day("paidOn"));
            return tr!(
                "The payment of {amount} on {when} no longer fits the loan. Review the loan's payments.",
                "El pago de {amount} del {when} ya no cuadra con el préstamo. Revisa los pagos del préstamo."
            );
        }
        "schedule" => t(
            "The installments don't add up to the loan. Review the schedule.",
            "Las cuotas no suman el préstamo. Revisa el plan de pagos.",
        ),
        "loan_closed" => t("This loan is closed", "Este préstamo está cerrado"),
        "archived_client" => t("This client is archived. Restore the client first.", "Este cliente está archivado. Restáuralo primero."),
        "client_has_loans" => t(
            "This client has loans. Delete the loans, or move them to another client, first.",
            "Este cliente tiene préstamos. Primero elimínalos o pásalos a otro cliente.",
        ),
        "loan_not_active" => {
            t("Only active loans can be marked as defaulted", "Solo los préstamos activos pueden marcarse como incobrables")
        }
        "loan_not_defaulted" => t("Only defaulted loans can be reinstated", "Solo los préstamos incobrables pueden reactivarse"),
        "no_late_fee" => t("There is no late fee on that installment", "Esa cuota no tiene cargo por mora"),
        "nothing_to_adjust" => t("Nothing to adjust: the amounts already match.", "No hay nada que ajustar: los montos ya coinciden."),
        "rate_limited" => t("Too many attempts. Wait a while and try again.", "Demasiados intentos. Espera un rato e inténtalo de nuevo."),
        "unknown" => t(
            "The connection dropped before the cloud answered, so it's not known whether this was saved. Check the books, then send it again or discard it.",
            "La conexión se cortó antes de que la nube respondiera, así que no se sabe si se guardó. Revisa los registros y luego envíalo de nuevo o descártalo.",
        ),
        "server" => {
            let m = e["message"].as_str().unwrap_or_default();
            return tr!("The cloud couldn't save this: {m}", "La nube no pudo guardar esto: {m}");
        }
        _ => t("Something went wrong. Try again.", "Algo salió mal. Inténtalo de nuevo."),
    };
    text.to_string()
}

/// A change the person's role doesn't allow.
pub fn forbidden() -> &'static str {
    t("Your role doesn't allow this. Ask an admin.", "Tu rol no permite esto. Pídeselo a un administrador.")
}

fn invalid(e: &Value) -> &'static str {
    match e["reason"].as_str().unwrap_or_default() {
        "required" => t("Fill in the required fields", "Completa los campos obligatorios"),
        "too_long" => t("That text is too long", "El texto es muy largo"),
        "one_line" => t("Write it on one line", "Escríbelo en una sola línea"),
        "future" => t("The date can't be in the future", "La fecha no puede ser futura"),
        "date" => t("Enter a valid date", "Escribe una fecha válida"),
        "email" => t("Enter a valid email address, or leave it empty", "Escribe un correo válido o déjalo vacío"),
        "phone" => t("Phone numbers may contain digits, spaces and + - ( )", "El teléfono solo puede tener números, espacios y + - ( )"),
        "positive" => t("Enter an amount greater than zero", "Escribe un monto mayor que cero"),
        "fee_too_large" => t("The fee is larger than the payment", "El cargo es mayor que el pago"),
        "automatic_late_fees" => {
            t("Late fees on installment loans are charged automatically", "En préstamos por cuotas los cargos por mora se calculan solos")
        }
        "before_start" => t("The date is before the loan started", "La fecha es anterior al inicio del préstamo"),
        "before_last_payment" => t("The date is before the latest payment", "La fecha es anterior al último pago"),
        "photo" => t("Use a JPEG, PNG or WebP picture under 4 MB", "Usa una foto JPEG, PNG o WebP de menos de 4 MB"),
        "confirm_name" => t("Type the business name exactly as shown", "Escribe el nombre del negocio tal como aparece"),
        "logo" => t("Use a PNG, JPEG or WebP picture under 1 MB", "Usa una imagen PNG, JPEG o WebP de menos de 1 MB"),
        _ => t("Check the values and try again", "Revisa los valores e inténtalo de nuevo"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codes_become_sentences() {
        let s = Settings::default();
        let conflict = describe(&json!({ "code": "conflict" }), &s);
        assert_ne!(conflict, describe(&json!({ "code": "invalid", "reason": "future" }), &s));
        assert!(describe(&json!({ "code": "insufficient_cash", "available": 150000 }), &s).contains("1,500.00"));
        assert!(describe(&json!({ "code": "server", "message": "boom" }), &s).ends_with("boom"));
        assert_eq!(describe(&json!({ "code": "something_new" }), &s), describe(&json!({}), &s));
    }
}
