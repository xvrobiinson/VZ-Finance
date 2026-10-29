//! The sync worker: one thread with its own Tokio runtime that keeps a
//! Convex connection open for the signed-in user and reports to the app
//! through a channel the UI awaits.
//!
//! It asks `desktop:compat` first, so an outdated build stops before
//! reading or sending anything. Every Convex call carries a token fetched
//! with the stored session (`auth::token`); the Convex client asks again on
//! each reconnect, and the worker renews it every few minutes so a long
//! connection never runs on an expired one.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use convex::{AuthenticationToken, ConvexClient, ConvexClientBuilder, FunctionResult, Value, WebSocketState};
use futures::StreamExt;
use futures::channel::mpsc as events;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use super::auth::{self, AuthError, Endpoints};
use super::vault::{self, Stored};
use super::wire::{self, Membership, Snapshot};

/// How often the Convex token is renewed while connected (Better Auth's
/// tokens last longer than this).
const TOKEN_RENEWAL: Duration = Duration::from_secs(10 * 60);
/// How long to wait before trying again when the cloud can't be reached at
/// start.
const RETRY: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub struct Config {
    pub endpoints: Endpoints,
    /// This build's version, checked with `desktop:compat`.
    pub client_version: String,
}

/// What the worker reports.
#[derive(Debug)]
pub enum Event {
    /// The connection to the cloud is up (`true`) or being re-established.
    Online(bool),
    /// This build is too old to sync; nothing was read or sent.
    UpdateRequired { min_version: String },
    /// The session ended; it was forgotten on this PC.
    SignedOut,
    /// The businesses the user belongs to, whenever that list changes.
    Businesses(Vec<Membership>),
    /// The open business's books, whenever anything in them changes.
    Snapshot(Box<Snapshot>),
    /// Something went wrong that the worker keeps going after (logged too).
    Problem(String),
}

enum Command {
    Open(String),
    Stop,
}

pub struct Worker {
    commands: mpsc::UnboundedSender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// Starts syncing for a remembered session. Events arrive on the
    /// returned receiver until the worker stops.
    pub fn start(config: Config, stored: Stored) -> std::io::Result<(Self, events::UnboundedReceiver<Event>)> {
        let (commands, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = events::unbounded();
        let thread = std::thread::Builder::new().name("sync".into()).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(r) => r,
                Err(e) => {
                    let _ = event_tx.unbounded_send(Event::Problem(e.to_string()));
                    return;
                }
            };
            runtime.block_on(run(config, stored, command_rx, event_tx));
        })?;
        Ok((Self { commands, thread: Some(thread) }, event_rx))
    }

    /// Follows one business's books (`Event::Snapshot`).
    pub fn open(&self, business_id: &str) {
        let _ = self.commands.send(Command::Open(business_id.to_string()));
    }

    /// Stops the worker and waits for its thread.
    pub fn stop(mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing the app must not wait for the network: the thread ends on
        // its own once it sees this.
        let _ = self.commands.send(Command::Stop);
    }
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect()
}

