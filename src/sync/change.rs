//! Every change a person can save, in one place, so a linked database can
//! send the same change to the cloud.
//!
//! `Store::save` runs a change with the local code (the same rules and
//! messages as today) and, in a linked database, queues it for the cloud
//! (`outbox`) with the arguments of the PWA's mutation for it. Records are
//! named by their cloud ids (`remote_ids`); a record made on this PC that
//! the cloud hasn't accepted yet can't be changed until it has.

use anyhow::{Result, anyhow};
use chrono::NaiveDate;
use rusqlite::Connection;
use serde_json::{Value, json};

use super::replica::{self, Entity};
use crate::database::repositories::clients::ClientInput;
use crate::domain::installments::{Installment, Instruction};
use crate::domain::loans::LoanTerms;
use crate::domain::money::Cents;
use crate::domain::portfolio::{Account, CapitalKind, EntryKind};
use crate::i18n::t;
use crate::services::Store;
use crate::services::picture::{self, PhotoChange};
use crate::services::settings::Settings;

#[derive(Clone, Debug)]
pub enum Change {
    /// With an ID photo (admins). The cloud gets the photo right after the
    /// client.
    NewClient {
        input: ClientInput,
        photo: PhotoChange,
    },
    EditClient {
        id: i64,
        input: ClientInput,
        photo: PhotoChange,
    },
    ArchiveClient {
        id: i64,
        archived: bool,
    },
    DeleteClient {
        id: i64,
        reason: String,
    },
    NewLoan {
        client_id: i64,
        terms: LoanTerms,
        schedule: Option<Vec<Installment>>,
        notes: String,
    },
    EditLoan {
        id: i64,
        client_id: i64,
        terms: LoanTerms,
        schedule: Option<Vec<Installment>>,
        notes: String,
        reason: String,
    },
    DeleteLoan {
        id: i64,
        reason: String,
    },
    MarkDefaulted {
        id: i64,
        on: NaiveDate,
        note: String,
    },
    Reinstate {
        id: i64,
    },
    WaiveLateFee {
        loan_id: i64,
        seq: u32,
        waived: bool,
        reason: String,
    },
    /// `instruction` is `None` on an interest-only loan, which takes an
    /// optional `fee` instead.
    NewPayment {
        loan_id: i64,
        amount: Cents,
        fee: Cents,
        on: NaiveDate,
        notes: String,
        instruction: Option<Instruction>,
    },
    EditPayment {
        id: i64,
        amount: Cents,
        fee: Cents,
        on: NaiveDate,
        notes: String,
        instruction: Option<Instruction>,
        reason: String,
    },
    DeletePayment {
        id: i64,
        reason: String,
    },
    NewCapital {
        kind: CapitalKind,
        amount: Cents,
        on: NaiveDate,
        notes: String,
    },
    EditCapital {
        id: i64,
        kind: CapitalKind,
        amount: Cents,
        on: NaiveDate,
        notes: String,
        reason: String,
    },
    DeleteCapital {
        id: i64,
        reason: String,
    },
    /// An expense, income or transfer (from `account` to the other one).
    NewEntry {
        kind: EntryKind,
        account: Account,
        amount: Cents,
        on: NaiveDate,
        description: String,
    },
    AdjustBalances {
        capital: Cents,
        interest: Cents,
        on: NaiveDate,
        reason: String,
    },
    EditEntry {
        id: i64,
        account: Account,
        amount: Cents,
        on: NaiveDate,
        description: String,
        reason: String,
    },
    DeleteEntry {
        id: i64,
        reason: String,
    },
    /// The business logo, or none.
    Logo(Option<Vec<u8>>),
    Settings(Box<Settings>),
}

/// A change as the cloud takes it: a mutation and its arguments (without
/// `businessId`, which the sender adds).
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub mutation: &'static str,
    pub args: Value,
}

/// Mutations that take an `opId`: the cloud records each operation once
/// however often it is sent, so they can wait in the outbox while offline.
pub const OFFLINE: [&str; 3] = ["clients:create", "payments:record", "money:addEntry"];

impl Op {
    pub fn works_offline(&self) -> bool {
        OFFLINE.contains(&self.mutation)
    }
}

/// The new record's id, when the change made one.
pub type Saved = Option<i64>;

