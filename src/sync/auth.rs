//! Signing in to the cloud with the emailed 6-digit code, as the PWA does
//! (Better Auth's email-OTP endpoints on the deployment's `.convex.site`
//! address), and trading the session for the short-lived Convex token that
//! every Convex call carries.
//!
//! These are plain blocking HTTPS calls; the sync worker makes them on its
//! own thread. The session is a cookie value: it is kept in Windows
//! Credential Manager (`vault`), never written to a file or a log.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;
use ureq::Agent;
use ureq::tls::{TlsConfig, TlsProvider};

use crate::i18n::t;
use crate::sync::vault;

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

    /// This session with the cookies of a renewal: the server may send only
    /// the ones that changed, so the others are kept. A cookie the server
    /// clears (empty value) is dropped.
    pub fn merged(&self, renewal: &Session) -> Session {
        let pairs = |s: &str| -> Vec<(String, String)> {
            s.split("; ").filter_map(|c| c.split_once('=')).map(|(n, v)| (n.to_string(), v.to_string())).collect()
        };
        let mut out = pairs(&self.0);
        for (name, value) in pairs(&renewal.0) {
            out.retain(|(n, _)| *n != name);
            if !value.is_empty() {
                out.push((name, value));
            }
        }
        Session(out.into_iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; "))
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
    /// Google sign-in was stopped in the browser, or took too long.
    #[error("{}", t("Google sign-in didn't finish. Try again.", "El inicio de sesión con Google no terminó. Inténtalo de nuevo."))]
    GoogleNotFinished,
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

// ----- Google ------------------------------------------------------------
//
// The person signs in with Google in their own browser; the app never sees
// their Google password. As RFC 8252 describes for desktop apps, the app
// waits on a port of its own on 127.0.0.1. Google returns to the sign-in
// server, which sends the browser to the PWA's `desktop-sign-in.html` with a
// one-time token, and that page passes it to the waiting app with the check
// value (`state`) the app made, which the app then trades for a session.

/// The PWA page that hands the browser back to the app.
const HANDOFF: &str = "/desktop-sign-in.html";
/// How long the app waits for the browser.
const GOOGLE_WAIT: Duration = Duration::from_secs(10 * 60);

/// A Google sign-in waiting for the browser.
pub struct GoogleSignIn {
    listener: TcpListener,
    state: String,
    /// Where to send the browser (Google's sign-in page).
    pub url: String,
}

/// Starts a Google sign-in: opens the local port and asks the server for
/// Google's address.
pub fn start_google(ep: &Endpoints) -> Result<GoogleSignIn, AuthError> {
    let local = |e: std::io::Error| AuthError::Refused(e.to_string());
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(local)?;
    let port = listener.local_addr().map_err(local)?.port();
    let state = vault::random_token(40).map_err(AuthError::Refused)?;
    let back = format!("{}{HANDOFF}?port={port}&state={state}", ep.origin);
    let body = json!({ "provider": "google", "callbackURL": back, "errorCallbackURL": back, "disableRedirect": true });
    let reply = call(ep, "POST", "/sign-in/social", None, Some(body))?;
    let url = reply.body.get("url").and_then(|u| u.as_str()).filter(|u| u.starts_with("https://")).map(str::to_string);
    match (reply.status, url) {
        (200, Some(url)) => Ok(GoogleSignIn { listener, state, url }),
        (200, None) => Err(AuthError::Refused(t("Google sign-in isn't turned on", "El inicio con Google no está activado").into())),
        _ => Err(refusal(&reply)),
    }
}

/// Waits for the browser to come back, then signs in. Returns the person's
/// email and the session. `stop` ends the wait early (the person cancelled).
pub fn finish_google(ep: &Endpoints, g: GoogleSignIn, stop: &AtomicBool) -> Result<(String, Session), AuthError> {
    g.listener.set_nonblocking(true).map_err(|e| AuthError::Refused(e.to_string()))?;
    let deadline = Instant::now() + GOOGLE_WAIT;
    loop {
        if stop.load(Ordering::Relaxed) || Instant::now() > deadline {
            return Err(AuthError::GoogleNotFinished);
        }
        let mut stream = match g.listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
            Err(e) => return Err(AuthError::Refused(e.to_string())),
        };
        let Some(back) = read_request(&mut stream).and_then(|target| Return::parse(&target, &g.state)) else {
            // Something else knocking (a favicon, a stray request): not ours.
            answer(&mut stream, "404 Not Found", "");
            continue;
        };
        let result = match back {
            Return::Token(ott) => verify_token(ep, &ott),
            Return::Failed => Err(AuthError::GoogleNotFinished),
        };
        let page = match &result {
            Ok(_) => page(
                t("You're signed in", "Sesión iniciada"),
                t("Go back to Acervus Finance. You can close this tab.", "Vuelve a Acervus Finance. Puedes cerrar esta pestaña."),
            ),
            Err(e) => page(t("Sign-in didn't finish", "No se completó el inicio de sesión"), &e.to_string()),
        };
        answer(&mut stream, "200 OK", &page);
        return result;
    }
}

/// What the browser brought back.
#[derive(Debug, PartialEq, Eq)]
enum Return {
    Token(String),
    Failed,
}

impl Return {
    /// `target` (`/done?state=…&ott=…`) if it carries this sign-in's
    /// `state`; anything else is ignored.
    fn parse(target: &str, state: &str) -> Option<Return> {
        let query = target.strip_prefix("/done?")?;
        let get = |name: &str| query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v);
        if get("state")? != state {
            return None;
        }
        Some(match get("ott") {
            Some(ott) if !ott.is_empty() && ott.chars().all(|c| c.is_ascii_alphanumeric() || "_-".contains(c)) => Return::Token(ott.into()),
            _ => Return::Failed,
        })
    }
}

