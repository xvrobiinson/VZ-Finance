//! Sync against a local copy of the PWA's backend (never a cloud
//! deployment). Ignored by default; to run it, in the PWA repo start
//! `CONVEX_AGENT_MODE=anonymous npx convex dev > <log>` with `SITE_URL` set
//! to http://localhost:5173 and no email provider (codes go to the log),
//! then here:
//!
//!   VZ_SYNC_LOCAL_LOG=<log> cargo test --test sync_local -- --ignored

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use convex::{ConvexClient, FunctionResult, Value};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use vz_finance::database::repositories::clients::ClientInput;
use vz_finance::domain::installments::Instruction;
use vz_finance::domain::loans::{Frequency, LoanTerms, Method};
use vz_finance::domain::money::Rate;
use vz_finance::domain::portfolio::{Account, CapitalKind, EntryKind};
use vz_finance::services::Store;
use vz_finance::services::picture::{Photo, PhotoChange};
use vz_finance::sync::auth::{self, AuthError, Endpoints, Session};
use vz_finance::sync::change::Change;
use vz_finance::sync::figures::figures;
use vz_finance::sync::outbox::Outcome;
use vz_finance::sync::vault::{self, Stored};
use vz_finance::sync::wire::Role;
use vz_finance::sync::wire::Snapshot;
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

/// Waits for a copy of the books that `ready` accepts and applies it.
fn apply_when(rx: &mut UnboundedReceiver<Event>, store: &mut Store, what: &str, ready: impl Fn(&Snapshot) -> bool) {
    let snap = wait_for(rx, what, |e| match e {
        Event::Snapshot(s) if ready(&s) => Some(s),
        Event::Problem(p) => panic!("{p}"),
        _ => None,
    });
    store.apply_cloud(&snap).unwrap();
}

/// Saves `change` here, sends it and returns the cloud's answer.
fn save_and_send(store: &mut Store, worker: &Worker, change: Change, today: NaiveDate) -> Outcome {
    let (_, q) = store.save(&change, today).unwrap();
    let q = q.expect("a linked database queues the change");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let outcome = rt.block_on(worker.send(&q)).unwrap_or(Outcome::NotSent);
    store.outbox_finish(&q, &outcome).unwrap();
    outcome
}

