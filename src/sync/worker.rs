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
use futures::channel::oneshot;
use serde::de::DeserializeOwned;
use tokio::sync::mpsc;

use super::auth::{self, AuthError, Endpoints};
use super::outbox::{Outcome, Queued};
use super::replica::Download;
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
    /// A photo or logo asked for with `Worker::download`.
    Downloaded { file: Download, bytes: Vec<u8> },
}

enum Command {
    Open(String),
    Send { mutation: &'static str, args: serde_json::Value, reply: oneshot::Sender<Outcome> },
    Call { mutation: &'static str, args: serde_json::Value, reply: oneshot::Sender<Outcome> },
    Download(Vec<Download>),
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

    /// Sends a queued change to the open business. The answer arrives on
    /// the returned channel: `Outcome::NotSent` when offline or no business is
    /// open (a closed channel means the same: the worker stopped first).
    pub fn send(&self, q: &Queued) -> oneshot::Receiver<Outcome> {
        let (reply, rx) = oneshot::channel();
        let _ = self.commands.send(Command::Send { mutation: q.op.mutation, args: q.op.args.clone(), reply });
        rx
    }

    /// Calls a mutation that isn't a change to the open business's books
    /// (creating or deleting a business), with `args` as given.
    pub fn call(&self, mutation: &'static str, args: serde_json::Value) -> oneshot::Receiver<Outcome> {
        let (reply, rx) = oneshot::channel();
        let _ = self.commands.send(Command::Call { mutation, args, reply });
        rx
    }

    /// Fetches photos and the logo for the open business
    /// (`Store::cloud_downloads`); each arrives as `Event::Downloaded`.
    pub fn download(&self, files: Vec<Download>) {
        if !files.is_empty() {
            let _ = self.commands.send(Command::Download(files));
        }
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

/// JSON as a Convex value. Every number is sent as a float, which is what
/// the cloud's `v.number()` arguments take.
fn to_convex(v: serde_json::Value) -> Value {
    use serde_json::Value as J;
    match v {
        J::Null => Value::Null,
        J::Bool(b) => Value::Boolean(b),
        J::Number(n) => Value::Float64(n.as_f64().unwrap_or(0.0)),
        J::String(s) => Value::String(s),
        J::Array(a) => Value::Array(a.into_iter().map(to_convex).collect()),
        J::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, to_convex(v))).collect()),
    }
}

/// How long to wait for the cloud's answer to a change.
const ANSWER: Duration = Duration::from_secs(25);

/// Sends one change. A photo or logo goes to the cloud's file storage first
/// (`upload` in the arguments); the mutation then names the stored file.
/// Until the mutation itself is sent nothing has changed in the cloud, so a
/// failed upload leaves the change waiting.
async fn send_change(mut client: ConvexClient, mutation: &str, mut args: serde_json::Value) -> Outcome {
    if let Some(file) = args.as_object_mut().and_then(|a| a.remove("upload")) {
        match upload(&mut client, mutation, &args["businessId"], &file).await {
            Ok(storage_id) => args["storageId"] = storage_id.into(),
            Err(o) => return o,
        }
    }
    // A client's photo change goes right after the client is saved.
    let photo = args.as_object_mut().and_then(|a| a.remove("photo"));
    let remove_photo = args.as_object_mut().and_then(|a| a.remove("removePhoto")).is_some();
    let business = args["businessId"].clone();
    let outcome = call(&mut client, mutation, args.clone()).await;
    let Outcome::Accepted(mut result) = outcome else { return outcome };
    if photo.is_some() || remove_photo {
        let client_id = args.get("clientId").or(result.get("clientId")).cloned().unwrap_or_default();
        let done = match photo {
            Some(file) => match upload(&mut client, "clients:setPhoto", &business, &file).await {
                Ok(storage_id) => {
                    let a = serde_json::json!({ "businessId": business, "clientId": client_id, "storageId": storage_id });
                    call(&mut client, "clients:setPhoto", a).await
                }
                Err(o) => o,
            },
            None => call(&mut client, "clients:removePhoto", serde_json::json!({ "businessId": business, "clientId": client_id })).await,
        };
        if !matches!(done, Outcome::Accepted(_)) {
            log::info!("sync: the photo change after {mutation} wasn't saved: {done:?}");
            // The client is saved; the app says the photo needs doing again.
            result["photoNotSaved"] = true.into();
        }
    }
    Outcome::Accepted(result)
}

async fn upload(
    client: &mut ConvexClient,
    mutation: &str,
    business: &serde_json::Value,
    file: &serde_json::Value,
) -> Result<String, Outcome> {
    use base64::Engine as _;
    let url_from = match mutation {
        "clients:setPhoto" => "clients:photoUploadUrl",
        "settings:setLogo" => "settings:logoUploadUrl",
        _ => return Err(Outcome::Refused(serde_json::json!({ "code": "invalid" }))),
    };
    let url = match call(client, url_from, serde_json::json!({ "businessId": business })).await {
        Outcome::Accepted(serde_json::Value::String(url)) => url,
        Outcome::Refused(e) => return Err(Outcome::Refused(e)),
        _ => return Err(Outcome::NotSent),
    };
    let content_type = file["contentType"].as_str().unwrap_or("application/octet-stream").to_string();
    let bytes = base64::engine::general_purpose::STANDARD.decode(file["data"].as_str().unwrap_or_default()).unwrap_or_default();
    let stored = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut res = auth::agent().post(&url).header("Content-Type", &content_type).send(&bytes[..]).map_err(|e| e.to_string())?;
        if res.status() != 200 {
            return Err(format!("upload: HTTP {}", res.status()));
        }
        let body: serde_json::Value = res.body_mut().read_json().map_err(|e| e.to_string())?;
        body["storageId"].as_str().map(str::to_string).ok_or_else(|| "upload: no storageId".into())
    })
    .await;
    match stored {
        Ok(Ok(id)) => Ok(id),
        Ok(Err(e)) => {
            log::info!("sync: {e}");
            Err(Outcome::NotSent)
        }
        Err(_) => Err(Outcome::NotSent),
    }
}

