//! Signing in to the cloud with the emailed 6-digit code, as the PWA does
//! (Better Auth's email-OTP endpoints on the deployment's `.convex.site`
//! address), and trading the session for the short-lived Convex token that
//! every Convex call carries.
//!
//! These are plain blocking HTTPS calls; the sync worker makes them on its
//! own thread. The session is a cookie value: it is kept in Windows
//! Credential Manager (`vault`), never written to a file or a log.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use ureq::Agent;
use ureq::tls::{TlsConfig, TlsProvider};

use crate::i18n::t;

/// Where a deployment answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    /// Convex functions (`https://<name>.convex.cloud`).
    pub cloud: String,
    /// Sign-in and the ID photo endpoint (`https://<name>.convex.site`).
    pub site: String,
    /// The PWA's address, which the deployment trusts for sign-in requests.
    pub origin: String,
}

impl Endpoints {
    /// The live deployment.
    pub fn production() -> Self {
        Self::new("https://proper-pony-55.convex.cloud", "https://acervus-finance-pwa.vz-labs.workers.dev")
    }

    /// `cloud` with its sign-in address worked out the way the PWA does
    /// (`.convex.cloud` → `.convex.site`; a local backend's port 3210 →
    /// 3211).
    pub fn new(cloud: &str, origin: &str) -> Self {
        let cloud = cloud.trim_end_matches('/').to_string();
        let site = if let Some(name) = cloud.strip_suffix(".convex.cloud") {
            format!("{name}.convex.site")
        } else {
            cloud.replace(":3210", ":3211")
        };
        Self { cloud, site, origin: origin.trim_end_matches('/').to_string() }
    }
}

/// A signed-in session (Better Auth's cookies). Its value is never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Session(String);

impl Session {
    pub fn from_stored(value: String) -> Self {
        Self(value)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session(…)")
    }
}

/// Why signing in or getting a token failed, as a sentence for the person.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error(
        "{}",
        t(
            "No connection to the cloud. Check the internet and try again.",
            "Sin conexión con la nube. Revisa el internet e inténtalo de nuevo."
        )
    )]
    Offline,
    #[error("{}", t("That code is wrong. Check it and try again.", "Ese código no es correcto. Revísalo e inténtalo de nuevo."))]
    WrongCode,
    #[error("{}", t("That code expired. Send a new one.", "Ese código venció. Pide uno nuevo."))]
    CodeExpired,
    #[error("{}", t("Too many wrong tries for this code. Send a new one.", "Demasiados intentos con este código. Pide uno nuevo."))]
    TooManyTries,
    #[error(
        "{}",
        t("Too many attempts. Wait a few minutes and try again.", "Demasiados intentos. Espera unos minutos e inténtalo de nuevo.")
    )]
    RateLimited,
    #[error(
        "{}",
        t(
            "We couldn't email your code. Try again in a few minutes.",
            "No pudimos enviarte el código por correo. Inténtalo de nuevo en unos minutos."
        )
    )]
    EmailNotSent,
    /// The session ended (signed out elsewhere, expired, or revoked).
    #[error("{}", t("You were signed out. Sign in again.", "Se cerró tu sesión. Inicia sesión de nuevo."))]
    SignedOut,
    #[error("{}: {reason}", t("The cloud refused the request", "La nube rechazó la solicitud"), reason = .0)]
    Refused(String),
}

#[derive(Deserialize)]
struct ErrorBody {
    code: Option<String>,
    message: Option<String>,
}

pub(crate) fn agent() -> Agent {
    Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(20)))
        .tls_config(TlsConfig::builder().provider(TlsProvider::NativeTls).build())
        .build()
        .into()
}