/// Each kind of change made on the desktop reaches the cloud, and the
/// cloud's books then match what the desktop worked out on its own.
#[test]
#[ignore = "needs a local PWA backend; see the top of this file"]
fn changes_made_here_reach_the_cloud() {
    let email = format!("desktop-saves-{}@example.com", std::process::id());
    let session = sign_in(&email);
    let business_id = create_business(&session, "Préstamos Sync");
    let stored = Stored { email, session };
    let (worker, mut rx) = Worker::start(Config { endpoints: endpoints(), client_version: "0.7.0".into() }, stored).unwrap();
    worker.open(&business_id);
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().join("data").join("linked.db")).unwrap();
    apply_when(&mut rx, &mut store, "the empty books", |_| true);
    let today = store.cloud_link().unwrap().unwrap().today;

    let check = |store: &mut Store, rx: &mut UnboundedReceiver<Event>, change: Change, what: &str, ready: &dyn Fn(&Snapshot) -> bool| {
        let outcome = save_and_send(store, &worker, change, today);
        assert!(matches!(outcome, Outcome::Accepted(_)), "{what}: {outcome:?}");
        let here = numbers(&store.snapshot().unwrap().book, today);
        apply_when(rx, store, what, ready);
        assert!(store.outbox_waiting().unwrap().is_empty());
        assert_eq!(numbers(&store.snapshot().unwrap().book, today), here, "{what}: the cloud's books differ from the desktop's");
    };

    let one_million = Change::NewCapital { kind: CapitalKind::Contribution, amount: 100_000_000, on: today, notes: "Inicio".into() };
    check(&mut store, &mut rx, one_million, "capital", &|s| s.capital.len() == 1);
    let ana = ClientInput { full_name: "Ana Gómez".into(), phone: "809-555-0101".into(), ..Default::default() };
    check(&mut store, &mut rx, Change::NewClient { input: ana, photo: PhotoChange::Keep }, "client", &|s| s.clients.len() == 1);
    let client = store.snapshot().unwrap().clients[0].id;
    let terms = LoanTerms {
        principal: 10_000_000,
        rate: Rate(100_000),
        interest_period: Frequency::Monthly,
        frequency: Frequency::Monthly,
        start_date: today,
        method: Method::Declining,
        installments: Some(4),
        late_fee_rate: Rate(50_000),
    };
    let loan = Change::NewLoan { client_id: client, terms: terms.clone(), schedule: None, notes: String::new() };
    check(&mut store, &mut rx, loan, "loan", &|s| s.loans.len() == 1);
    let loan = store.snapshot().unwrap().loans[0].id;
    let pay = Change::NewPayment {
        loan_id: loan,
        amount: 1_500_000,
        fee: 0,
        on: today,
        notes: String::new(),
        instruction: Some(Instruction::Auto),
    };
    check(&mut store, &mut rx, pay, "payment", &|s| s.payments.len() == 1);
    let gas = Change::NewEntry {
        kind: EntryKind::Expense,
        account: Account::Interest,
        amount: 50_000,
        on: today,
        description: "Gasolina".into(),
    };
    check(&mut store, &mut rx, gas, "expense", &|s| s.entries.len() == 1);
    let payment = store.snapshot().unwrap().payments[0].id;
    let edit = Change::EditPayment {
        id: payment,
        amount: 2_000_000,
        fee: 0,
        on: today,
        notes: "corregido".into(),
        instruction: Some(Instruction::Auto),
        reason: "Monto equivocado".into(),
    };
    check(&mut store, &mut rx, edit, "payment edit", &|s| s.payments.first().is_some_and(|p| p.amount == 2_000_000));

    let mut settings = store.snapshot().unwrap().settings;
    settings.company_phone = "809-555-0000".into();
    check(&mut store, &mut rx, Change::Settings(Box::new(settings)), "settings", &|s| {
        s.settings.as_ref().is_some_and(|x| x.company_phone == "809-555-0000")
    });
    let capital = store.snapshot().unwrap().capital[0].id;
    let more = Change::EditCapital {
        id: capital,
        kind: CapitalKind::Contribution,
        amount: 120_000_000,
        on: today,
        notes: "Inicio".into(),
        reason: "Faltaba".into(),
    };
    check(&mut store, &mut rx, more, "capital edit", &|s| s.capital[0].amount == 120_000_000);
    let entry = store.snapshot().unwrap().entries[0].id;
    let gas = Change::EditEntry {
        id: entry,
        account: Account::Interest,
        amount: 60_000,
        on: today,
        description: "Gasolina".into(),
        reason: "Recibo".into(),
    };
    check(&mut store, &mut rx, gas, "entry edit", &|s| s.entries[0].amount == 60_000);
    let cash = store.snapshot().unwrap().book.summary(today);
    let count = Change::AdjustBalances {
        capital: cash.available_capital - 10_000,
        interest: cash.available_interest,
        on: today,
        reason: "Conteo de caja".into(),
    };
    check(&mut store, &mut rx, count, "adjustment", &|s| s.entries.len() == 2);
    let loan_notes =
        Change::EditLoan { id: loan, client_id: client, terms, schedule: None, notes: "Fiador: Pedro".into(), reason: "Nota".into() };
    check(&mut store, &mut rx, loan_notes, "loan edit", &|s| s.loans[0].notes == "Fiador: Pedro");
    let default = Change::MarkDefaulted { id: loan, on: today, note: "No contesta".into() };
    check(&mut store, &mut rx, default, "default", &|s| s.loans[0].status == "defaulted");
    check(&mut store, &mut rx, Change::Reinstate { id: loan }, "reinstate", &|s| s.loans[0].status == "active");

    // Sent twice (an answer lost on the way): recorded once.
    let tip =
        Change::NewEntry { kind: EntryKind::Income, account: Account::Interest, amount: 25_000, on: today, description: "Propina".into() };
    let (_, q) = store.save(&tip, today).unwrap();
    let q = q.unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let first = rt.block_on(worker.send(&q)).unwrap();
    let again = rt.block_on(worker.send(&q)).unwrap();
    match (&first, &again) {
        (Outcome::Accepted(a), Outcome::Accepted(b)) => {
            assert_eq!(a["entryId"], b["entryId"]);
            assert_eq!(b["duplicate"], true);
        }
        other => panic!("{other:?}"),
    }
    store.outbox_finish(&q, &again).unwrap();
    apply_when(&mut rx, &mut store, "the income", |s| s.entries.len() == 3);

    for (change, what) in [
        (Change::DeleteEntry { id: store.snapshot().unwrap().entries[0].id, reason: "Duplicado".into() }, "entry delete"),
        (Change::DeletePayment { id: payment, reason: "Error".into() }, "payment delete"),
        (Change::DeleteLoan { id: loan, reason: "Error".into() }, "loan delete"),
    ] {
        let before = store.snapshot().unwrap();
        let (entries, payments, loans) = (before.entries.len(), before.payments.len(), before.loans.len());
        check(&mut store, &mut rx, change, what, &|s| s.entries.len() + s.payments.len() + s.loans.len() == entries + payments + loans - 1);
    }
    check(&mut store, &mut rx, Change::ArchiveClient { id: client, archived: true }, "archive", &|s| s.clients[0].archived_at.is_some());
    check(&mut store, &mut rx, Change::ArchiveClient { id: client, archived: false }, "restore", &|s| s.clients[0].archived_at.is_none());
    let small = Change::NewCapital { kind: CapitalKind::Contribution, amount: 500_000, on: today, notes: String::new() };
    check(&mut store, &mut rx, small, "second capital", &|s| s.capital.len() == 2);
    let small = store.snapshot().unwrap().capital.iter().map(|c| c.id).max().unwrap();
    check(&mut store, &mut rx, Change::DeleteCapital { id: small, reason: "Prueba".into() }, "capital delete", &|s| s.capital.len() == 1);

    // Two edits from the same copy: the second started from a version the
    // first replaced, so the cloud refuses it and the books keep the first.
    let renamed = |name: &str| Change::EditClient {
        id: client,
        input: ClientInput { full_name: name.into(), ..Default::default() },
        photo: PhotoChange::Keep,
    };
    assert!(matches!(save_and_send(&mut store, &worker, renamed("Ana G."), today), Outcome::Accepted(_)));
    match save_and_send(&mut store, &worker, renamed("Ana María"), today) {
        Outcome::Refused(e) => assert_eq!(e["code"], "conflict"),
        other => panic!("a stale edit was {other:?}"),
    }
    apply_when(&mut rx, &mut store, "the first edit", |s| s.clients.first().is_some_and(|c| c.full_name == "Ana G."));
    assert_eq!(store.snapshot().unwrap().clients[0].full_name, "Ana G.");
    assert_eq!(store.outbox_refused().unwrap().len(), 1);
    let gone = Change::DeleteClient { id: client, reason: "Ya no es cliente".into() };
    assert!(matches!(save_and_send(&mut store, &worker, gone, today), Outcome::Accepted(_)));
    apply_when(&mut rx, &mut store, "the client delete", |s| s.clients.is_empty());
    worker.stop();
}