/// A function result read into `T`.
fn read<T: DeserializeOwned>(result: FunctionResult) -> Result<T, String> {
    match result {
        FunctionResult::Value(v) => serde_json::from_value(wire::normalize(serde_json::Value::from(v))).map_err(|e| e.to_string()),
        FunctionResult::ErrorMessage(m) => Err(m),
        FunctionResult::ConvexError(e) => Err(serde_json::Value::from(e.data).to_string()),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Compat {
    ok: bool,
    min_version: String,
}

/// Fetches Convex tokens with the session, renewing the stored session when
/// the server slides it forward and reporting a session that has ended.
fn token_fetcher(endpoints: Endpoints, stored: Arc<Mutex<Stored>>, out: events::UnboundedSender<Event>) -> convex::AuthTokenFetcher {
    Box::new(move |_force| {
        let (endpoints, stored, out) = (endpoints.clone(), stored.clone(), out.clone());
        Box::pin(async move {
            let session = stored.lock().unwrap().session.clone();
            let ep = endpoints.clone();
            let got = tokio::task::spawn_blocking(move || auth::token(&ep, &session)).await?;
            match got {
                Ok((token, renewed)) => {
                    if let Some(session) = renewed {
                        let mut s = stored.lock().unwrap();
                        s.session = session;
                        if let Err(e) = vault::save(&endpoints.cloud, &s) {
                            log::warn!("renewed session not saved: {e}");
                        }
                    }
                    Ok(AuthenticationToken::User(token))
                }
                Err(AuthError::SignedOut) => {
                    vault::forget(&endpoints.cloud);
                    let _ = out.unbounded_send(Event::SignedOut);
                    Err(anyhow::anyhow!("signed out"))
                }
                Err(e) => Err(anyhow::anyhow!(e.to_string())),
            }
        })
    })
}

async fn connect(config: &Config, out: &events::UnboundedSender<Event>) -> Option<(ConvexClient, mpsc::Receiver<WebSocketState>)> {
    let (state_tx, state_rx) = mpsc::channel(16);
    match ConvexClientBuilder::new(&config.endpoints.cloud).with_on_state_change(state_tx).build().await {
        Ok(client) => Some((client, state_rx)),
        Err(e) => {
            log::info!("sync: can't reach the cloud: {e}");
            let _ = out.unbounded_send(Event::Online(false));
            None
        }
    }
}

async fn run(config: Config, stored: Stored, mut commands: mpsc::UnboundedReceiver<Command>, out: events::UnboundedSender<Event>) {
    let send = |e: Event| {
        let _ = out.unbounded_send(e);
    };
    let (mut client, mut states) = loop {
        if let Some(c) = connect(&config, &out).await {
            break c;
        }
        tokio::select! {
            _ = tokio::time::sleep(RETRY) => {}
            cmd = commands.recv() => if matches!(cmd, None | Some(Command::Stop)) { return },
        }
    };

    let compat = client
        .query("desktop:compat", args(&[("clientVersion", &config.client_version)]))
        .await
        .map_err(|e| e.to_string())
        .and_then(read::<Compat>);
    match compat {
        Ok(c) if c.ok => {}
        Ok(c) => return send(Event::UpdateRequired { min_version: c.min_version }),
        Err(e) => return send(Event::Problem(format!("version check: {e}"))),
    }

    let stored = Arc::new(Mutex::new(stored));
    let fetcher = || token_fetcher(config.endpoints.clone(), stored.clone(), out.clone());
    client.set_auth_callback(Some(fetcher())).await;

    let mut businesses = match client.subscribe("businesses:mine", BTreeMap::new()).await {
        Ok(s) => s,
        Err(e) => return send(Event::Problem(format!("businesses: {e}"))),
    };
    let mut books: Option<convex::QuerySubscription> = None;
    let mut renewal = tokio::time::interval(TOKEN_RENEWAL);
    renewal.tick().await;

    loop {
        tokio::select! {
            Some(s) = states.recv() => send(Event::Online(matches!(s, WebSocketState::Connected))),
            Some(r) = businesses.next() => match read::<Vec<Membership>>(r) {
                Ok(list) => send(Event::Businesses(list)),
                Err(e) => send(Event::Problem(format!("businesses: {e}"))),
            },
            Some(r) = async { match books.as_mut() { Some(b) => b.next().await, None => std::future::pending().await } } => {
                match read::<Snapshot>(r) {
                    Ok(snap) => send(Event::Snapshot(Box::new(snap))),
                    Err(e) => send(Event::Problem(format!("books: {e}"))),
                }
            },
            _ = renewal.tick() => client.set_auth_callback(Some(fetcher())).await,
            cmd = commands.recv() => match cmd {
                Some(Command::Open(id)) => match client.subscribe("data:snapshot", args(&[("businessId", &id)])).await {
                    Ok(s) => books = Some(s),
                    Err(e) => send(Event::Problem(format!("books: {e}"))),
                },
                Some(Command::Stop) | None => return,
            },
        }
    }
}