/// Fetches a photo (with the user's token, admins only) or the logo.
fn fetch(endpoints: &Endpoints, business: &str, token: Option<&str>, file: &Download) -> Result<Vec<u8>, String> {
    let agent = auth::agent();
    let req = match file {
        Download::Photo { client, .. } => agent
            .get(format!("{}/client-photo", endpoints.site))
            .query("businessId", business)
            .query("clientId", client)
            .header("Authorization", format!("Bearer {}", token.unwrap_or_default())),
        Download::Logo { url } => agent.get(url),
    };
    let mut res = req.call().map_err(|e| e.to_string())?;
    if res.status() != 200 {
        return Err(format!("HTTP {}", res.status()));
    }
    res.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())
}

async fn call(client: &mut ConvexClient, mutation: &str, args: serde_json::Value) -> Outcome {
    let serde_json::Value::Object(args) = args else { return Outcome::Refused(serde_json::json!({ "code": "invalid" })) };
    let args = args.into_iter().map(|(k, v)| (k, to_convex(v))).collect();
    match tokio::time::timeout(ANSWER, client.mutation(mutation, args)).await {
        Ok(Ok(FunctionResult::Value(v))) => Outcome::Accepted(wire::normalize(serde_json::Value::from(v))),
        Ok(Ok(FunctionResult::ConvexError(e))) => Outcome::Refused(wire::normalize(serde_json::Value::from(e.data))),
        Ok(Ok(FunctionResult::ErrorMessage(m))) => Outcome::Refused(serde_json::json!({ "code": "server", "message": m })),
        Ok(Err(e)) => {
            log::info!("sync: {mutation} got no answer: {e}");
            Outcome::NoAnswer
        }
        Err(_) => Outcome::NoAnswer,
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
/// The Convex token last fetched, for the photo endpoint.
type Token = Arc<Mutex<Option<String>>>;

fn token_fetcher(
    endpoints: Endpoints,
    stored: Arc<Mutex<Stored>>,
    latest: Token,
    out: events::UnboundedSender<Event>,
) -> convex::AuthTokenFetcher {
    Box::new(move |_force| {
        let (endpoints, stored, latest, out) = (endpoints.clone(), stored.clone(), latest.clone(), out.clone());
        Box::pin(async move {
            let session = stored.lock().unwrap().session.clone();
            let ep = endpoints.clone();
            let got = tokio::task::spawn_blocking(move || auth::token(&ep, &session)).await?;
            match got {
                Ok((token, renewed)) => {
                    if let Some(session) = renewed {
                        let mut s = stored.lock().unwrap();
                        s.session = s.session.merged(&session);
                        if let Err(e) = vault::save(&endpoints.cloud, &s) {
                            log::warn!("renewed session not saved: {e}");
                        }
                    }
                    *latest.lock().unwrap() = Some(token.clone());
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
    // The business to follow can be chosen before the cloud is reached.
    let mut wanted: Option<String> = None;
    let (mut client, mut states) = loop {
        if let Some(c) = connect(&config, &out).await {
            break c;
        }
        tokio::select! {
            _ = tokio::time::sleep(RETRY) => {}
            cmd = commands.recv() => match cmd {
                None | Some(Command::Stop) => return,
                Some(Command::Open(id)) => wanted = Some(id),
                // A change can't be sent yet; dropping the reply says so.
                Some(_) => {}
            },
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
    let latest: Token = Arc::default();
    let fetcher = || token_fetcher(config.endpoints.clone(), stored.clone(), latest.clone(), out.clone());
    client.set_auth_callback(Some(fetcher())).await;

    let mut businesses = match client.subscribe("businesses:mine", BTreeMap::new()).await {
        Ok(s) => s,
        Err(e) => return send(Event::Problem(format!("businesses: {e}"))),
    };
    let mut books: Option<convex::QuerySubscription> = None;
    let mut open: Option<String> = None;
    if let Some(id) = wanted {
        match client.subscribe("data:snapshot", args(&[("businessId", &id)])).await {
            Ok(s) => {
                books = Some(s);
                open = Some(id);
            }
            Err(e) => send(Event::Problem(format!("books: {e}"))),
        }
    }
    let mut online = true;
    send(Event::Online(true));
    let mut renewal = tokio::time::interval(TOKEN_RENEWAL);
    renewal.tick().await;

    loop {
        tokio::select! {
            Some(s) = states.recv() => {
                online = matches!(s, WebSocketState::Connected);
                send(Event::Online(online));
            },
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
                    Ok(s) => {
                        books = Some(s);
                        open = Some(id);
                    }
                    Err(e) => send(Event::Problem(format!("books: {e}"))),
                },
                Some(Command::Send { mutation, mut args, reply }) => match (&open, online) {
                    (Some(id), true) => {
                        args["businessId"] = id.clone().into();
                        let client = client.clone();
                        tokio::spawn(async move { let _ = reply.send(send_change(client, mutation, args).await); });
                    }
                    _ => { let _ = reply.send(Outcome::NotSent); }
                },
                Some(Command::Call { mutation, args, reply }) => if online {
                    let client = client.clone();
                    tokio::spawn(async move { let _ = reply.send(send_change(client, mutation, args).await); });
                } else {
                    let _ = reply.send(Outcome::NotSent);
                },
                Some(Command::Download(files)) => if let Some(business) = open.clone() {
                    for file in files {
                        let (endpoints, business, out) = (config.endpoints.clone(), business.clone(), out.clone());
                        let token = latest.lock().unwrap().clone();
                        tokio::task::spawn_blocking(move || {
                            let event = match fetch(&endpoints, &business, token.as_deref(), &file) {
                                Ok(bytes) => Event::Downloaded { file, bytes },
                                Err(e) => Event::Problem(format!("download: {e}")),
                            };
                            let _ = out.unbounded_send(event);
                        });
                    }
                },
                Some(Command::Stop) | None => return,
            },
        }
    }
}