/// The figures without record numbers: a record made here gets its number
/// from the cloud when the cloud accepts it.
fn numbers(book: &vz_finance::domain::portfolio::Book, today: NaiveDate) -> serde_json::Value {
    let mut f = figures(book, today);
    for loan in f["loans"].as_array_mut().unwrap() {
        loan.as_object_mut().unwrap().remove("no");
    }
    f
}

/// An ID photo and the logo set on the desktop reach the cloud, and an
/// admin's desktop fetches them back (the photo kept encrypted).
#[test]
#[ignore = "needs a local PWA backend; see the top of this file"]
fn photos_and_the_logo_go_both_ways() {
    let email = format!("desktop-photos-{}@example.com", std::process::id());
    let session = sign_in(&email);
    let business_id = create_business(&session, "Préstamos Fotos");
    let (worker, mut rx) =
        Worker::start(Config { endpoints: endpoints(), client_version: "0.7.0".into() }, Stored { email, session }).unwrap();
    worker.open(&business_id);
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().join("data").join("linked.db")).unwrap();
    apply_when(&mut rx, &mut store, "the empty books", |_| true);
    let today = store.cloud_link().unwrap().unwrap().today;

    let ana = ClientInput { full_name: "Ana Gómez".into(), ..Default::default() };
    assert!(matches!(
        save_and_send(&mut store, &worker, Change::NewClient { input: ana, photo: PhotoChange::Keep }, today),
        Outcome::Accepted(_)
    ));
    apply_when(&mut rx, &mut store, "the client", |s| s.clients.len() == 1);
    let client = store.snapshot().unwrap().clients[0].id;

    let mut picture = image::RgbImage::new(64, 48);
    picture.pixels_mut().for_each(|p| *p = image::Rgb([200, 120, 40]));
    let mut png = Vec::new();
    picture.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let photo = Photo::from_upload(&png).unwrap();
    let outcome = save_and_send(
        &mut store,
        &worker,
        Change::EditClient {
            id: client,
            input: ClientInput { full_name: "Ana Gómez".into(), ..Default::default() },
            photo: PhotoChange::Set(photo),
        },
        today,
    );
    assert!(matches!(outcome, Outcome::Accepted(_)), "{outcome:?}");
    let outcome = save_and_send(&mut store, &worker, Change::Logo(Some(png)), today);
    assert!(matches!(outcome, Outcome::Accepted(_)), "{outcome:?}");

    let snap = wait_for(&mut rx, "the photo and logo", |e| match e {
        Event::Snapshot(s) if s.clients[0].has_photo && s.settings.as_ref().is_some_and(|x| x.logo_url.is_some()) => Some(s),
        Event::Problem(p) => panic!("{p}"),
        _ => None,
    });
    store.apply_cloud(&snap).unwrap();
    store.forget_cloud_photos().unwrap();
    let wanted = store.cloud_downloads(&snap).unwrap();
    assert_eq!(wanted.len(), 2, "{wanted:?}");
    worker.download(wanted);
    for _ in 0..2 {
        let (file, bytes) = wait_for(&mut rx, "a download", |e| match e {
            Event::Downloaded { file, bytes } => Some((file, bytes)),
            Event::Problem(p) => panic!("{p}"),
            _ => None,
        });
        store.save_download(&file, &bytes).unwrap();
    }
    assert!(store.client_photo(client).unwrap().unwrap().starts_with(&[0xFF, 0xD8]));
    assert!(store.snapshot().unwrap().logo.is_some());
    assert!(store.cloud_downloads(&snap).unwrap().is_empty());
    worker.stop();
}

