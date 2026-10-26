use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::client::{ClientForm, OnCreated};
use super::date_field::{DateChanged, DateField};
use super::schedule::{COLUMN_GAP, COLUMNS, ScheduleChanged, ScheduleEditor, schedule_header, schedule_row};
use super::{
    body_room, choice_tile, date_field, error_banner, field, field_row, footer, grow_field, invalid_date, panel, picked_date, reason_input,
    value,
};
use crate::app::{Route, VzApp};
use crate::domain::installments::{self, Installment};
use crate::domain::ledger::{ContractStatus, LoanFacts};
use crate::domain::loans::{Frequency, LoanTerms, MAX_INSTALLMENTS, MAX_PERIOD_RATE, Method, RATE_PRESETS};
use crate::domain::money::{Cents, MoneyFormat, ParseError, Rate, parse_money};
use crate::domain::portfolio::{Account, Book};
use crate::i18n::t;
use crate::services::{Record, Snapshot};
use crate::tr;
use crate::ui::components::{icon, kv, num};
use crate::ui::kit::{Button, Segmented, Stat};
use crate::ui::theme::{Palette, RADIUS_CONTROL, Typeset as _, palette, space, text};

/// Narrowest the terms column gets beside the preview; it widens to fit
/// the payment-period control.
const TERMS_COLUMN: f32 = 440.0;
/// Room the schedule's "Capital left" column needs for its heading and a
/// six-figure amount.
const CAPITAL_LEFT: f32 = 88.0;

/// Narrowest the preview gets beside the terms: the schedule's fixed
/// columns, "Capital left", and the panel's padding.
fn preview_min() -> f32 {
    COLUMNS.iter().sum::<f32>() + COLUMN_GAP * COLUMNS.len() as f32 + CAPITAL_LEFT + 2.0 * space::L
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub id: i64,
    pub title: SharedString,
}

impl SearchableListItem for Choice {
    type Value = i64;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &i64 {
        &self.id
    }
}

