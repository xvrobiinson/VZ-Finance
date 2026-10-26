//! Client details through the database: the optional ID number, address
//! and photo, edits that leave the money untouched, and a photo that
//! survives a restart, a backup and a restore without ever being printed.

use super::*;
use crate::domain::loans::{Frequency, LoanTerms};
use crate::domain::money::Rate;
use crate::domain::portfolio::CapitalKind;
use crate::services::picture::{Photo, tests::png};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn full(name: &str) -> ClientInput {
    ClientInput {
        full_name: name.into(),
        phone: "809-555-0101".into(),
        email: "ana@example.com".into(),
        id_number: "001-1234567-8".into(),
        address: "Calle 5 #12, Santiago".into(),
        notes: String::new(),
    }
}

fn photo() -> Photo {
    Photo::from_upload(&png(1200, 800)).unwrap()
}

#[test]
fn a_client_needs_only_a_name() {
    let mut s = Store::in_memory();
    let id = s.create_client(&ClientInput { full_name: "  Ana Ruiz ".into(), ..Default::default() }).unwrap();
    let c = s.snapshot().unwrap().client(id).cloned().unwrap();
    assert_eq!(c.full_name, "Ana Ruiz");
    assert_eq!((c.phone, c.id_number, c.address, c.has_photo), (None, None, None, false));
    assert!(s.client_photo(id).unwrap().is_none());
    assert!(s.create_client(&ClientInput { full_name: "   ".into(), ..Default::default() }).is_err());
}

#[test]
fn a_client_can_have_an_id_number_address_and_photo() {
    let mut s = Store::in_memory();
    let p = photo();
    let id = s.save_client(None, &full("Ana Ruiz"), &PhotoChange::Set(p.clone())).unwrap();
    let c = s.snapshot().unwrap().client(id).cloned().unwrap();
    assert_eq!(c.id_number.as_deref(), Some("001-1234567-8"));
    assert_eq!(c.address.as_deref(), Some("Calle 5 #12, Santiago"));
    assert!(c.has_photo);
    assert_eq!(s.client_photo(id).unwrap().as_deref(), Some(p.jpeg()));
}

#[test]
fn optional_details_are_checked() {
    let mut s = Store::in_memory();
    let mut input = full("Ana Ruiz");
    input.id_number = "9".repeat(41);
    assert!(s.create_client(&input).unwrap_err().downcast_ref::<Invalid>().is_some());
    input.id_number = "001".into();
    input.address = "line one\nline two".into();
    assert!(s.create_client(&input).unwrap_err().downcast_ref::<Invalid>().is_some());
    assert!(s.snapshot().unwrap().clients.is_empty(), "nothing is saved when a detail is refused");
}

#[test]
fn editing_a_client_is_logged_and_leaves_their_money_alone() {
    let today = d(2026, 6, 1);
    let mut s = Store::in_memory();
    let cid = s.create_client(&ClientInput { full_name: "Ana Ruiz".into(), ..Default::default() }).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
    let lid = s.create_loan(cid, &LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1)), "", today).unwrap();
    s.record_payment(lid, 60_000, 0, d(2026, 2, 1), "", today).unwrap();
    let before = s.snapshot().unwrap();

    s.save_client(Some(cid), &full("Ana María Ruiz"), &PhotoChange::Set(photo())).unwrap();
    s.save_client(Some(cid), &full("Ana María Ruiz"), &PhotoChange::Set(photo())).unwrap();
    s.save_client(Some(cid), &full("Ana María Ruiz"), &PhotoChange::Remove).unwrap();

    let after = s.snapshot().unwrap();
    assert_eq!(after.loans, before.loans);
    assert_eq!(after.payments, before.payments);
    assert_eq!(after.book.summary(today), before.book.summary(today));
    let c = after.client(cid).unwrap();
    assert_eq!((c.full_name.as_str(), c.has_photo), ("Ana María Ruiz", false));

    // Newest first: removed, replaced, then the first edit with every field.
    let photo_changes: Vec<_> = after
        .changes
        .iter()
        .filter_map(|ch| ch.fields.iter().find(|f| f.field == "photo"))
        .map(|f| (f.old.as_str(), f.new.as_str()))
        .collect();
    assert_eq!(photo_changes, vec![("on_file", ""), ("on_file", "replaced"), ("", "on_file")]);
    let first = after.changes.last().unwrap();
    let fields: Vec<_> = first.fields.iter().map(|f| f.field.as_str()).collect();
    assert_eq!(fields, vec!["full_name", "phone", "email", "id_number", "address", "photo"]);
}