/// Waiving a late fee the cloud assessed. The cloud assesses late fees
/// hourly; while this waits, run `npx convex run lateFees:refreshAll` in the
/// PWA repo.
#[test]
#[ignore = "needs a local PWA backend and a late-fee run; see above"]
fn a_late_fee_is_waived_from_the_desktop() {
    let email = format!("desktop-fees-{}@example.com", std::process::id());
    let session = sign_in(&email);
    let business_id = create_business(&session, "Préstamos Mora");
    let (worker, mut rx) =
        Worker::start(Config { endpoints: endpoints(), client_version: "0.7.0".into() }, Stored { email, session }).unwrap();
    worker.open(&business_id);
    let tmp = tempfile::tempdir().unwrap();
    let mut store = Store::open(tmp.path().join("data").join("linked.db")).unwrap();
    apply_when(&mut rx, &mut store, "the empty books", |_| true);
    let today = store.cloud_link().unwrap().unwrap().today;
    let start = today - chrono::Days::new(45);

    let money = Change::NewCapital { kind: CapitalKind::Contribution, amount: 50_000_000, on: start, notes: String::new() };
    assert!(matches!(save_and_send(&mut store, &worker, money, today), Outcome::Accepted(_)));
    let ana = ClientInput { full_name: "Ana Gómez".into(), ..Default::default() };
    assert!(matches!(
        save_and_send(&mut store, &worker, Change::NewClient { input: ana, photo: PhotoChange::Keep }, today),
        Outcome::Accepted(_)
    ));
    apply_when(&mut rx, &mut store, "the client", |s| s.clients.len() == 1 && s.capital.len() == 1);
    let terms = LoanTerms {
        principal: 1_000_000,
        rate: Rate(100_000),
        interest_period: Frequency::Monthly,
        frequency: Frequency::Monthly,
        start_date: start,
        method: Method::Declining,
        installments: Some(3),
        late_fee_rate: Rate(50_000),
    };
    let client = store.snapshot().unwrap().clients[0].id;
    let loan = Change::NewLoan { client_id: client, terms, schedule: None, notes: String::new() };
    assert!(matches!(save_and_send(&mut store, &worker, loan, today), Outcome::Accepted(_)));
    println!("READY FOR LATE FEES");
    apply_when(&mut rx, &mut store, "the late fee", |s| !s.late_fees.is_empty());
    let loan = store.snapshot().unwrap().loans[0].id;
    let waive = Change::WaiveLateFee { loan_id: loan, seq: 1, waived: true, reason: "Cliente avisó".into() };
    let outcome = save_and_send(&mut store, &worker, waive, today);
    assert!(matches!(outcome, Outcome::Accepted(_)), "{outcome:?}");
    apply_when(&mut rx, &mut store, "the waiver", |s| s.late_fees[0].waived_at.is_some());
    worker.stop();
}