impl Change {
    /// Whether it can be saved without a connection and sent later: new
    /// clients, payments, expenses and income, as in the PWA.
    pub fn works_offline(&self) -> bool {
        matches!(
            self,
            Change::NewClient { .. } | Change::NewPayment { .. } | Change::NewEntry { kind: EntryKind::Expense | EntryKind::Income, .. }
        )
    }

    /// Runs the change with the local code.
    pub(crate) fn run(&self, s: &mut Store, today: NaiveDate) -> Result<Saved> {
        use Change::*;
        Ok(match self {
            NewClient { input, photo } => Some(s.save_client(None, input, photo)?),
            EditClient { id, input, photo } => s.save_client(Some(*id), input, photo).map(|_| None)?,
            ArchiveClient { id, archived } => s.set_client_archived(*id, *archived).map(|_| None)?,
            DeleteClient { id, reason } => s.delete_client(*id, reason).map(|_| None)?,
            NewLoan { client_id, terms, schedule, notes } => {
                Some(s.create_loan_with_schedule(*client_id, terms, schedule.clone(), notes, today)?)
            }
            EditLoan { id, client_id, terms, schedule, notes, reason } => {
                s.update_loan(*id, *client_id, terms, schedule.clone(), notes, reason, today).map(|_| None)?
            }
            DeleteLoan { id, reason } => s.delete_loan(*id, reason).map(|_| None)?,
            MarkDefaulted { id, on, note } => s.mark_defaulted(*id, *on, note, today).map(|_| None)?,
            Reinstate { id } => s.reinstate(*id, today).map(|_| None)?,
            WaiveLateFee { loan_id, seq, waived, reason } => s.set_late_fee_waived(*loan_id, *seq, *waived, reason, today).map(|_| None)?,
            NewPayment { loan_id, amount, fee, on, notes, instruction } => Some(match instruction {
                None => s.record_payment(*loan_id, *amount, *fee, *on, notes, today)?,
                Some(i) => s.record_installment_payment(*loan_id, *amount, *on, notes, i, today)?,
            }),
            EditPayment { id, amount, fee, on, notes, instruction, reason } => {
                match instruction {
                    None => s.update_payment(*id, *amount, *fee, *on, notes, reason, today)?,
                    Some(i) => s.update_installment_payment(*id, *amount, *on, notes, i, reason, today)?,
                }
                None
            }
            DeletePayment { id, reason } => s.delete_payment(*id, reason, today).map(|_| None)?,
            NewCapital { kind, amount, on, notes } => Some(s.add_capital(*kind, *amount, *on, notes, today)?),
            EditCapital { id, kind, amount, on, notes, reason } => {
                s.update_capital(*id, *kind, *amount, *on, notes, reason, today).map(|_| None)?
            }
            DeleteCapital { id, reason } => s.delete_capital(*id, reason).map(|_| None)?,
            NewEntry { kind, account, amount, on, description } => Some(match kind {
                EntryKind::Expense => s.add_expense(*account, *amount, *on, description, today)?,
                EntryKind::Income => s.add_income(*account, *amount, *on, description, today)?,
                EntryKind::Transfer => s.add_transfer(*account, *amount, *on, description, today)?,
                EntryKind::Adjustment => return Err(anyhow!("adjustments are made with AdjustBalances")),
            }),
            AdjustBalances { capital, interest, on, reason } => s.adjust_balances(*capital, *interest, *on, reason, today).map(|_| None)?,
            EditEntry { id, account, amount, on, description, reason } => {
                s.update_entry(*id, *account, *amount, *on, description, reason, today).map(|_| None)?
            }
            DeleteEntry { id, reason } => s.delete_entry(*id, reason).map(|_| None)?,
            Logo(Some(bytes)) => s.set_logo(bytes).map(|_| None)?,
            Logo(None) => s.remove_logo().map(|_| None)?,
            Settings(settings) => s.save_settings(settings).map(|_| None)?,
        })
    }

