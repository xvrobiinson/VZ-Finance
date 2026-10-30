//! Cloud sync with the Acervus Finance PWA's Convex backend
//! (docs/DESKTOP-SYNC-PLAN.md in the PWA repo).
//!
//! - `auth`: signing in with the emailed code, and Convex tokens.
//! - `vault`: the session in Windows Credential Manager; DPAPI for ID photos.
//! - `wire`: what the cloud sends, as plain structs.
//! - `replica`: a linked database as a copy of the cloud's books.
//! - `change`, `outbox`: changes saved on this PC, queued for the cloud.
//! - `figures`: the engine's figures in the shape the cloud compares.
//! - `worker`: the background connection that reports to the app.
//! - `drive`: backups to the person's Google Drive.
//!
//! Without a signed-in session none of this runs and the app makes no
//! network requests, as before.

pub mod auth;
pub mod change;
pub mod drive;
pub mod figures;
pub mod outbox;
pub mod refusal;
pub mod replica;
pub mod vault;
pub mod wire;
pub mod worker;

/// This build's version, which the cloud checks before syncing.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