/// Not a check: signs `VZ_SYNC_EMAIL` in to the local backend and leaves the
/// session in Credential Manager, so a debug build started with
/// VZ_SYNC_CLOUD=http://127.0.0.1:3210 VZ_SYNC_ORIGIN=http://localhost:5173
/// opens signed in (for screenshots). `vault::forget` removes it again.
#[test]
#[ignore = "prepares a local session for screenshots; see above"]
fn remember_a_local_session_for_the_app() {
    let email = std::env::var("VZ_SYNC_EMAIL").expect("set VZ_SYNC_EMAIL");
    let session = sign_in(&email);
    vault::save(CLOUD, &Stored { email, session }).unwrap();
}

/// A session renewed by the cloud keeps working: the renewal may carry only
/// some of the cookies, so the stored session merges them.
#[test]
#[ignore = "needs a local PWA backend; see the top of this file"]
fn a_renewed_session_keeps_working() {
    let email = format!("desktop-renew-{}@example.com", std::process::id());
    let mut session = sign_in(&email);
    for round in 0..3 {
        let (_, renewed) = auth::token(&endpoints(), &session).unwrap_or_else(|e| panic!("round {round}: {e}"));
        if let Some(r) = renewed {
            let names =
                |s: &Session| s.as_str().split("; ").filter_map(|c| c.split_once('=')).map(|(n, _)| n.to_string()).collect::<Vec<_>>();
            println!("round {round}: had {:?}, renewal sent {:?}", names(&session), names(&r));
            session = session.merged(&r);
        }
    }
}

/// The server takes the desktop's Google request: the way back (the PWA's
/// handoff page with this PC's port) is accepted and the browser is sent to
/// Google, which then returns to the server. Needs `GOOGLE_CLIENT_ID` and
/// `GOOGLE_CLIENT_SECRET` set on the local backend (any value will do: Google
/// itself isn't reached).
#[test]
#[ignore = "needs a local PWA backend; see the top of this file"]
fn google_sign_in_starts_with_the_way_back_to_this_pc() {
    let g = auth::start_google(&endpoints()).unwrap();
    assert!(g.url.starts_with("https://accounts.google.com/"), "{}", g.url);
    let redirect = format!("redirect_uri={}", urlencode(&format!("{}/api/auth/callback/google", endpoints().site)));
    assert!(g.url.contains(&redirect), "{}", g.url);
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Undoes `remember_a_local_session_for_the_app`.
#[test]
#[ignore = "removes the screenshot session; see above"]
fn forget_the_local_session() {
    vault::forget(CLOUD);
    assert!(vault::load(CLOUD).is_none());
}