pub struct LoanForm {
    app: WeakEntity<VzApp>,
    snap: Rc<Snapshot>,
    client: Entity<SelectState<Vec<Choice>>>,
    client_id: Option<i64>,
    principal: Entity<InputState>,
    method: Method,
    /// A preset rate, or `None` for a custom rate typed in `custom_rate`.
    preset: Option<Rate>,
    custom_rate: Entity<InputState>,
    interest_period: Frequency,
    frequency: Frequency,
    installments: Entity<InputState>,
    late_fee: Entity<InputState>,
    start: Entity<DateField>,
    notes: Entity<TextareaState>,
    /// Open when the schedule is being reviewed or edited by hand.
    schedule: Option<Entity<ScheduleEditor>>,
    /// The open schedule was changed by hand.
    schedule_edited: bool,
    /// Terms changed after the schedule was edited (it was kept).
    terms_moved: bool,
    /// The loan being edited, if any.
    editing: Option<i64>,
    reason: Entity<InputState>,
    /// New loans: open the printable loan receipt after saving.
    print_receipt: bool,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

struct Parsed {
    terms: Option<LoanTerms>,
    principal_err: Option<String>,
    rate_err: Option<String>,
    start_err: Option<String>,
    count_err: Option<String>,
    fee_err: Option<String>,
}

fn facts_for(terms: &LoanTerms) -> LoanFacts {
    LoanFacts {
        principal: terms.principal,
        rate: terms.period_rate(),
        frequency: terms.frequency,
        start_date: terms.start_date,
        status: ContractStatus::Active,
        closed_on: None,
    }
}

/// The clients a loan can go to, by name. Archived clients can't get new
/// loans, but a loan being edited keeps its client (`keep`).
pub fn client_choices(snap: &Snapshot, keep: Option<i64>) -> Vec<Choice> {
    snap.clients
        .iter()
        .filter(|c| !c.is_archived() || Some(c.id) == keep)
        .map(|c| Choice { id: c.id, title: c.full_name.clone().into() })
        .collect()
}

fn proposed(terms: &LoanTerms) -> Vec<Installment> {
    installments::proposed_schedule(&facts_for(terms), terms.method, terms.installments.unwrap_or(1))
}

impl LoanForm {
    pub fn new(
        app: WeakEntity<VzApp>,
        snap: Rc<Snapshot>,
        client_id: Option<i64>,
        editing: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let existing = editing.and_then(|id| snap.loan(id).cloned());
        let client_id = existing.as_ref().map(|l| l.client_id).or(client_id);
        let choices = client_choices(&snap, client_id.filter(|_| editing.is_some()));
        let selected = client_id.and_then(|id| choices.iter().position(|c| c.id == id)).map(IndexPath::new);
        let client = cx.new(|cx| SelectState::new(choices, selected, window, cx).searchable(true));
        let prefill = |s: InputState, v: Option<String>| match v {
            Some(v) => s.default_value(v),
            None => s,
        };
        let principal = cx.new(|cx| {
            prefill(InputState::new(window, cx).placeholder("0.00"), existing.as_ref().map(|l| MoneyFormat::input_value(l.terms.principal)))
        });
        let initial_rate = existing.as_ref().map_or(snap.settings.default_rate, |l| l.terms.rate);
        let preset = RATE_PRESETS.contains(&initial_rate).then_some(initial_rate);
        let custom_rate = cx.new(|cx| {
            let s = InputState::new(window, cx).placeholder(t("e.g. 12", "ej. 12"));
            if preset.is_none() { s.default_value(initial_rate.percent_label().trim_end_matches('%').to_string()) } else { s }
        });
        let count = existing.as_ref().and_then(|l| l.terms.installments).unwrap_or(5);
        let installments = cx.new(|cx| InputState::new(window, cx).placeholder("5").default_value(count.to_string()));
        let fee_rate = existing.as_ref().map_or(snap.settings.late_fee_rate, |l| l.terms.late_fee_rate);
        let late_fee = cx.new(|cx| InputState::new(window, cx).default_value(fee_rate.percent_label().trim_end_matches('%').to_string()));
        let start = date_field(existing.as_ref().map_or(crate::utils::dates::today(), |l| l.terms.start_date), window, cx);
        let notes = cx.new(|cx| {
            let s = TextareaState::new(window, cx).rows(2).placeholder(t("Purpose, collateral, agreement", "Motivo, garantía, acuerdo"));
            match &existing {
                Some(l) => s.default_value(l.notes.clone()),
                None => s,
            }
        });
        let frequency = existing.as_ref().map_or(snap.settings.default_frequency, |l| l.terms.frequency);
        let interest_period = existing.as_ref().map_or(frequency, |l| l.terms.interest_period);
        let method = existing.as_ref().map_or(Method::Declining, |l| l.terms.method);
        let reason = reason_input(window, cx);
        let print_receipt = snap.settings.receipt_after_loan;

        let mut subs = vec![
            cx.subscribe_in(&client, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.client_id = *v;
                cx.notify();
            }),
            cx.subscribe_in(&start, window, |this, _, _: &DateChanged, window, cx| this.terms_changed(window, cx)),
        ];
        for input in [&principal, &custom_rate, &installments, &late_fee] {
            subs.push(cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| {
                if let InputEvent::Change = ev {
                    this.terms_changed(window, cx)
                }
            }));
        }
        let mut form = Self {
            app,
            snap: snap.clone(),
            client,
            client_id,
            principal,
            method,
            preset,
            custom_rate,
            interest_period,
            frequency,
            installments,
            late_fee,
            start,
            notes,
            schedule: None,
            schedule_edited: false,
            terms_moved: false,
            editing,
            reason,
            print_receipt,
            error: None,
            show_errors: false,
            _subs: subs,
        };
        // A schedule edited by hand earlier opens as it was saved.
        if let Some(l) = existing.filter(|l| l.schedule_edited && !l.schedule.is_empty()) {
            form.open_schedule(&l.schedule, window, cx);
            form.schedule_edited = true;
        }
        form
    }

    /// The ledger this loan is checked against: without the loan itself when
    /// it is being edited.
    fn book(&self) -> Book {
        match self.editing {
            Some(id) => self.snap.book_without(Record::Loan(id)),
            None => self.snap.book.clone(),
        }
    }

    pub fn first_focus(&self, cx: &App) -> FocusHandle {
        if self.client_id.is_some() { self.principal.focus_handle(cx) } else { self.client.focus_handle(cx) }
    }

    pub fn client_id(&self) -> Option<i64> {
        self.client_id
    }

    /// Opens the client form on top of this one and returns it. Everything
    /// typed here stays as it is; a client saved there is selected here.
    pub fn add_client(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<ClientForm>> {
        let form = cx.entity().downgrade();
        let on_created: OnCreated = Rc::new(move |id, window, cx| {
            let _ = form.update(cx, |f, cx| f.client_added(id, window, cx));
        });
        self.app.update(cx, |app, cx| app.open_new_client_for(on_created, window, cx)).ok()
    }

    /// A client was just added: refresh the list and select them, then move
    /// on to the amount. Nothing else in the form changes.
    pub fn client_added(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(app) = self.app.upgrade() {
            self.snap = app.read(cx).snap.clone();
        }
        let keep = self.client_id.filter(|_| self.editing.is_some());
        let choices = client_choices(&self.snap, keep);
        self.client.update(cx, |s, cx| {
            s.set_items(choices, window, cx);
            s.set_selected_value(&id, window, cx);
        });
        self.client_id = Some(id);
        self.principal.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn open_schedule(&mut self, schedule: &[Installment], window: &mut Window, cx: &mut Context<Self>) {
        let money = self.snap.settings.money();
        let editor = cx.new(|cx| ScheduleEditor::new(schedule, money, window, cx));
        self._subs.push(cx.subscribe(&editor, |this, _, _: &ScheduleChanged, cx| {
            this.schedule_edited = true;
            cx.notify();
        }));
        self.schedule = Some(editor);
    }

    /// Something that shapes the schedule changed: an open schedule follows
    /// along unless it was edited by hand, which is never overwritten.
    fn terms_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = self.schedule.clone() {
            if self.schedule_edited {
                self.terms_moved = true;
            } else if let Some(terms) = self.parse(cx).terms.filter(|t| t.method.has_installments()) {
                let s = proposed(&terms);
                editor.update(cx, |e, cx| e.load(&s, window, cx));
            }
        }
        cx.notify();
    }

    fn recalculate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(editor), Some(terms)) = (self.schedule.clone(), self.parse(cx).terms) {
            let s = proposed(&terms);
            editor.update(cx, |e, cx| e.load(&s, window, cx));
        }
        self.schedule_edited = false;
        self.terms_moved = false;
        cx.notify();
    }

    fn rate(&self, cx: &App) -> Result<Rate, String> {
        if let Some(r) = self.preset {
            return Ok(r);
        }
        match Rate::from_percent_str(&value(&self.custom_rate, cx)) {
            Ok(r) if r <= MAX_PERIOD_RATE => Ok(r),
            Ok(_) | Err(ParseError::TooLarge) => Err(t("The rate can't be above 100%", "La tasa no puede ser mayor que 100%").into()),
            Err(ParseError::Empty) => Err(t("Enter the rate", "Escribe la tasa").into()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn parse(&self, cx: &App) -> Parsed {
        let principal = parse_money(&value(&self.principal, cx));
        let rate = self.rate(cx);
        let start = picked_date(&self.start, cx);
        let principal_err = match &principal {
            Ok(0) => Some(t("Enter an amount greater than zero", "Escribe un monto mayor que cero").to_string()),
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        };
        let today = crate::utils::dates::today();
        let start_err = match start {
            None => Some(invalid_date()),
            Some(d) if d > today => {
                Some(t("The loan date can't be in the future", "La fecha del préstamo no puede ser futura").to_string())
            }
            _ => None,
        };
        let has_installments = self.method.has_installments();
        let count = value(&self.installments, cx).trim().parse::<u32>().ok().filter(|n| (1..=MAX_INSTALLMENTS).contains(n));
        let count_err = (has_installments && count.is_none())
            .then(|| t("Enter between 1 and 1,000 installments", "Escribe entre 1 y 1,000 cuotas").to_string());
        let fee_text = value(&self.late_fee, cx);
        let fee = if fee_text.trim().is_empty() { Ok(Rate(0)) } else { Rate::from_percent_str(&fee_text) };
        let fee_err = match &fee {
            Ok(r) if *r <= MAX_PERIOD_RATE => None,
            Ok(_) => Some(t("The late fee can't be above 100%", "El cargo por mora no puede ser mayor que 100%").to_string()),
            Err(e) => Some(e.to_string()),
        }
        .filter(|_| has_installments);
        let terms = match (principal, &rate, start) {
            (Ok(p), Ok(r), Some(s)) if p > 0 && count_err.is_none() && fee_err.is_none() => Some(LoanTerms {
                principal: p,
                rate: *r,
                interest_period: self.interest_period,
                frequency: self.frequency,
                start_date: s,
                method: self.method,
                installments: if has_installments { count } else { None },
                late_fee_rate: if has_installments { fee.unwrap_or_default() } else { Rate(0) },
            }),
            _ => None,
        };
        Parsed { terms, principal_err, rate_err: rate.err(), start_err, count_err, fee_err }
    }

    /// The schedule to save: as edited, or the proposed one. `Err` explains
    /// what is wrong with an edited one.
    fn current_schedule(&self, terms: &LoanTerms, cx: &App) -> Result<Vec<Installment>, String> {
        if !terms.method.has_installments() {
            return Ok(Vec::new());
        }
        let Some(editor) = &self.schedule else { return Ok(proposed(terms)) };
        let s = editor.read(cx).schedule(cx)?;
        installments::check_schedule(terms.principal, terms.start_date, &s).map_err(|e| e.message(&self.snap.settings.money()))?;
        Ok(s)
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let parsed = self.parse(cx);
        let (Some(client_id), Some(terms), None) = (self.client_id, parsed.terms, parsed.start_err) else {
            cx.notify();
            return;
        };
        let schedule = match self.current_schedule(&terms, cx) {
            Ok(s) => s,
            Err(msg) => {
                self.error = Some(msg);
                cx.notify();
                return;
            }
        };
        // Only a schedule the user opened is sent; otherwise the service
        // works it out (or keeps an unchanged one).
        let schedule = self.schedule.as_ref().map(|_| schedule).filter(|_| terms.method.has_installments());
        let notes = self.notes.read(cx).value().to_string();
        let reason = value(&self.reason, cx);
        let editing = self.editing;
        let print = self.print_receipt && editing.is_none();
        let remember = self.print_receipt;
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            let r = match editing {
                Some(id) => app.store.update_loan(id, client_id, &terms, schedule, &notes, &reason, today).map(|_| id),
                None => app.store.create_loan_with_schedule(client_id, &terms, schedule, &notes, today),
            };
            match r {
                Ok(id) => {
                    if editing.is_none() && app.settings().receipt_after_loan != remember {
                        // Remember the choice for next time.
                        let s = app.settings().clone();
                        let _ = app.store.save_settings(&crate::services::settings::Settings { receipt_after_loan: remember, ..s });
                    }
                    app.reload(cx);
                    Ok(id)
                }
                Err(e) => Err(app.error_message(&e)),
            }
        });
        match result {
            Ok(Ok(id)) => {
                window.close_dialog(cx);
                let _ = self.app.update(cx, |app, cx| {
                    if editing.is_some() {
                        app.notify_success(
                            t("Loan updated. Its payments were worked out again.", "Préstamo actualizado. Sus pagos se recalcularon."),
                            window,
                            cx,
                        );
                        return;
                    }
                    let name = app.snap.client_name(client_id).to_string();
                    let amount = app.settings().money().format(terms.principal);
                    app.notify_success(tr!("Loan of {amount} to {name} created", "Préstamo de {amount} a {name} creado"), window, cx);
                    app.navigate(Route::Loan(id), window, cx);
                    if print {
                        app.open_loan_receipt(id, window, cx);
                    }
                });
            }
            Ok(Err(msg)) => {
                self.error = Some(msg);
                cx.notify();
            }
            Err(_) => window.close_dialog(cx),
        }
    }

    fn method_tile(&self, p: &Palette, m: Method, cx: &mut Context<Self>) -> impl IntoElement {
        let desc = match m {
            Method::Declining => t("Interest on the capital still owed", "Interés sobre el capital adeudado"),
            Method::Flat => t("Interest on the amount lent", "Interés sobre el monto prestado"),
            Method::InterestOnly => t("Interest each period, capital whenever", "Interés cada período, capital cuando quiera"),
        };
        let title = match m {
            Method::Declining => t("Installments", "Cuotas"),
            Method::Flat => t("Flat installments", "Cuotas fijas"),
            Method::InterestOnly => t("Interest only", "Solo interés"),
        };
        choice_tile(p, m.key(), title, Some(desc.to_string()), self.method == m).on_click(cx.listener(move |this, _, window, cx| {
            this.method = m;
            this.terms_changed(window, cx);
        }))
    }

    /// Daily / weekly / 15 days / monthly.
    fn period_control(&self, id: &'static str, selected: Frequency, interest: bool, cx: &mut Context<Self>) -> Segmented {
        let options = Frequency::ALL.map(|f| (f, SharedString::from(f.label()))).to_vec();
        Segmented::pick(id, options, selected, cx, move |this: &mut Self, f, window, cx| {
            if interest {
                this.interest_period = f;
            } else {
                this.frequency = f;
            }
            this.terms_changed(window, cx);
        })
    }

    fn preview(&self, p: &Palette, terms: Option<&LoanTerms>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let card = panel(p).gap(px(space::M));
        let Some(terms) = terms else {
            return card
                .h_full()
                .justify_center()
                .items_center()
                .child(icon(IconName::Calculator, p.faint, 20.))
                .child(div().max_w(px(280.)).text_center().typeset(text::BODY).text_color(p.muted).child(t(
                    "Enter the amount, rate and installments to see the payment schedule.",
                    "Escribe el monto, la tasa y las cuotas para ver el calendario de pagos.",
                )))
                .into_any_element();
        };
        if terms.method.has_installments() {
            self.installment_preview(p, card, terms, window, cx)
        } else {
            self.interest_only_preview(p, card, terms)
        }
    }

    fn installment_preview(&self, p: &Palette, card: Div, terms: &LoanTerms, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let money = self.snap.settings.money();
        let ds = self.snap.settings.date_style;
        let schedule = self.current_schedule(terms, cx);
        let shown = match (&schedule, &self.schedule) {
            (Ok(s), _) => s.clone(),
            (Err(_), Some(editor)) => editor.read(cx).schedule(cx).unwrap_or_default(),
            (Err(_), None) => proposed(terms),
        };
        let total_principal: Cents = shown.iter().map(|x| x.principal).sum();
        let total_interest: Cents = shown.iter().map(|x| x.interest).sum();
        let n = shown.len();
        let every = terms.frequency.label().to_lowercase();
        let rate = terms.period_rate();
        let fee = terms.late_fee_rate.percent_label();
        let bullet = |text: String| {
            h_flex()
                .items_start()
                .gap(px(space::S))
                .child(div().mt(px(7.)).size(px(4.)).rounded_full().bg(p.accent).flex_none())
                .child(div().flex_1().min_w_0().typeset(text::CAPTION).text_color(p.muted).child(text))
        };
        // Secondary figures beside the total: smaller, never truncated.
        let figure = |label: &str, value: String| {
            v_flex()
                .gap(px(2.))
                .child(div().typeset(text::CAPTION).text_color(p.muted).whitespace_nowrap().child(label.to_string()))
                .child(num(value).typeset(text::BODY_STRONG))
        };
        let table: AnyElement = match &self.schedule {
            Some(editor) => editor.clone().into_any_element(),
            None => {
                let mut left = terms.principal;
                let rows = shown.iter().map(|x| {
                    left -= x.principal;
                    schedule_row(
                        [
                            div().text_color(p.faint).child(x.seq.to_string()).into_any_element(),
                            div().child(ds.format(x.due)).into_any_element(),
                            div().child(money.format(x.principal)).into_any_element(),
                            div().child(money.format(x.interest)).into_any_element(),
                            div().typeset(text::BODY_STRONG).child(money.format(x.total())).into_any_element(),
                        ],
                        true,
                        div().text_color(p.muted).child(money.format(left)),
                    )
                    .h(px(26.))
                    .typeset(text::BODY)
                });
                v_flex()
                    .child(schedule_header(p, true))
                    .child(div().id("schedule-preview").max_h(px(260.)).overflow_y_scroll().child(v_flex().children(rows)))
                    .into_any_element()
            }
        };
        let unallocated = match &schedule {
            Err(_) => self
                .schedule
                .as_ref()
                .and_then(|e| e.read(cx).schedule(cx).ok())
                .map(|s| terms.principal - s.iter().map(|x| x.principal).sum::<Cents>()),
            Ok(_) => None,
        }
        .filter(|d| *d != 0);
        let schedule_actions = h_flex()
            .gap(px(space::S))
            .flex_wrap()
            .when(self.schedule.is_none(), |d| {
                d.child(
                    Button::new("edit-schedule").small().icon(IconName::Pencil).label(t("Edit schedule", "Editar calendario")).on_click(
                        cx.listener(|this, _, window, cx| {
                            if let Some(terms) = this.parse(cx).terms {
                                let s = proposed(&terms);
                                this.open_schedule(&s, window, cx);
                                cx.notify();
                            }
                        }),
                    ),
                )
            })
            .when(self.schedule.is_some() && self.schedule_edited, |d| {
                d.child(
                    Button::new("recalc-schedule")
                        .small()
                        .label(t("Use the calculated schedule", "Usar el calendario calculado"))
                        .on_click(cx.listener(|this, _, window, cx| this.recalculate(window, cx))),
                )
            })
            .when_some(unallocated, |d, diff| {
                let label = {
                    let a = money.format_signed(diff);
                    tr!("Put {a} on the last installment", "Poner {a} en la última cuota")
                };
                d.child(Button::new("fix-last").small().label(label).on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(editor) = this.schedule.clone() {
                        editor.update(cx, |e, cx| e.adjust_last(diff, window, cx));
                    }
                })))
            });
        card.child(
            h_flex()
                .flex_wrap()
                .items_end()
                .gap(px(space::XL))
                .child(Stat::new(t("Total to repay", "Total a pagar"), money.format(total_principal + total_interest)))
                .child(figure(t("Capital", "Capital"), money.format(total_principal)))
                .child(figure(t("Interest", "Interés"), money.format(total_interest)))
                .child(figure(t("Installments", "Cuotas"), format!("{n} · {every}"))),
        )
        .child(
            v_flex()
                .gap(px(space::XS))
                .child(bullet(terms.method.explanation().to_string()))
                .child(bullet({
                    let r = rate.label();
                    match rate.conversion_note() {
                        Some(note) => tr!("Rate: {note}, on a 30-day month.", "Tasa: {note}, sobre un mes de 30 días."),
                        None => tr!("Rate: {r}.", "Tasa: {r}."),
                    }
                }))
                .when(terms.late_fee_rate.0 > 0, |d| {
                    d.child(bullet(tr!(
                        "Late fee: {fee} of what is still unpaid on an installment, charged once the day after its due date.",
                        "Cargo por mora: {fee} de lo que falte por pagar de una cuota, cobrado una vez el día después de su vencimiento."
                    )))
                })
                .when(terms.method == Method::Declining, |d| {
                    d.child(bullet(
                        t(
                            "Paying capital early lowers the interest that follows; capital paid late keeps it higher.",
                            "Abonar capital antes baja el interés siguiente; el capital pagado tarde lo mantiene más alto.",
                        )
                        .to_string(),
                    ))
                }),
        )
        .when(self.terms_moved, |d| {
            d.child(
                div()
                    .px(px(space::M))
                    .py(px(space::S))
                    .rounded(px(RADIUS_CONTROL))
                    .bg(p.warning_tint)
                    .text_color(p.warning)
                    .typeset(text::CAPTION)
                    .child(t(
                        "The terms changed after you edited the schedule. Your edits were kept; use the calculated schedule to start over.",
                        "Los términos cambiaron después de editar el calendario. Se conservaron tus cambios; usa el calendario calculado para empezar de nuevo.",
                    )),
            )
        })
        .child(
            v_flex()
                .gap(px(space::S))
                .pt(px(space::S))
                .border_t_1()
                .border_color(p.border)
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_between()
                        .gap(px(space::M))
                        .child(div().typeset(text::BODY_STRONG).child(if self.schedule_edited {
                            t("Payment schedule · edited", "Calendario de pagos · editado")
                        } else {
                            t("Payment schedule", "Calendario de pagos")
                        }))
                        .child(schedule_actions),
                )
                .children(schedule.as_ref().err().map(|e| div().typeset(text::CAPTION).text_color(p.negative).child(e.clone())))
                .child(table),
        )
        .into_any_element()
    }

    fn interest_only_preview(&self, p: &Palette, card: Div, terms: &LoanTerms) -> AnyElement {
        let money = self.snap.settings.money();
        let ds = self.snap.settings.date_style;
        let rate = terms.period_rate();
        let interest = terms.interest_per_period();
        let half = terms.principal / 2;
        let example_pay = interest + half;
        let next_interest = rate.interest(terms.principal - half);
        let every = terms.frequency.per();
        let principal = money.format(terms.principal);
        let (i, h, pay, next) = (money.format(interest), money.format(half), money.format(example_pay), money.format(next_interest));
        let quoted = rate.label();
        let bullet = |text: String| {
            h_flex()
                .items_start()
                .gap(px(space::S))
                .child(div().mt(px(7.)).size(px(5.)).rounded_full().bg(p.accent).flex_none())
                .child(div().flex_1().min_w_0().typeset(text::BODY).text_color(p.text).child(text))
        };
        card.h_full()
            .child(Stat::new(tr!("Interest {every}", "Interés {every}"), i.clone()).caption(match rate.conversion_note() {
                Some(note) => note,
                None => tr!("{quoted} of {principal}", "{quoted} de {principal}"),
            }))
            .child(v_flex().child(kv(p, t("First payment due", "Primer pago"), ds.format(terms.first_due()))).child(kv(
                p,
                t("Loan date", "Fecha del préstamo"),
                ds.format(terms.start_date),
            )))
            .child(
                v_flex()
                    .gap(px(space::S))
                    .pt(px(space::M))
                    .border_t_1()
                    .border_color(p.border)
                    .child(div().typeset(text::BODY_STRONG).child(t("How payments work", "Cómo funcionan los pagos")))
                    .child(bullet(tr!(
                        "Paying {i} covers the interest. The capital stays at {principal}.",
                        "Si paga {i}, cubre el interés. El capital sigue en {principal}."
                    )))
                    .child(bullet(tr!(
                        "Paying {pay} is {i} of interest plus {h} of capital, so the next interest is only {next}.",
                        "Si paga {pay}, son {i} de interés y {h} de abono a capital, y el próximo interés baja a {next}."
                    )))
                    .child(bullet(
                        t(
                            "Any amount above the interest lowers the capital. The loan closes when the capital reaches zero.",
                            "Todo lo que pase del interés baja el capital. El préstamo se cierra cuando el capital llega a cero.",
                        )
                        .to_string(),
                    )),
            )
            .into_any_element()
    }
}

