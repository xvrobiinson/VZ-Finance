//! Backups to the person's own Google Drive (docs/DESKTOP-SYNC-PLAN.md in
//! the PWA repo, phase 4). The app asks only for `drive.file`: it can see
//! and change the files it made, nothing else in the Drive. Backups go into
//! one folder, "Acervus Finance backups"; the newest `KEEP` are kept.
//!
//! The Google access token comes from the cloud (`auth::google_token`),
//! which keeps the refresh token. Plain blocking HTTPS calls, made off the
//! app's thread.

use std::path::Path;

use serde_json::{Value, json};

use crate::i18n::t;
use crate::sync::auth::{GoogleToken, agent};

/// The one permission asked for: files this app creates.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
/// The folder backups go into.
pub const FOLDER: &str = "Acervus Finance backups";
/// How many backups stay in the folder.
pub const KEEP: usize = 30;

const API: &str = "https://www.googleapis.com/drive/v3";
const UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const FOLDER_TYPE: &str = "application/vnd.google-apps.folder";

/// Why a Drive backup failed, as a sentence for the person.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DriveError {
    #[error(
        "{}",
        t(
            "No connection to Google Drive. The backup will be sent later.",
            "Sin conexión con Google Drive. La copia se enviará más tarde."
        )
    )]
    Offline,
    /// The permission was removed (in the Google account) or never given.
    #[error(
        "{}",
        t(
            "Google Drive isn't connected. Turn Drive backups on again.",
            "Google Drive no está conectado. Activa de nuevo las copias en Drive."
        )
    )]
    NotConnected,
    #[error(
        "{}",
        t("Your Google Drive is full. Free some space for the backups.", "Tu Google Drive está lleno. Libera espacio para las copias.")
    )]
    Full,
    #[error("{}: {reason}", t("Google Drive refused the backup", "Google Drive rechazó la copia"), reason = .0)]
    Refused(String),
}

/// What one upload did.
#[derive(Debug, PartialEq, Eq)]
pub struct Uploaded {
    /// The backups folder's id, to find it faster next time.
    pub folder: String,
    /// Older backups deleted to keep the newest `KEEP`.
    pub pruned: usize,
}

/// Uploads the backup file at `path` as `name`, then deletes the oldest
/// backups beyond `KEEP`. `folder` is the folder id from last time, if any.
pub fn backup(token: &GoogleToken, path: &Path, name: &str, folder: Option<&str>) -> Result<Uploaded, DriveError> {
    if !token.has(SCOPE) {
        return Err(DriveError::NotConnected);
    }
    let bytes = std::fs::read(path).map_err(|e| DriveError::Refused(e.to_string()))?;
    let folder = ensure_folder(token, folder)?;
    upload(token, &folder, name, &bytes)?;
    let pruned = prune(token, &folder, KEEP)?;
    Ok(Uploaded { folder, pruned })
}

struct Reply {
    status: u16,
    body: Value,
    location: Option<String>,
}

fn send(token: &GoogleToken, method: &str, url: &str, body: Option<Body>) -> Result<Reply, DriveError> {
    let agent = agent();
    let auth = token.bearer();
    let result = match (method, body) {
        ("GET", _) => agent.get(url).header("Authorization", &auth).call(),
        ("DELETE", _) => agent.delete(url).header("Authorization", &auth).call(),
        (_, Some(Body::Json(v))) => agent.post(url).header("Authorization", &auth).send_json(v),
        (_, Some(Body::Resumable(v, kind, len))) => agent
            .post(url)
            .header("Authorization", &auth)
            .header("X-Upload-Content-Type", kind)
            .header("X-Upload-Content-Length", len.to_string())
            .send_json(v),
        (_, Some(Body::Bytes(b))) => {
            agent.put(url).header("Authorization", &auth).header("Content-Type", "application/octet-stream").send(&b[..])
        }
        (_, None) => agent.post(url).header("Authorization", &auth).send_empty(),
    };
    let mut res = result.map_err(|e| {
        log::info!("Drive request failed: {e}");
        DriveError::Offline
    })?;
    let status = res.status().as_u16();
    let location = res.headers().get("location").and_then(|h| h.to_str().ok()).map(str::to_string);
    let body = res.body_mut().read_json::<Value>().unwrap_or(Value::Null);
    if status >= 400 {
        return Err(refusal(status, &body));
    }
    Ok(Reply { status, body, location })
}