    /// The change in the cloud's terms. Fails when it names a record the
    /// cloud doesn't have yet.
    pub fn op(&self, conn: &Connection) -> Result<Op> {
        use Change::*;
        let id = |entity, local: i64| remote_id(conn, entity, local, 0);
        let (mutation, args) = match self {
            NewClient { input, photo } => ("clients:create", with_photo(client(input), photo)),
            EditClient { id: local, input, photo } => {
                let (cid, version) = id(Entity::Client, *local)?;
                let mut a = with_photo(client(input), photo);
                a["clientId"] = json!(cid);
                a["expectedVersion"] = json!(version);
                ("clients:update", a)
            }
            ArchiveClient { id: local, archived } => {
                ("clients:setArchived", json!({ "clientId": id(Entity::Client, *local)?.0, "archived": archived }))
            }
            DeleteClient { id: local, reason } => {
                ("clients:remove", json!({ "clientId": id(Entity::Client, *local)?.0, "reason": reason }))
            }
            NewLoan { client_id, terms: tm, schedule, notes } => (
                "loans:create",
                json!({ "clientId": id(Entity::Client, *client_id)?.0, "terms": terms(tm), "schedule": schedule.as_deref().map(installments), "notes": notes }),
            ),
            EditLoan { id: local, client_id, terms: tm, schedule, notes, reason } => {
                let (lid, version) = id(Entity::Loan, *local)?;
                (
                    "loans:update",
                    json!({ "loanId": lid, "expectedVersion": version, "clientId": id(Entity::Client, *client_id)?.0, "terms": terms(tm),
                            "schedule": schedule.as_deref().map(installments), "notes": notes, "reason": reason }),
                )
            }
            DeleteLoan { id: local, reason } => {
                let (lid, version) = id(Entity::Loan, *local)?;
                ("loans:remove", json!({ "loanId": lid, "expectedVersion": version, "reason": reason }))
            }
            MarkDefaulted { id: local, on, note } => {
                let (lid, version) = id(Entity::Loan, *local)?;
                ("loans:markDefaulted", json!({ "loanId": lid, "expectedVersion": version, "on": on.to_string(), "note": note }))
            }
            Reinstate { id: local } => {
                let (lid, version) = id(Entity::Loan, *local)?;
                ("loans:reinstate", json!({ "loanId": lid, "expectedVersion": version }))
            }
            WaiveLateFee { loan_id, seq, waived, reason } => {
                let (lid, version) = id(Entity::Loan, *loan_id)?;
                (
                    "loans:setLateFeeWaived",
                    json!({ "loanId": lid, "expectedVersion": version, "seq": seq, "waived": waived, "reason": reason }),
                )
            }
            NewPayment { loan_id, amount, fee, on, notes, instruction: i } => {
                let (lid, version) = id(Entity::Loan, *loan_id)?;
                (
                    "payments:record",
                    json!({ "loanId": lid, "loanVersion": version, "amount": amount, "fee": fee, "paidOn": on.to_string(),
                            "notes": notes, "instruction": instruction(i.as_ref())? }),
                )
            }
            EditPayment { id: local, amount, fee, on, notes, instruction: i, reason } => {
                let (pid, version) = id(Entity::Payment, *local)?;
                (
                    "payments:update",
                    json!({ "paymentId": pid, "expectedVersion": version, "amount": amount, "fee": fee, "paidOn": on.to_string(),
                            "notes": notes, "instruction": instruction(i.as_ref())?, "reason": reason }),
                )
            }
            DeletePayment { id: local, reason } => {
                let (pid, version) = id(Entity::Payment, *local)?;
                ("payments:remove", json!({ "paymentId": pid, "expectedVersion": version, "reason": reason }))
            }
            NewCapital { kind, amount, on, notes } => {
                ("money:addCapital", json!({ "kind": kind.key(), "amount": amount, "occurredOn": on.to_string(), "notes": notes }))
            }
            EditCapital { id: local, kind, amount, on, notes, reason } => {
                let (kid, version) = id(Entity::Capital, *local)?;
                (
                    "money:updateCapital",
                    json!({ "id": kid, "expectedVersion": version, "kind": kind.key(), "amount": amount, "occurredOn": on.to_string(),
                            "notes": notes, "reason": reason }),
                )
            }
            DeleteCapital { id: local, reason } => {
                let (kid, version) = id(Entity::Capital, *local)?;
                ("money:removeCapital", json!({ "id": kid, "expectedVersion": version, "reason": reason }))
            }
            NewEntry { kind, account, amount, on, description } => (
                "money:addEntry",
                json!({ "kind": kind.key(), "account": account.key(), "amount": amount, "occurredOn": on.to_string(), "description": description }),
            ),
            AdjustBalances { capital, interest, on, reason } => (
                "money:adjustBalances",
                json!({ "capital": capital, "interest": interest, "occurredOn": on.to_string(), "reason": reason }),
            ),
            EditEntry { id: local, account, amount, on, description, reason } => {
                let (eid, version) = id(Entity::Entry, *local)?;
                (
                    "money:updateEntry",
                    json!({ "id": eid, "expectedVersion": version, "account": account.key(), "amount": amount, "occurredOn": on.to_string(),
                            "description": description, "reason": reason }),
                )
            }
            DeleteEntry { id: local, reason } => {
                let (eid, version) = id(Entity::Entry, *local)?;
                ("money:removeEntry", json!({ "id": eid, "expectedVersion": version, "reason": reason }))
            }
            Logo(Some(bytes)) => {
                let png = picture::logo_png(bytes).map_err(|e| anyhow!(e.message()))?;
                ("settings:setLogo", json!({ "upload": upload(&png, "image/png") }))
            }
            Logo(None) => ("settings:setLogo", json!({ "storageId": null })),
            Settings(s) => ("settings:update", settings(conn, s)?),
        };
        Ok(Op { mutation, args })
    }