impl Render for LoanForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let parsed = self.parse(cx);
        let show = self.show_errors;
        let money = self.snap.settings.money();
        let book = self.book();
        let on = picked_date(&self.start, cx).unwrap_or(crate::utils::dates::today());
        let available = book.available_from(on, None);
        let principal_now = parse_money(&value(&self.principal, cx)).ok();
        let over_cash = principal_now.is_some_and(|a| a > available);
        let avail = money.format(available.max(0));
        // Loans take capital first, then interest.
        let from_capital = book.balances_on(on).get(Account::Capital).max(0);
        let principal_hint = Some(match principal_now {
            Some(amount) if amount > from_capital && !over_cash => {
                let (c, i) = (money.format(from_capital), money.format(amount - from_capital));
                tr!("{c} from capital and {i} from interest", "{c} del capital y {i} del interés")
            }
            _ => tr!("Available to lend: {avail}", "Disponible para prestar: {avail}"),
        });
        let principal_err = if over_cash {
            Some(tr!("Only {avail} is available to lend on this date", "Solo hay {avail} disponible para prestar en esta fecha"))
        } else if show {
            parsed.principal_err.clone()
        } else {
            None
        };
        let client_err = (show && self.client_id.is_none()).then(|| {
            if self.snap.clients.iter().all(|c| c.is_archived()) {
                t("Add a client first", "Agrega un cliente primero").to_string()
            } else {
                t("Choose a client", "Elige un cliente").to_string()
            }
        });
        let schedule_ok = parsed.terms.as_ref().is_some_and(|t| self.current_schedule(t, cx).is_ok());
        let can_submit = self.client_id.is_some() && parsed.terms.is_some() && parsed.start_err.is_none() && !over_cash && schedule_ok;
        // A typed custom rate is checked as you type; an empty one only on submit.
        let typed = self.preset.is_none() && !value(&self.custom_rate, cx).trim().is_empty();
        let rate_err = parsed.rate_err.clone().filter(|_| show || typed);
        let installment_loan = self.method.has_installments();

        // The terms sit beside the preview when the window leaves room for
        // both, and above it otherwise.
        let column = Segmented::fitted_width(&Frequency::ALL.map(|f| f.label())).max(TERMS_COLUMN);
        let side_by_side = body_room(window) >= column + space::XL + preview_min();

        let presets = RATE_PRESETS.iter().map(|r| (Some(*r), SharedString::from(r.percent_label())));
        let rate_options: Vec<_> = presets.chain([(None, SharedString::from(t("Other", "Otra")))]).collect();
        let rate_presets = Segmented::pick("rate", rate_options, self.preset, cx, |this: &mut Self, r, window, cx| {
            this.preset = r;
            this.terms_changed(window, cx);
        });
        let rate_control = v_flex()
            .gap(px(space::S))
            .child(h_flex().flex_wrap().gap(px(space::S)).child(rate_presets).when(self.preset.is_none(), |d| {
                d.child(div().w(px(92.)).child(Input::new(&self.custom_rate).suffix(div().text_color(p.muted).child("%"))))
            }))
            .child(self.period_control("per", self.interest_period, true, cx));
        let rate_label = {
            let per = self.interest_period.per();
            tr!("Interest rate · {per}", "Tasa de interés · {per}")
        };

        let terms = v_flex()
            .gap(px(space::M + 2.))
            .map(|d| if side_by_side { d.w(px(column)).flex_none() } else { d.w_full() })
            .children(error_banner(&p, &self.error))
            .child(field(
                &p,
                t("Client", "Cliente"),
                h_flex()
                    .gap(px(space::S))
                    .child(
                        div().flex_1().min_w_0().child(
                            Select::new(&self.client)
                                .placeholder(t("Choose a client", "Elige un cliente"))
                                .search_placeholder(t("Search clients", "Buscar clientes")),
                        ),
                    )
                    .child(
                        Button::new("add-client")
                            .icon(IconName::Plus)
                            .label(t("Add new client", "Agregar cliente nuevo"))
                            .tooltip(t("Add a client without leaving this loan", "Agrega un cliente sin salir de este préstamo"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_client(window, cx);
                            })),
                    ),
                None,
                client_err,
            ))
            .child(field(
                &p,
                t("Amount lent", "Monto prestado"),
                Input::new(&self.principal).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                principal_hint,
                principal_err,
            ))
            .child(field(
                &p,
                t("Loan type", "Tipo de préstamo"),
                h_flex().gap(px(space::S)).items_stretch().children(Method::ALL.map(|m| self.method_tile(&p, m, cx).into_any_element())),
                None,
                None,
            ))
            .child(field(&p, &rate_label, rate_control, None, rate_err))
            .child(field(
                &p,
                t("Payments due", "Pagos"),
                self.period_control("every", self.frequency, false, cx),
                installment_loan.then(|| t("How often installments fall due", "Cada cuánto vencen las cuotas").to_string()),
                None,
            ))
            .child(
                field_row()
                    .child(grow_field(field(
                        &p,
                        t("Loan date", "Fecha del préstamo"),
                        self.start.clone(),
                        None,
                        parsed.start_err.clone().filter(|_| show),
                    )))
                    .when(installment_loan, |d| {
                        d.child(div().w(px(110.)).flex_none().child(field(
                            &p,
                            t("Installments", "Cuotas"),
                            Input::new(&self.installments),
                            None,
                            parsed.count_err.clone(),
                        )))
                        .child(div().w(px(130.)).flex_none().child(field(
                            &p,
                            t("Late fee", "Cargo por mora"),
                            Input::new(&self.late_fee).suffix(div().text_color(p.muted).child("%")),
                            Some(t("Once per overdue installment", "Una vez por cuota atrasada").to_string()),
                            parsed.fee_err.clone(),
                        )))
                    }),
            )
            .child(field(&p, t("Notes", "Notas"), Textarea::new(&self.notes), None, None))
            .when(self.editing.is_none(), |d| {
                d.child(
                    h_flex()
                        .gap(px(10.))
                        .child(Switch::new("print-loan-receipt").checked(self.print_receipt).on_click(cx.listener(
                            |this, checked: &bool, _, cx| {
                                this.print_receipt = *checked;
                                cx.notify();
                            },
                        )))
                        .child(div().typeset(text::BODY).child(t(
                            "Open the loan receipt to print after saving",
                            "Abrir el recibo del préstamo para imprimir al guardar",
                        ))),
                )
            })
            .when_some(self.editing, |d, id| {
                let n = self.snap.payments_for_loan(id).filter(|x| !x.is_reversed()).count();
                d.child(field(
                    &p,
                    t("Reason for the change", "Motivo del cambio"),
                    Input::new(&self.reason),
                    (n > 0).then(|| {
                        let n = crate::i18n::count(n, "payment", "payments", "pago", "pagos");
                        tr!("Its {n} will be worked out again.", "Sus {n} se volverán a calcular.")
                    }),
                    None,
                ))
            });

        let review = v_flex()
            .gap(px(space::S))
            .map(|d| if side_by_side { d.flex_1().min_w_0().self_stretch() } else { d.w_full() })
            .child(div().typeset(text::LABEL).child(t("Review before saving", "Revisa antes de guardar")))
            .child(div().when(side_by_side, |d| d.flex_1()).child(self.preview(&p, parsed.terms.as_ref(), window, cx)))
            .child(footer(
                &p,
                if self.editing.is_some() { t("Save changes", "Guardar cambios") } else { t("Create loan", "Crear préstamo") },
                can_submit,
                false,
                cx.listener(|this, _, window, cx| this.submit(window, cx)),
            ));

        if side_by_side {
            h_flex().items_start().gap(px(space::XL)).child(terms).child(review)
        } else {
            v_flex().gap(px(space::XL)).child(terms).child(review)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    // The kit's glob import also brings GPUI's `test` macro; keep the
    // built-in one for what `#[gpui::test]` expands to.
    use ::gpui;
    use core::prelude::v1::test;
    use gpui_kit::component::Root;

    use super::*;
    use crate::database::repositories::clients::ClientInput;
    use crate::domain::portfolio::CapitalKind;
    use crate::services::Store;

    /// The app in a test window over an in-memory database with one client
    /// and money to lend, and the New loan form open.
    fn loan_form(cx: &mut TestAppContext) -> (Entity<VzApp>, Entity<LoanForm>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::theme::install(Default::default(), cx);
            cx.set_reduce_motion(true);
        });
        let today = crate::utils::dates::today();
        let mut store = Store::in_memory();
        store.create_client(&ClientInput { full_name: "Ana Ruiz".into(), ..Default::default() }).unwrap();
        store.add_capital(CapitalKind::Contribution, 10_000_000, today, "", today).unwrap();
        let slot = Rc::new(RefCell::new(None));
        let keep = slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let app = cx.new(|cx| VzApp::new(store, window, cx));
            *keep.borrow_mut() = Some(app.clone());
            Root::new(app, window, cx)
        });
        let app = slot.borrow_mut().take().unwrap();
        let form = cx.update(|window, cx| app.update(cx, |a, cx| a.loan_dialog(None, None, window, cx)));
        cx.run_until_parked();
        // Something typed before adding the client.
        cx.update(|window, cx| {
            form.update(cx, |f, cx| {
                f.principal.update(cx, |s, cx| s.set_value("2500", window, cx));
                f.notes.update(cx, |s, cx| s.set_value("Motorcycle", window, cx));
                // Never open a browser from a test.
                f.print_receipt = false;
            })
        });
        (app, form, cx)
    }

    fn clients(app: &Entity<VzApp>, cx: &mut VisualTestContext) -> Vec<(i64, String)> {
        cx.update(|_, cx| app.read(cx).snap.clients.iter().map(|c| (c.id, c.full_name.clone())).collect())
    }

    fn typed(form: &Entity<LoanForm>, cx: &mut VisualTestContext) -> (String, String) {
        cx.update(|_, cx| {
            let f = form.read(cx);
            (value(&f.principal, cx), f.notes.read(cx).value().to_string())
        })
    }

    /// How many dialogs are open, found by closing them one by one.
    fn close_all(cx: &mut VisualTestContext) -> usize {
        let mut n = 0;
        while cx.update(|window, cx| window.has_active_dialog(cx)) {
            cx.update(|window, cx| window.close_dialog(cx));
            cx.run_until_parked();
            n += 1;
        }
        n
    }

    #[gpui::test]
    fn a_client_added_from_the_loan_form_is_selected_and_the_loan_is_theirs(cx: &mut TestAppContext) {
        let (app, form, cx) = loan_form(cx);
        let client = cx.update(|window, cx| form.update(cx, |f, cx| f.add_client(window, cx))).unwrap();
        cx.run_until_parked();

        // A failed save keeps the client form open with what was typed.
        let long = "x".repeat(201);
        cx.update(|window, cx| {
            client.update(cx, |f, cx| {
                f.name.update(cx, |s, cx| s.set_value(long.clone(), window, cx));
                f.submit(window, cx);
            })
        });
        assert_eq!(clients(&app, cx).len(), 1);
        assert_eq!(cx.update(|_, cx| value(&client.read(cx).name, cx)), long);

        // Saving twice in a row (a double click, or Enter and a click) adds one client.
        cx.update(|window, cx| {
            client.update(cx, |f, cx| {
                f.name.update(cx, |s, cx| s.set_value("Bruno Díaz", window, cx));
                f.submit(window, cx);
                f.submit(window, cx);
            })
        });
        cx.update(|window, cx| client.update(cx, |f, cx| f.submit(window, cx)));
        cx.run_until_parked();
        let all = clients(&app, cx);
        assert_eq!(all.iter().filter(|(_, n)| n == "Bruno Díaz").count(), 1);
        let bruno = all.iter().find(|(_, n)| n == "Bruno Díaz").unwrap().0;

        // The loan form selected them, kept what was typed, and lists them.
        assert_eq!(cx.update(|_, cx| form.read(cx).client_id()), Some(bruno));
        assert_eq!(cx.update(|_, cx| form.read(cx).client.read(cx).selected_value().copied()), Some(bruno));
        assert_eq!(typed(&form, cx), ("2500".into(), "Motorcycle".into()));

        // The loan is created for the new client.
        cx.update(|window, cx| form.update(cx, |f, cx| f.submit(window, cx)));
        cx.run_until_parked();
        let loans: Vec<_> = cx.update(|_, cx| app.read(cx).snap.loans.iter().map(|l| (l.client_id, l.terms.principal)).collect());
        assert_eq!(loans, vec![(bruno, 250_000)]);
        assert_eq!(close_all(cx), 0, "both forms closed");
    }

    #[gpui::test]
    fn cancelling_the_client_form_leaves_the_loan_form_as_it_was(cx: &mut TestAppContext) {
        let (app, form, cx) = loan_form(cx);
        cx.update(|window, cx| form.update(cx, |f, cx| f.add_client(window, cx))).unwrap();
        cx.run_until_parked();
        // Cancel closes only the client form.
        cx.update(|window, cx| window.close_dialog(cx));
        cx.run_until_parked();
        assert_eq!(clients(&app, cx).len(), 1);
        assert_eq!(cx.update(|_, cx| form.read(cx).client_id()), None);
        assert_eq!(typed(&form, cx), ("2500".into(), "Motorcycle".into()));
        assert_eq!(close_all(cx), 1, "the loan form is still open");
    }

    #[gpui::test]
    fn the_client_form_opens_over_the_loan_form(cx: &mut TestAppContext) {
        let (_, form, cx) = loan_form(cx);
        cx.update(|window, cx| form.update(cx, |f, cx| f.add_client(window, cx))).unwrap();
        cx.run_until_parked();
        assert_eq!(close_all(cx), 2);
    }
}
