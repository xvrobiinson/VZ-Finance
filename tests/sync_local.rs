//! Sync against a local copy of the PWA's backend (never a cloud
//! deployment). Ignored by default; to run it, in the PWA repo start
//! `CONVEX_AGENT_MODE=anonymous npx convex dev > <log>` with `SITE_URL` set
//! to http://localhost:5173 and no email provider (codes go to the log),
//! then here:
//!
//!   VZ_SYNC_LOCAL_LOG=<log> cargo test --test sync_local -- --ignored

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use convex::{ConvexClient, FunctionResult, Value};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use vz_finance::services::Store;
use vz_finance::sync::auth::{self, AuthError, Endpoints, Session};
use vz_finance::sync::figures::figures;
use vz_finance::sync::vault::{self, Stored};
use vz_finance::sync::wire::Role;
use vz_finance::sync::worker::{Config, Event, Worker};

const CLOUD: &str = "http://127.0.0.1:3210";

fn endpoints() -> Endpoints {
    Endpoints::new(CLOUD, "http://localhost:5173")
}

/// Signs `email` in with the code the local backend wrote to its log.
fn sign_in(email: &str) -> Session {
    let log = std::env::var("VZ_SYNC_LOCAL_LOG").expect("set VZ_SYNC_LOCAL_LOG to the `convex dev` log");
    auth::send_code(&endpoints(), email).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let code = loop {
        let text = std::fs::read_to_string(&log).unwrap();
        let found = text.lines().rev().filter(|l| l.contains(email)).find_map(|l| {
            let i = l.find("CODE: ")? + 6;
            l.get(i..i + 6).map(str::to_string)
        });
        if let Some(code) = found {
            break code;
        }
        assert!(Instant::now() < deadline, "no code in the log");
        std::thread::sleep(Duration::from_millis(200));
    };
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert_eq!(auth::sign_in(&endpoints(), email, wrong).err(), Some(AuthError::WrongCode));
    auth::sign_in(&endpoints(), email, &code).unwrap()
}

fn create_business(session: &Session, name: &str) -> String {
    let (token, _) = auth::token(&endpoints(), session).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut c = ConvexClient::new(CLOUD).await.unwrap();
        c.set_auth(Some(token)).await;
        let args: BTreeMap<String, Value> =
            [("name", name), ("timezone", "America/Santo_Domingo"), ("language", "es"), ("currencyCode", "DOP")]
                .into_iter()
                .map(|(k, v)| (k.to_string(), Value::String(v.into())))
                .collect();
        match c.mutation("businesses:create", args).await.unwrap() {
            FunctionResult::Value(Value::String(id)) => id,
            other => panic!("create failed: {other:?}"),
        }
    })
}

/// The next event that `want` accepts, skipping others.
fn wait_for<T>(rx: &mut UnboundedReceiver<Event>, what: &str, mut want: impl FnMut(Event) -> Option<T>) -> T {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let ev = rx.next().await.unwrap_or_else(|| panic!("worker stopped before {what}"));
                if let Some(t) = want(ev) {
                    return t;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
    })
}

