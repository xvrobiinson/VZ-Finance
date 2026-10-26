use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::schedule::{ScheduleChanged, ScheduleEditor};
use super::{date_field, error_banner, field, footer, invalid_date, picked_date, reason_input, value};
use crate::app::{Route, VzApp};
use crate::domain::installments::{self, Installment};
use crate::domain::ledger::{ContractStatus, LoanFacts};
use crate::domain::loans::{Frequency, LoanTerms, MAX_INSTALLMENTS, MAX_PERIOD_RATE, Method, RATE_PRESETS};
use crate::domain::money::{Cents, MoneyFormat, ParseError, Rate, parse_money};
use crate::domain::portfolio::{Account, Book};
use crate::i18n::t;
use crate::services::{Record, Snapshot};
use crate::tr;
use crate::ui::components::{icon, kv, num, th};
use crate::ui::theme::{Palette, palette, size, space, tabular};

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
        // Archived clients can't get new loans, but a loan keeps its client.
        let choices: Vec<Choice> = snap
            .clients
            .iter()
            .filter(|c| !c.is_archived() || Some(c.id) == client_id.filter(|_| editing.is_some()))
            .map(|c| Choice { id: c.id, title: c.full_name.clone().into() })
            .collect();
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

    fn chip(
        &self,
        p: &Palette,
        id: impl Into<ElementId>,
        label: String,
        selected: bool,
        on: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(32.))
            .px(px(12.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(if selected { p.accent } else { p.control })
            .when(selected, |d| d.bg(p.accent_tint).text_color(p.accent_text).font_weight(FontWeight::MEDIUM))
            .when(!selected, |d| d.hover(|s| s.bg(p.hover)))
            .cursor_pointer()
            .whitespace_nowrap()
            .child(label)
            .on_click(cx.listener(move |this, _, window, cx| {
                on(this, window, cx);
                this.terms_changed(window, cx);
            }))
    }

    fn method_tile(&self, p: &Palette, m: Method, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.method == m;
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
        v_flex()
            .id(m.key())
            .flex_1()
            .min_w_0()
            .p(px(10.))
            .gap(px(2.))
            .rounded(px(8.))
            .border_1()
            .border_color(if selected { p.accent } else { p.border })
            .when(selected, |d| d.bg(p.accent_tint))
            .when(!selected, |d| d.hover(|s| s.bg(p.hover)))
            .cursor_pointer()
            .child(div().font_weight(FontWeight::MEDIUM).text_color(if selected { p.accent_text } else { p.text }).child(title))
            .child(div().text_size(px(size::META)).text_color(p.muted).child(desc))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.method = m;
                this.terms_changed(window, cx);
            }))
    }

    /// Daily / weekly / 15 days / monthly, as small chips.
    fn period_chips(&self, p: &Palette, id: &'static str, selected: Frequency, interest: bool, cx: &mut Context<Self>) -> Div {
        h_flex().gap(px(6.)).flex_wrap().children(Frequency::ALL.map(|f| {
            self.chip(
                p,
                SharedString::from(format!("{id}-{}", f.key())),
                f.label().to_string(),
                selected == f,
                move |this, _, _| {
                    if interest {
                        this.interest_period = f;
                    } else {
                        this.frequency = f;
                    }
                },
                cx,
            )
        }))
    }

    fn preview(&self, p: &Palette, terms: Option<&LoanTerms>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let card = v_flex().p(px(space::L + 2.)).gap(px(space::M)).rounded(px(10.)).bg(p.raised).border_1().border_color(p.rule);
        let Some(terms) = terms else {
            return card
                .h_full()
                .justify_center()
                .items_center()
                .child(icon(IconName::Calculator, p.faint, 20.))
                .child(div().max_w(px(280.)).text_center().text_color(p.muted).child(t(
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
                .gap(px(8.))
                .child(div().mt(px(7.)).size(px(4.)).rounded_full().bg(p.accent).flex_none())
                .child(div().flex_1().min_w_0().text_size(px(size::META + 0.5)).text_color(p.muted).child(text))
        };
        let figure = |label: &str, value: String, big: bool| {
            v_flex()
                .flex_1()
                .flex_basis(px(0.))
                .when(big, |d| d.flex_grow(1.5))
                .min_w_0()
                .gap(px(2.))
                .child(div().text_size(px(size::META)).text_color(p.muted).truncate().child(label.to_string()))
                .child(num(value).truncate().text_size(px(if big { size::FIGURE } else { size::META + 4. })).font_weight(if big {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                }))
        };
        let table: AnyElement = match &self.schedule {
            Some(editor) => editor.clone().into_any_element(),
            None => {
                let mut left = terms.principal;
                let rows = shown.iter().map(|x| {
                    left -= x.principal;
                    h_flex()
                        .h(px(26.))
                        .gap(px(6.))
                        .font_features(tabular())
                        .text_size(px(size::META + 0.5))
                        .child(div().w(px(24.)).text_color(p.faint).child(x.seq.to_string()))
                        .child(div().w(px(112.)).child(ds.format(x.due)))
                        .child(div().w(px(104.)).text_right().child(money.format(x.principal)))
                        .child(div().w(px(96.)).text_right().child(money.format(x.interest)))
                        .child(div().w(px(104.)).text_right().font_weight(FontWeight::MEDIUM).child(money.format(x.total())))
                        .child(div().flex_1().text_right().text_color(p.muted).child(money.format(left)))
                });
                v_flex()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .pb(px(4.))
                            .child(div().w(px(24.)).child(th(p, "#")))
                            .child(div().w(px(112.)).child(th(p, t("Due date", "Vence"))))
                            .child(div().w(px(104.)).text_right().child(th(p, t("Capital", "Capital"))))
                            .child(div().w(px(96.)).text_right().child(th(p, t("Interest", "Interés"))))
                            .child(div().w(px(104.)).text_right().child(th(p, t("Total", "Total"))))
                            .child(div().flex_1().text_right().child(th(p, t("Capital left", "Capital restante")))),
                    )
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
                .gap(px(space::L))
                .child(figure(t("Total to repay", "Total a pagar"), money.format(total_principal + total_interest), true))
                .child(figure(t("Capital", "Capital"), money.format(total_principal), false))
                .child(figure(t("Interest", "Interés"), money.format(total_interest), false))
                .child(figure(t("Installments", "Cuotas"), format!("{n} · {every}"), false)),
        )
        .child(
            v_flex()
                .gap(px(4.))
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
                    .py(px(8.))
                    .rounded(px(8.))
                    .bg(p.warning_tint)
                    .text_color(p.warning)
                    .text_size(px(size::META + 0.5))
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
                        .justify_between()
                        .gap(px(space::M))
                        .child(div().font_weight(FontWeight::MEDIUM).child(if self.schedule_edited {
                            t("Payment schedule · edited", "Calendario de pagos · editado")
                        } else {
                            t("Payment schedule", "Calendario de pagos")
                        }))
                        .child(schedule_actions),
                )
                .children(schedule.as_ref().err().map(|e| div().text_size(px(size::META + 0.5)).text_color(p.negative).child(e.clone())))
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
                .gap(px(8.))
                .child(div().mt(px(7.)).size(px(5.)).rounded_full().bg(p.accent).flex_none())
                .child(div().flex_1().min_w_0().text_color(p.text).child(text))
        };
        card.h_full()
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(div().text_size(px(size::META)).text_color(p.muted).child(tr!("Interest {every}", "Interés {every}")))
                    .child(num(i.clone()).text_size(px(size::FIGURE + 2.)).line_height(px(30.)).font_weight(FontWeight::MEDIUM))
                    .child(div().text_size(px(size::META)).text_color(p.muted).child(match rate.conversion_note() {
                        Some(note) => note,
                        None => tr!("{quoted} of {principal}", "{quoted} de {principal}"),
                    })),
            )
            .child(v_flex().child(kv(p, t("First payment due", "Primer pago"), ds.format(terms.first_due()))).child(kv(
                p,
                t("Loan date", "Fecha del préstamo"),
                ds.format(terms.start_date),
            )))
            .child(
                v_flex()
                    .gap(px(8.))
                    .pt(px(space::M))
                    .border_t_1()
                    .border_color(p.border)
                    .child(div().font_weight(FontWeight::MEDIUM).child(t("How payments work", "Cómo funcionan los pagos")))
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

        let mut chips = h_flex().gap(px(6.)).flex_wrap();
        for (i, r) in RATE_PRESETS.iter().enumerate() {
            let r = *r;
            let id: &'static str = ["rate-5", "rate-10", "rate-15"][i];
            chips = chips.child(self.chip(&p, id, r.percent_label(), self.preset == Some(r), move |this, _, _| this.preset = Some(r), cx));
        }
        chips = chips.child(self.chip(
            &p,
            "rate-other",
            t("Other", "Otra").to_string(),
            self.preset.is_none(),
            |this, _, _| this.preset = None,
            cx,
        ));
        let rate_control = v_flex()
            .gap(px(space::S))
            .child(h_flex().gap(px(space::S)).child(chips).when(self.preset.is_none(), |d| {
                d.child(div().w(px(92.)).child(Input::new(&self.custom_rate).suffix(div().text_color(p.muted).child("%"))))
            }))
            .child(self.period_chips(&p, "per", self.interest_period, true, cx));
        let rate_label = {
            let per = self.interest_period.per();
            tr!("Interest rate · {per}", "Tasa de interés · {per}")
        };

        h_flex()
            .items_start()
            .gap(px(space::XL))
            .child(
                v_flex()
                    .w(px(440.))
                    .flex_none()
                    .gap(px(space::M + 2.))
                    .children(error_banner(&p, &self.error))
                    .child(field(
                        &p,
                        t("Client", "Cliente"),
                        Select::new(&self.client)
                            .placeholder(t("Choose a client", "Elige un cliente"))
                            .search_placeholder(t("Search clients", "Buscar clientes")),
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
                        h_flex()
                            .gap(px(space::S))
                            .items_stretch()
                            .children(Method::ALL.map(|m| self.method_tile(&p, m, cx).into_any_element())),
                        None,
                        None,
                    ))
                    .child(field(&p, &rate_label, rate_control, None, rate_err))
                    .child(field(
                        &p,
                        t("Payments due", "Pagos"),
                        self.period_chips(&p, "every", self.frequency, false, cx),
                        installment_loan.then(|| t("How often installments fall due", "Cada cuánto vencen las cuotas").to_string()),
                        None,
                    ))
                    .child(
                        h_flex()
                            .gap(px(space::M))
                            .items_start()
                            .child(div().flex_1().child(field(
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
                                .child(div().child(t(
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
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .self_stretch()
                    .gap(px(space::S))
                    .child(
                        div()
                            .text_size(px(size::META + 1.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(t("Review before saving", "Revisa antes de guardar")),
                    )
                    .child(div().flex_1().child(self.preview(&p, parsed.terms.as_ref(), window, cx)))
                    .child(footer(
                        &p,
                        if self.editing.is_some() { t("Save changes", "Guardar cambios") } else { t("Create loan", "Crear préstamo") },
                        can_submit,
                        false,
                        cx.listener(|this, _, window, cx| this.submit(window, cx)),
                    )),
            )
    }
}