#[test]
fn a_photo_survives_a_restart_a_backup_and_a_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data").join("portfolio.db");
    let p = photo();
    let id = {
        let mut s = Store::open(path.clone()).unwrap();
        s.save_client(None, &full("Ana Ruiz"), &PhotoChange::Set(p.clone())).unwrap()
    };
    let mut s = Store::open(path.clone()).unwrap();
    assert_eq!(s.client_photo(id).unwrap().as_deref(), Some(p.jpeg()));
    assert_eq!(s.snapshot().unwrap().client(id).unwrap().id_number.as_deref(), Some("001-1234567-8"));

    let backup = dir.path().join("backup.db");
    s.backup_to(&backup).unwrap();
    s.save_client(Some(id), &full("Ana Ruiz"), &PhotoChange::Remove).unwrap();
    assert!(s.client_photo(id).unwrap().is_none());
    s.restore_from(&backup).unwrap();
    assert_eq!(s.client_photo(id).unwrap().as_deref(), Some(p.jpeg()));

    // Deleting the client deletes the photo with them.
    s.delete_client(id, "").unwrap();
    let left: i64 = s.conn.query_row("SELECT COUNT(*) FROM client_photos", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
}

#[test]
fn photos_stay_out_of_receipts_exports_and_the_receipts_folder() {
    let today = d(2026, 6, 1);
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("data").join("portfolio.db")).unwrap();
    let cid = s.save_client(None, &full("Ana Ruiz"), &PhotoChange::Set(photo())).unwrap();
    s.add_capital(CapitalKind::Contribution, 2_000_000, d(2026, 1, 1), "", today).unwrap();
    let lid = s.create_loan(cid, &LoanTerms::interest_only(100_000, Rate(100_000), Frequency::Monthly, d(2026, 1, 1)), "", today).unwrap();
    let pid = s.record_payment(lid, 60_000, 0, d(2026, 2, 1), "", today).unwrap();

    let receipts = [s.write_receipt(pid).unwrap(), s.write_loan_receipt(lid).unwrap()];
    for r in &receipts {
        let html = std::fs::read_to_string(r).unwrap();
        assert!(html.contains("001-1234567-8"), "the ID number is printed by default");
        assert!(!html.contains("data:image/jpeg") && !html.contains("/9j/"), "no photo in {}", r.display());
    }
    let files: Vec<_> = std::fs::read_dir(s.receipts_dir()).unwrap().flatten().map(|e| e.path()).collect();
    assert!(files.iter().all(|f| f.extension().is_some_and(|x| x == "html")));

    let exported = s.export_csv(&dir.path().join("exports")).unwrap();
    let clients = std::fs::read_to_string(exported.iter().find(|p| p.to_string_lossy().contains("clients")).unwrap()).unwrap();
    assert!(clients.starts_with("id,full_name,id_number,phone,email,address,notes"));
    assert!(clients.len() < 500, "the photo is not exported");
}

#[test]
fn reset_removes_photos_too() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Store::open(dir.path().join("data").join("portfolio.db")).unwrap();
    s.save_client(None, &full("Ana Ruiz"), &PhotoChange::Set(photo())).unwrap();
    s.reset_all_data().unwrap();
    let left: i64 = s.conn.query_row("SELECT COUNT(*) FROM client_photos", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
}