    /// A change waiting in the outbox, read back to show it on top of a new
    /// copy from the cloud. Only the kinds that can wait offline.
    pub fn from_op(conn: &Connection, op: &Op) -> Result<Change> {
        let a = &op.args;
        let text = |k: &str| a[k].as_str().unwrap_or_default().to_string();
        let int = |k: &str| a[k].as_i64().ok_or_else(|| anyhow!("{} has no {k}", op.mutation));
        let day = |k: &str| NaiveDate::parse_from_str(a[k].as_str().unwrap_or_default(), "%Y-%m-%d").map_err(|e| anyhow!("{k}: {e}"));
        match op.mutation {
            "clients:create" => Ok(Change::NewClient {
                input: ClientInput {
                    full_name: text("fullName"),
                    phone: text("phone"),
                    email: text("email"),
                    id_number: text("idNumber"),
                    address: text("address"),
                    notes: text("notes"),
                },
                photo: PhotoChange::Keep,
            }),
            "payments:record" => {
                let loan_id = replica::local(conn, Entity::Loan, a["loanId"].as_str().unwrap_or_default())?
                    .ok_or_else(|| anyhow!("the loan is no longer in the cloud"))?;
                let i = &a["instruction"];
                let instruction = match i["kind"].as_str() {
                    _ if replica::is_interest_only(conn, loan_id)? => None,
                    Some("installment") => Some(Instruction::Installment(i["seq"].as_u64().unwrap_or(0) as u32)),
                    Some("spread") => Some(Instruction::Spread(
                        i["parts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|p| (p["seq"].as_u64().unwrap_or(0) as u32, p["amount"].as_i64().unwrap_or(0)))
                            .collect(),
                    )),
                    Some("custom") => Some(Instruction::Custom {
                        principal: i["principal"].as_i64().unwrap_or(0),
                        interest: i["interest"].as_i64().unwrap_or(0),
                        fee: i["fee"].as_i64().unwrap_or(0),
                    }),
                    _ => Some(Instruction::Auto),
                };
                Ok(Change::NewPayment {
                    loan_id,
                    amount: int("amount")?,
                    fee: int("fee")?,
                    on: day("paidOn")?,
                    notes: text("notes"),
                    instruction,
                })
            }
            "money:addEntry" => Ok(Change::NewEntry {
                kind: EntryKind::from_key(&text("kind")).ok_or_else(|| anyhow!("entry kind"))?,
                account: Account::from_key(&text("account")).ok_or_else(|| anyhow!("account"))?,
                amount: int("amount")?,
                on: day("occurredOn")?,
                description: text("description"),
            }),
            other => Err(anyhow!("{other} doesn't wait offline")),
        }
    }
}

fn remote_id(conn: &Connection, entity: Entity, local: i64, seq: i64) -> Result<(String, i64)> {
    replica::remote(conn, entity, local, seq)?.ok_or_else(|| {
        anyhow!(
            "{}",
            t(
                "This record hasn't reached the cloud yet. Wait until it's synced, then try again.",
                "Este registro aún no llega a la nube. Espera a que se sincronice e inténtalo de nuevo."
            )
        )
    })
}

/// A file the worker uploads to the cloud's storage before the mutation,
/// which then gets the stored file's `storageId` in its place.
fn upload(bytes: &[u8], content_type: &str) -> Value {
    use base64::Engine as _;
    json!({ "contentType": content_type, "data": base64::engine::general_purpose::STANDARD.encode(bytes) })
}

/// A client's arguments with the photo change the worker makes once the
/// client is saved (`photo`: a file to upload; `removePhoto`).
fn with_photo(mut args: Value, photo: &PhotoChange) -> Value {
    match photo {
        PhotoChange::Set(p) => args["photo"] = upload(p.jpeg(), "image/jpeg"),
        PhotoChange::Remove => args["removePhoto"] = json!(true),
        PhotoChange::Keep => {}
    }
    args
}

fn client(c: &ClientInput) -> Value {
    json!({ "fullName": c.full_name, "phone": c.phone, "email": c.email, "idNumber": c.id_number, "address": c.address, "notes": c.notes })
}

fn terms(t: &LoanTerms) -> Value {
    json!({
        "principal": t.principal, "rate": t.rate.0, "interestPeriod": t.interest_period.key(), "frequency": t.frequency.key(),
        "startDate": t.start_date.to_string(), "method": t.method.key(), "installments": t.installments, "lateFeeRate": t.late_fee_rate.0,
    })
}

fn installments(s: &[Installment]) -> Value {
    s.iter().map(|x| json!({ "seq": x.seq, "due": x.due.to_string(), "principal": x.principal, "interest": x.interest })).collect()
}

fn instruction(i: Option<&Instruction>) -> Result<Value> {
    Ok(match i {
        None | Some(Instruction::Auto) => json!({ "kind": "auto" }),
        Some(Instruction::Installment(seq)) => json!({ "kind": "installment", "seq": seq }),
        Some(Instruction::Spread(parts)) => {
            json!({ "kind": "spread", "parts": parts.iter().map(|(seq, amount)| json!({ "seq": seq, "amount": amount })).collect::<Vec<_>>() })
        }
        Some(Instruction::Custom { principal, interest, fee }) => {
            json!({ "kind": "custom", "principal": principal, "interest": interest, "fee": fee })
        }
        Some(Instruction::Fixed(_)) => return Err(anyhow!("a stored split can't be sent")),
    })
}

/// Whether two settings differ in what the business shares through the
/// cloud (not this PC's language, theme, charts or sidebar).
pub fn business_differs(a: &Settings, b: &Settings) -> bool {
    a.receipt_name != b.receipt_name
        || a.company_address != b.company_address
        || a.company_phone != b.company_phone
        || a.receipt_company != b.receipt_company
        || a.receipt_client != b.receipt_client
        || a.receipt_text != b.receipt_text
        || a.currency_code != b.currency_code
        || a.number_style != b.number_style
        || a.date_style != b.date_style
        || a.default_rate != b.default_rate
        || a.default_frequency != b.default_frequency
        || a.late_fee_rate != b.late_fee_rate
        || a.receipt_after_payment != b.receipt_after_payment
        || a.receipt_after_loan != b.receipt_after_loan
}

/// The business settings. The language stays the cloud's: on this PC it is
/// the person's own choice.
fn settings(conn: &Connection, s: &Settings) -> Result<Value> {
    let language = replica::cloud_language(conn)?;
    let wording: serde_json::Map<String, Value> =
        s.receipt_text.to_pairs().into_iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (k.to_string(), json!(v))).collect();
    Ok(json!({
        "expectedVersion": replica::settings_version(conn)?,
        "receiptName": s.receipt_name.trim(),
        "companyAddress": s.company_address.trim(),
        "companyPhone": s.company_phone.trim(),
        "receiptCompany": { "name": s.receipt_company.name, "address": s.receipt_company.address, "phone": s.receipt_company.phone },
        "receiptClient": { "name": s.receipt_client.name, "idNumber": s.receipt_client.id_number, "phone": s.receipt_client.phone,
                           "address": s.receipt_client.address },
        "receiptText": wording,
        "currencyCode": s.currency_code,
        "numberStyle": s.number_style.key(),
        "dateStyle": s.date_style.key(),
        "language": language,
        "defaultRate": s.default_rate.0,
        "defaultFrequency": s.default_frequency.key(),
        "lateFeeRate": s.late_fee_rate.0,
        "receiptAfterPayment": s.receipt_after_payment,
        "receiptAfterLoan": s.receipt_after_loan,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::outbox::Outcome;
    use crate::sync::replica::tests::{book, snap};
    use serde_json::json;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 10).unwrap()
    }

    fn linked() -> Store {
        let mut s = Store::in_memory();
        s.apply_cloud(&snap(book())).unwrap();
        s
    }

    fn payment(amount: Cents) -> Change {
        let on = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
        Change::NewPayment { loan_id: 3, amount, fee: 0, on, notes: String::new(), instruction: Some(Instruction::Auto) }
    }

    fn juan() -> ClientInput {
        ClientInput { full_name: "Juan Pérez".into(), phone: "809-555".into(), id_number: "001".into(), ..Default::default() }
    }

    fn payments(s: &Store) -> usize {
        s.snapshot().unwrap().payments.len()
    }

    #[test]
    fn an_unlinked_database_saves_as_before() {
        let mut s = Store::in_memory();
        let input = ClientInput { full_name: "Ana".into(), ..Default::default() };
        let (id, queued) = s.save(&Change::NewClient { input, photo: PhotoChange::Keep }, today()).unwrap();
        assert!(id.is_some() && queued.is_none());
        assert!(s.outbox_waiting().unwrap().is_empty());
    }

    #[test]
    fn a_payment_is_saved_here_and_queued_for_the_cloud() {
        let mut s = linked();
        let (id, q) = s.save(&payment(20000), today()).unwrap();
        assert!(id.is_some());
        assert_eq!(payments(&s), 2);
        let q = q.unwrap();
        assert_eq!(q.op.mutation, "payments:record");
        assert!(q.op.works_offline());
        let a = &q.op.args;
        assert_eq!((a["loanId"].as_str(), a["loanVersion"].as_i64(), a["amount"].as_i64()), (Some("l3"), Some(5), Some(20000)));
        assert_eq!((a["paidOn"].as_str(), &a["instruction"]), (Some("2026-03-09"), &json!({ "kind": "auto" })));
        assert_eq!(a["opId"].as_str(), Some(q.op_id.as_str()));
        assert_eq!(s.outbox_waiting().unwrap(), vec![q]);
    }

    #[test]
    fn edits_name_the_version_they_started_from() {
        let mut s = linked();
        let input = ClientInput { full_name: "Juan Pérez".into(), phone: "809-555".into(), ..Default::default() };
        let (_, q) = s.save(&Change::EditClient { id: 7, input, photo: PhotoChange::Keep }, today()).unwrap();
        let q = q.unwrap();
        assert_eq!(
            (q.op.mutation, q.op.args["clientId"].as_str(), q.op.args["expectedVersion"].as_i64()),
            ("clients:update", Some("c7"), Some(2))
        );
        assert!(q.op.args.get("opId").is_none(), "only offline kinds carry an opId");
    }

    #[test]
    fn a_record_the_cloud_lacks_cant_be_changed_yet() {
        let mut s = linked();
        let input = ClientInput { full_name: "Nuevo".into(), ..Default::default() };
        let (id, _) = s.save(&Change::NewClient { input: input.clone(), photo: PhotoChange::Keep }, today()).unwrap();
        let err = s.save(&Change::EditClient { id: id.unwrap(), input, photo: PhotoChange::Keep }, today()).unwrap_err();
        assert!(err.to_string().contains("cloud") || err.to_string().contains("nube"), "{err}");
        assert_eq!(s.outbox_waiting().unwrap().len(), 1);
    }

    #[test]
    fn waiting_changes_show_on_top_of_each_new_copy() {
        let mut s = linked();
        s.save(&payment(20000), today()).unwrap();
        let input = ClientInput { full_name: "Nuevo".into(), ..Default::default() };
        s.save(&Change::NewClient { input, photo: PhotoChange::Keep }, today()).unwrap();
        // A new copy from the cloud, still without them: they stay on screen.
        s.apply_cloud(&snap(book())).unwrap();
        assert_eq!(payments(&s), 2);
        assert_eq!(s.snapshot().unwrap().clients.len(), 3);

        // Accepted: the copy that comes with it has them; nothing is doubled.
        let q = s.outbox_waiting().unwrap();
        let mut next = book();
        next["payments"].as_array_mut().unwrap().push(json!({
            "_id": "p13", "loanId": "l3", "recordNo": 13.0, "paidOn": "2026-03-09", "amount": 20000.0, "notes": "", "mode": "auto",
            "principal": 17250.0, "interest": 0.0, "fee": 2750.0,
            "applied": [{ "seq": 2.0, "principal": 17250.0, "interest": 0.0, "fee": 2750.0 }], "createdAt": 1773014400000.0, "version": 1.0
        }));
        assert!(s.outbox_finish(&q[0], &Outcome::Accepted(json!({ "paymentId": "p13" }))).unwrap());
        // Refused: put aside and no longer in the books.
        assert!(s.outbox_finish(&q[1], &Outcome::Refused(json!({ "code": "invalid" }))).unwrap());
        s.apply_cloud(&snap(next)).unwrap();
        assert_eq!(payments(&s), 2);
        assert_eq!(s.snapshot().unwrap().clients.len(), 2);
        assert!(s.outbox_waiting().unwrap().is_empty());
        assert_eq!(s.outbox_refused().unwrap()[0].error, json!({ "code": "invalid" }));
    }

    #[test]
    fn a_lost_answer_is_sent_again_only_when_that_is_safe() {
        let mut s = linked();
        let (_, pay) = s.save(&payment(20000), today()).unwrap();
        let on = NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
        let (_, cap) =
            s.save(&Change::NewCapital { kind: CapitalKind::Contribution, amount: 1000, on, notes: String::new() }, today()).unwrap();
        assert!(!s.outbox_finish(&pay.unwrap(), &Outcome::NoAnswer).unwrap());
        assert!(s.outbox_finish(&cap.clone().unwrap(), &Outcome::NoAnswer).unwrap());
        assert_eq!(s.outbox_waiting().unwrap().len(), 1, "the payment waits");
        assert_eq!(s.outbox_refused().unwrap()[0].error, json!({ "code": "unknown" }), "the capital entry needs a look");
        // Not sent at all: it simply waits.
        s.outbox_retry(&cap.as_ref().unwrap().op_id, None).unwrap();
        assert!(!s.outbox_finish(&cap.unwrap(), &Outcome::NotSent).unwrap());
        assert_eq!(s.outbox_waiting().unwrap().len(), 2);
    }

    #[test]
    fn later_payments_on_the_same_loan_follow_its_version() {
        let mut s = linked();
        let (_, first) = s.save(&payment(10000), today()).unwrap();
        s.save(&payment(10000), today()).unwrap();
        s.outbox_finish(&first.unwrap(), &Outcome::Accepted(json!({ "paymentId": "p13", "loanVersionBefore": 5, "loanVersionAfter": 6 })))
            .unwrap();
        assert_eq!(s.outbox_waiting().unwrap()[0].op.args["loanVersion"], 6);
    }

    #[test]
    fn settings_keep_the_clouds_language() {
        let mut s = linked();
        let mut settings = s.snapshot().unwrap().settings;
        settings.language = crate::i18n::Lang::En;
        settings.company_phone = "809-000".into();
        let (_, q) = s.save(&Change::Settings(Box::new(settings)), today()).unwrap();
        let a = q.unwrap().op.args;
        assert_eq!(
            (a["language"].as_str(), a["expectedVersion"].as_i64(), a["companyPhone"].as_str()),
            (Some("es"), Some(4), Some("809-000"))
        );
        assert_eq!(a["receiptText"], json!({ "receipt_payment_footer": "=Gracias" }));
    }

    #[test]
    fn photos_and_the_logo_travel_as_files_to_upload() {
        let mut s = linked();
        let png = crate::services::picture::tests::png(40, 30);
        let photo = picture::Photo::from_upload(&png).unwrap();
        let (_, q) = s.save(&Change::EditClient { id: 7, input: juan(), photo: PhotoChange::Set(photo) }, today()).unwrap();
        let a = q.unwrap().op.args;
        assert_eq!((a["clientId"].as_str(), a["photo"]["contentType"].as_str()), (Some("c7"), Some("image/jpeg")));
        assert!(s.client_photo(7).unwrap().is_some(), "shown here at once");
        let (_, q) = s.save(&Change::EditClient { id: 7, input: juan(), photo: PhotoChange::Remove }, today()).unwrap();
        assert_eq!(q.unwrap().op.args["removePhoto"], true);

        let (_, q) = s.save(&Change::Logo(Some(png)), today()).unwrap();
        let q = q.unwrap();
        assert_eq!((q.op.mutation, q.op.args["upload"]["contentType"].as_str()), ("settings:setLogo", Some("image/png")));
        let (_, q) = s.save(&Change::Logo(None), today()).unwrap();
        assert_eq!(q.unwrap().op.args, json!({ "storageId": null }));
    }
}