/// The request target of one HTTP request (`GET <target> HTTP/1.1`).
fn read_request(stream: &mut TcpStream) -> Option<String> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.lines().next()?.split(' ');
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(str::to_string)
}

fn answer(stream: &mut TcpStream, status: &str, html: &str) {
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

/// The small page the browser shows when it comes back.
fn page(title: &str, text: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>Acervus Finance</title>\
         <body style=\"margin:0;min-height:100vh;display:grid;place-items:center;font:15px/22px system-ui,sans-serif;\
         background:linear-gradient(#9fc9f1,#f1f6fc);color:#17191c\"><main style=\"max-width:340px;margin:16px;padding:32px 28px;\
         border-radius:22px;background:#fffd;text-align:center\"><h1 style=\"margin:0 0 6px;font-size:20px\">{}</h1>\
         <p style=\"margin:0;color:#666c77\">{}</p></main>",
        esc(title),
        esc(text)
    )
}

/// Trades the one-time token for a session.
fn verify_token(ep: &Endpoints, ott: &str) -> Result<(String, Session), AuthError> {
    let reply = call(ep, "POST", "/cross-domain/one-time-token/verify", None, Some(json!({ "token": ott })))?;
    if reply.status != 200 {
        return Err(refusal(&reply));
    }
    let email = reply.body.pointer("/user/email").and_then(|e| e.as_str()).map(str::to_string);
    match (email, reply.session) {
        (Some(email), Some(session)) => Ok((email, session)),
        _ => Err(AuthError::Refused("no session".into())),
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
    fn a_renewal_keeps_the_other_cookies() {
        let s = Session("better-auth.session_token=abc; better-auth.convex_jwt=old".into());
        let r = s.merged(&Session("better-auth.convex_jwt=new".into()));
        assert_eq!(r.as_str(), "better-auth.session_token=abc; better-auth.convex_jwt=new");
        let cleared = r.merged(&Session("better-auth.convex_jwt=".into()));
        assert_eq!(cleared.as_str(), "better-auth.session_token=abc");
    }

    #[test]
    fn only_this_sign_ins_return_is_taken() {
        let state = "S".repeat(40);
        let ok = format!("/done?state={state}&ott=abcDEF123_-");
        assert_eq!(Return::parse(&ok, &state), Some(Return::Token("abcDEF123_-".into())));
        assert_eq!(Return::parse(&format!("/done?state={state}&error=access_denied"), &state), Some(Return::Failed));
        assert_eq!(Return::parse(&format!("/done?state={state}&ott=<script>"), &state), Some(Return::Failed));
        assert_eq!(Return::parse("/done?state=other&ott=abc", &state), None);
        assert_eq!(Return::parse("/favicon.ico", &state), None);
    }

    #[test]
    fn the_browser_comes_back_to_the_waiting_app() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = "s".repeat(40);
        let g = GoogleSignIn { listener, state: state.clone(), url: String::new() };
        let browser = std::thread::spawn(move || {
            let get = |path: &str| {
                let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(s, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
                let mut reply = String::new();
                let _ = s.read_to_string(&mut reply);
                reply
            };
            let stray = get("/favicon.ico");
            let back = get(&format!("/done?state={state}&error=access_denied"));
            (stray, back)
        });
        let ep = Endpoints::new("http://127.0.0.1:9", "http://localhost:5173");
        assert_eq!(finish_google(&ep, g, &AtomicBool::new(false)).unwrap_err(), AuthError::GoogleNotFinished);
        let (stray, back) = browser.join().unwrap();
        assert!(stray.starts_with("HTTP/1.1 404"));
        assert!(back.starts_with("HTTP/1.1 200") && back.contains("<h1"));
    }

    #[test]
    fn session_is_never_printed() {
        assert_eq!(format!("{:?}", Session("secret".into())), "Session(…)");
    }
}