/// The cookies in a `set-better-auth-cookie` header, as one `Cookie`-style
/// value (`a=1; b=2`). Attributes (`Path`, `Expires`, which can contain
/// commas) are dropped.
fn cookie_value(header: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut rest = header;
    while !rest.is_empty() {
        // A new cookie starts after a comma followed by `name=`.
        let end = rest
            .match_indices(',')
            .map(|(i, _)| i)
            .find(|&i| {
                let after = rest[i + 1..].trim_start();
                after
                    .split_once('=')
                    .is_some_and(|(name, _)| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
            })
            .unwrap_or(rest.len());
        let cookie = rest[..end].split(';').next().unwrap_or("").trim();
        if !cookie.is_empty() {
            out.push(cookie);
        }
        rest = rest.get(end + 1..).unwrap_or("");
    }
    out.join("; ")
}

struct Reply {
    status: u16,
    body: serde_json::Value,
    session: Option<Session>,
}

fn call(ep: &Endpoints, method: &str, path: &str, session: Option<&Session>, body: Option<serde_json::Value>) -> Result<Reply, AuthError> {
    let url = format!("{}/api/auth{path}", ep.site);
    let agent = agent();
    let cookie = session.map(Session::as_str).unwrap_or("");
    let result = match (method, body) {
        ("GET", _) => agent.get(&url).header("Origin", &ep.origin).header("Better-Auth-Cookie", cookie).call(),
        (_, body) => {
            agent.post(&url).header("Origin", &ep.origin).header("Better-Auth-Cookie", cookie).send_json(body.unwrap_or_else(|| json!({})))
        }
    };
    let mut res = result.map_err(|e| {
        log::info!("sign-in request failed: {e}");
        AuthError::Offline
    })?;
    let session =
        res.headers().get("set-better-auth-cookie").and_then(|h| h.to_str().ok()).map(cookie_value).filter(|c| !c.is_empty()).map(Session);
    let status = res.status().as_u16();
    let body = res.body_mut().read_json::<serde_json::Value>().unwrap_or(serde_json::Value::Null);
    Ok(Reply { status, body, session })
}

fn refusal(reply: &Reply) -> AuthError {
    let e: ErrorBody = serde_json::from_value(reply.body.clone()).unwrap_or(ErrorBody { code: None, message: None });
    match (reply.status, e.code.as_deref()) {
        (_, Some("INVALID_OTP")) => AuthError::WrongCode,
        (_, Some("OTP_EXPIRED")) => AuthError::CodeExpired,
        (_, Some("TOO_MANY_ATTEMPTS")) => AuthError::TooManyTries,
        (_, Some("EMAIL_NOT_SENT")) => AuthError::EmailNotSent,
        (429, _) => AuthError::RateLimited,
        (401, _) => AuthError::SignedOut,
        (status, code) => AuthError::Refused(e.message.or(code.map(str::to_string)).unwrap_or_else(|| format!("HTTP {status}"))),
    }
}

/// Emails a sign-in code to `email`.
pub fn send_code(ep: &Endpoints, email: &str) -> Result<(), AuthError> {
    let reply = call(ep, "POST", "/email-otp/send-verification-otp", None, Some(json!({ "email": email.trim(), "type": "sign-in" })))?;
    if reply.status == 200 { Ok(()) } else { Err(refusal(&reply)) }
}

/// Signs in with the emailed code. A new address gets an account, as in
/// the PWA.
pub fn sign_in(ep: &Endpoints, email: &str, code: &str) -> Result<Session, AuthError> {
    let reply = call(ep, "POST", "/sign-in/email-otp", None, Some(json!({ "email": email.trim(), "otp": code.trim() })))?;
    if reply.status != 200 {
        return Err(refusal(&reply));
    }
    reply.session.ok_or_else(|| AuthError::Refused("no session".into()))
}

/// A fresh Convex token for `session`, and the session itself when the
/// server renewed it (Better Auth slides it forward once a day).
pub fn token(ep: &Endpoints, session: &Session) -> Result<(String, Option<Session>), AuthError> {
    let reply = call(ep, "GET", "/convex/token", Some(session), None)?;
    let token = reply.body.get("token").and_then(|t| t.as_str()).map(str::to_string);
    match (reply.status, token) {
        (200, Some(token)) => Ok((token, reply.session)),
        (200, None) => Err(AuthError::SignedOut),
        _ => Err(refusal(&reply)),
    }
}

/// Ends the session on the server. Failing (offline) is not an error: the
/// session is forgotten on this PC either way.
pub fn sign_out(ep: &Endpoints, session: &Session) {
    if let Err(e) = call(ep, "POST", "/sign-out", Some(session), None) {
        log::info!("sign-out not sent: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_address_follows_the_deployment() {
        let p = Endpoints::new("https://proper-pony-55.convex.cloud/", "https://app.example.com/");
        assert_eq!(p.site, "https://proper-pony-55.convex.site");
        assert_eq!(p.origin, "https://app.example.com");
        assert_eq!(Endpoints::new("http://127.0.0.1:3210", "http://localhost:5173").site, "http://127.0.0.1:3211");
    }

    #[test]
    fn cookies_keep_name_and_value_only() {
        let h = "better-auth.session_token=abc.def; Path=/; Expires=Wed, 12 Nov 2026 10:00:00 GMT; HttpOnly, \
                 better-auth.session_data=xyz; Path=/";
        assert_eq!(cookie_value(h), "better-auth.session_token=abc.def; better-auth.session_data=xyz");
        assert_eq!(cookie_value(""), "");
    }

    #[test]
    fn session_is_never_printed() {
        assert_eq!(format!("{:?}", Session("secret".into())), "Session(…)");
    }
}