enum Body {
    Json(Value),
    /// Starts a resumable upload: the file's details, its type and size.
    Resumable(Value, &'static str, usize),
    Bytes(Vec<u8>),
}

fn refusal(status: u16, body: &Value) -> DriveError {
    let reason = body.pointer("/error/errors/0/reason").and_then(|r| r.as_str()).unwrap_or("");
    let message = body.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("");
    log::warn!("Drive answered {status}: {reason}");
    match (status, reason) {
        (401, _) | (403, "insufficientPermissions" | "insufficientFilePermissions" | "accessNotConfigured") => DriveError::NotConnected,
        (403, "storageQuotaExceeded") => DriveError::Full,
        _ => DriveError::Refused(if message.is_empty() { format!("HTTP {status}") } else { message.to_string() }),
    }
}

/// `s` for a Drive search or a URL query.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The backups folder: the one from last time if it's still there, else
/// one found by name among the app's files, else a new one.
fn ensure_folder(token: &GoogleToken, known: Option<&str>) -> Result<String, DriveError> {
    if let Some(id) = known.filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))) {
        match send(token, "GET", &format!("{API}/files/{id}?fields=id,trashed"), None) {
            Ok(r) if r.body.get("trashed") == Some(&Value::Bool(false)) => return Ok(id.to_string()),
            Ok(_) | Err(DriveError::Refused(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let q = format!("name = '{FOLDER}' and mimeType = '{FOLDER_TYPE}' and trashed = false");
    let found = send(token, "GET", &format!("{API}/files?q={}&fields=files(id)&pageSize=1", encode(&q)), None)?;
    if let Some(id) = found.body.pointer("/files/0/id").and_then(|i| i.as_str()) {
        return Ok(id.to_string());
    }
    let made =
        send(token, "POST", &format!("{API}/files?fields=id"), Some(Body::Json(json!({ "name": FOLDER, "mimeType": FOLDER_TYPE }))))?;
    made.body.get("id").and_then(|i| i.as_str()).map(str::to_string).ok_or_else(|| DriveError::Refused("no folder id".into()))
}

/// A resumable upload: the details first, then the bytes to the address
/// Drive gives back.
fn upload(token: &GoogleToken, folder: &str, name: &str, bytes: &[u8]) -> Result<(), DriveError> {
    let details = json!({ "name": name, "parents": [folder], "mimeType": "application/vnd.sqlite3" });
    let started = send(
        token,
        "POST",
        &format!("{UPLOAD}?uploadType=resumable&fields=id"),
        Some(Body::Resumable(details, "application/vnd.sqlite3", bytes.len())),
    )?;
    let to = started
        .location
        .filter(|l| l.starts_with("https://www.googleapis.com/"))
        .ok_or_else(|| DriveError::Refused("no upload address".into()))?;
    let done = send(token, "PUT", &to, Some(Body::Bytes(bytes.to_vec())))?;
    match done.status {
        200 | 201 => Ok(()),
        s => Err(DriveError::Refused(format!("HTTP {s}"))),
    }
}

/// Deletes the oldest files in the folder beyond `keep`.
fn prune(token: &GoogleToken, folder: &str, keep: usize) -> Result<usize, DriveError> {
    let q = format!("'{folder}' in parents and trashed = false");
    let list = send(token, "GET", &format!("{API}/files?q={}&orderBy=createdTime%20desc&fields=files(id)&pageSize=200", encode(&q)), None)?;
    let ids: Vec<String> = list
        .body
        .get("files")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|f| f.get("id").and_then(|i| i.as_str()).map(str::to_string)).collect())
        .unwrap_or_default();
    let mut pruned = 0;
    for id in ids.iter().skip(keep) {
        match send(token, "DELETE", &format!("{API}/files/{}", encode(id)), None) {
            Ok(_) => pruned += 1,
            // One that can't go now is tried again next time.
            Err(e) => log::warn!("an old Drive backup was not deleted: {e}"),
        }
    }
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_are_encoded() {
        assert_eq!(encode("name = 'A B'"), "name%20%3D%20%27A%20B%27");
    }

    #[test]
    fn drive_refusals_become_sentences() {
        let body = |reason: &str| json!({ "error": { "errors": [{ "reason": reason }], "message": "m" } });
        assert_eq!(refusal(403, &body("insufficientPermissions")), DriveError::NotConnected);
        assert_eq!(refusal(403, &body("storageQuotaExceeded")), DriveError::Full);
        assert_eq!(refusal(401, &json!({})), DriveError::NotConnected);
        assert_eq!(refusal(500, &body("backendError")), DriveError::Refused("m".into()));
    }

    #[test]
    fn nothing_is_sent_without_the_drive_permission() {
        let token = GoogleToken::new("x".into(), vec!["openid".into(), "email".into()]);
        let file = std::env::temp_dir().join("acervus-drive-test.db");
        assert_eq!(backup(&token, &file, "b.db", None), Err(DriveError::NotConnected));
    }
}