#[test]
#[ignore = "needs a local PWA backend; see the top of this file"]
fn signs_in_follows_the_books_and_signs_out() {
    let email = format!("desktop-{}@example.com", std::process::id());
    let session = sign_in(&email);
    let business_id = create_business(&session, "Préstamos Escritorio");

    // An outdated build is stopped before it reads anything.
    let stored = Stored { email: email.clone(), session: session.clone() };
    let old = Config { endpoints: endpoints(), client_version: "0.6.0".into() };
    let (worker, mut rx) = Worker::start(old, stored.clone()).unwrap();
    let min = wait_for(&mut rx, "the version check", |e| match e {
        Event::UpdateRequired { min_version } => Some(min_version),
        Event::Businesses(_) | Event::Snapshot(_) => panic!("an outdated build read data"),
        _ => None,
    });
    assert_eq!(min, "0.7.0");
    worker.stop();

    let config = Config { endpoints: endpoints(), client_version: "0.7.0".into() };
    let (worker, mut rx) = Worker::start(config, stored.clone()).unwrap();
    let list = wait_for(&mut rx, "the business list", |e| match e {
        Event::Businesses(list) => Some(list),
        Event::Problem(p) => panic!("{p}"),
        _ => None,
    });
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].business_id.as_str(), list[0].role), (business_id.as_str(), Role::Admin));

    worker.open(&business_id);
    let snap = wait_for(&mut rx, "the books", |e| match e {
        Event::Snapshot(s) => Some(s),
        Event::Problem(p) => panic!("{p}"),
        _ => None,
    });
    assert_eq!(snap.business.name, "Préstamos Escritorio");
    assert_eq!(snap.me.role, Role::Admin);
    assert!(snap.loans.is_empty());
    assert_eq!(snap.settings.as_ref().map(|s| s.currency_code.as_str()), Some("DOP"));

    // A change made elsewhere (here, a second business) arrives by itself.
    create_business(&session, "Segundo Negocio");
    let list = wait_for(&mut rx, "the updated list", |e| match e {
        Event::Businesses(list) if list.len() == 2 => Some(list),
        _ => None,
    });
    assert!(list.iter().any(|b| b.name == "Segundo Negocio"));
    worker.stop();

    // The session is kept by Windows, not in a file.
    vault::save(CLOUD, &stored).unwrap();
    assert_eq!(vault::load(CLOUD).map(|s| s.email), Some(email));
    auth::sign_out(&endpoints(), &session);
    assert_eq!(auth::token(&endpoints(), &session).err(), Some(AuthError::SignedOut));
    vault::forget(CLOUD);
    assert!(vault::load(CLOUD).is_none());
}

/// The desktop's copy of an imported book shows the same figures as the
/// desktop that exported it. Import first, in the PWA repo:
///
///   cargo run --example seed_demo -- <demo.db>
///   cargo run --example export_for_cloud -- <demo.db> <export-dir>
///   node scripts/import-desktop.mjs <export-dir> --url http://127.0.0.1:3210
///       --origin http://localhost:5173 --email <email> --code-from-log <log>
///
/// then run this with VZ_SYNC_EXPORT=<export-dir> and VZ_SYNC_EMAIL=<email>.
#[test]
#[ignore = "needs a local PWA backend with an imported book; see above"]
fn an_imported_book_arrives_with_the_same_figures() {
    let dir = std::path::PathBuf::from(std::env::var("VZ_SYNC_EXPORT").expect("set VZ_SYNC_EXPORT"));
    let email = std::env::var("VZ_SYNC_EMAIL").expect("set VZ_SYNC_EMAIL");
    let expected: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("figures.json")).unwrap()).unwrap();

    let stored = Stored { email: email.clone(), session: sign_in(&email) };
    let config = Config { endpoints: endpoints(), client_version: "0.7.0".into() };
    let (worker, mut rx) = Worker::start(config, stored).unwrap();
    let list = wait_for(&mut rx, "the business list", |e| match e {
        Event::Businesses(list) if !list.is_empty() => Some(list),
        Event::Problem(p) => panic!("{p}"),
        _ => None,
    });
    worker.open(&list[0].business_id);
    let snap = wait_for(&mut rx, "the books", |e| match e {
        Event::Snapshot(s) => Some(s),
        Event::Problem(p) => panic!("{p}"),
        _ => None,
    });
    worker.stop();
    assert_eq!(snap.today, expected["asOf"], "import and check on the same day");

    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().join("data").join("linked.db")).unwrap();
    store.apply_cloud(&snap).unwrap();
    let today = chrono::NaiveDate::parse_from_str(&snap.today, "%Y-%m-%d").unwrap();
    assert_eq!(store.refresh(today).unwrap(), 0, "the cloud's late fees are the desktop's");
    let got = figures(&store.snapshot().unwrap().book, today);
    assert_eq!(got["summary"], expected["summary"]);
    assert_eq!(got["loans"], expected["loans"]);
}
