use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{date_picker, error_banner, field, footer, picked_date, value};
use crate::app::{Route, VzApp};
use crate::domain::loans::{Frequency, Installment, InterestMethod, LoanTerms, MAX_TERM_COUNT, level_payment, schedule_totals};
use crate::domain::money::{Rate, parse_money};
use crate::services::Snapshot;
use crate::ui::components::{icon, num};
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
    rate: Entity<InputState>,
    method: InterestMethod,
    frequency_select: Entity<SelectState<Vec<Choice>>>,
    frequency: Frequency,
    term: Entity<InputState>,
    start: Entity<DatePickerState>,
    notes: Entity<TextareaState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

struct Parsed {
    terms: Option<LoanTerms>,
    principal_err: Option<String>,
    rate_err: Option<String>,
    term_err: Option<String>,
    start_err: Option<String>,
}

impl LoanForm {
    pub fn new(app: WeakEntity<VzApp>, snap: Rc<Snapshot>, client_id: Option<i64>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let choices: Vec<Choice> = snap
            .clients
            .iter()
            .filter(|c| !c.is_archived())
            .map(|c| Choice { id: c.id, title: c.full_name.clone().into() })
            .collect();
        let selected = client_id.and_then(|id| choices.iter().position(|c| c.id == id)).map(IndexPath::new);
        let client = cx.new(|cx| SelectState::new(choices, selected, window, cx).searchable(true));
        let freq_choices: Vec<Choice> =
            Frequency::PERIODIC.iter().enumerate().map(|(i, f)| Choice { id: i as i64, title: f.label().into() }).collect();
        let frequency_select = cx.new(|cx| SelectState::new(freq_choices, Some(IndexPath::new(2)), window, cx));
        let principal = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let rate = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. 12"));
        let term = cx.new(|cx| InputState::new(window, cx).default_value("12"));
        let today = crate::utils::dates::today();
        let start = date_picker(today, window, cx);
        let notes = cx.new(|cx| TextareaState::new(window, cx).rows(2).placeholder("Purpose, collateral, agreement reference"));
        let method = snap.settings.default_method;

        let mut subs = vec![
            cx.subscribe_in(&client, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.client_id = *v;
                cx.notify();
            }),
            cx.subscribe_in(&frequency_select, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                if let Some(i) = v {
                    this.frequency = Frequency::PERIODIC[*i as usize];
                }
                cx.notify();
            }),
            cx.subscribe_in(&start, window, |_, _, _: &DatePickerEvent, _, cx| cx.notify()),
        ];
        for input in [&principal, &rate, &term] {
            subs.push(cx.subscribe_in(input, window, |_, _, ev: &InputEvent, _, cx| {
                if let InputEvent::Change = ev {
                    cx.notify()
                }
            }));
        }
        Self {
            app,
            snap,
            client,
            client_id,
            principal,
            rate,
            method,
            frequency_select,
            frequency: Frequency::Monthly,
            term,
            start,
            notes,
            error: None,
            show_errors: false,
            _subs: subs,
        }
    }

    pub fn first_focus(&self, cx: &App) -> FocusHandle {
        if self.client_id.is_some() { self.principal.focus_handle(cx) } else { self.client.focus_handle(cx) }
    }

    fn frequency(&self) -> Frequency {
        if self.method == InterestMethod::Simple { Frequency::AtMaturity } else { self.frequency }
    }

    fn parse(&self, cx: &App) -> Parsed {
        let principal = parse_money(&value(&self.principal, cx));
        let rate = Rate::from_percent_str(&value(&self.rate, cx));
        let term: Result<u32, _> = value(&self.term, cx).trim().parse::<u32>();
        let start = picked_date(&self.start, cx);
        let principal_err = match &principal {
            Ok(0) => Some("Enter an amount greater than zero".into()),
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        };
        let rate_err = rate.as_ref().err().map(|e| match e {
            crate::domain::money::ParseError::Empty => "Enter the annual rate (0 for interest-free)".to_string(),
            crate::domain::money::ParseError::TooLarge => "Rates above 1000% are not accepted".to_string(),
            e => e.to_string(),
        });
        let term_err = match term {
            Ok(n) if (1..=MAX_TERM_COUNT).contains(&n) => None,
            _ => Some(format!("Enter a whole number from 1 to {MAX_TERM_COUNT}")),
        };
        let today = crate::utils::dates::today();
        let start_err = match start {
            None => Some("Choose the date the money was lent".to_string()),
            Some(d) if d > today => Some("The start date can't be in the future".to_string()),
            _ => None,
        };
        let terms = match (principal, rate, term, start) {
            (Ok(p), Ok(r), Ok(n), Some(s)) if p > 0 && term_err.is_none() => Some(LoanTerms {
                principal: p,
                rate: r,
                method: self.method,
                frequency: self.frequency(),
                term_count: n,
                start_date: s,
            }),
            _ => None,
        };
        Parsed { terms, principal_err, rate_err, term_err, start_err }
    }

    /// Validates and saves; closes the dialog only on success. Bound to the
    /// dialog's Confirm action (Enter) and to the primary button.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_errors = true;
        let parsed = self.parse(cx);
        let (Some(client_id), Some(terms)) = (self.client_id, parsed.terms) else {
            cx.notify();
            return;
        };
        if parsed.start_err.is_some() {
            cx.notify();
            return;
        }
        let notes = self.notes.read(cx).value().to_string();
        let result = self.app.update(cx, |app, cx| {
            let today = app.today;
            match app.store.create_loan(client_id, &terms, &notes, today) {
                Ok(id) => {
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
                    let name = app.snap.client_name(client_id).to_string();
                    let amount = app.settings().money().format(terms.principal);
                    app.notify_success(format!("Loan of {amount} to {name} created"), window, cx);
                    app.navigate(Route::Loan(id), window, cx);
                });
            }
            Ok(Err(msg)) => {
                self.error = Some(msg);
                cx.notify();
            }
            Err(_) => window.close_dialog(cx),
        }
    }

    fn method_tile(&self, p: &Palette, m: InterestMethod, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.method == m;
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
            .child(div().font_weight(FontWeight::MEDIUM).text_color(if selected { p.accent_text } else { p.text }).child(m.label()))
            .child(div().text_size(px(size::META)).line_height(px(16.)).text_color(p.muted).child(m.description()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.method = m;
                cx.notify();
            }))
    }

    fn preview(&self, p: &Palette, terms: Option<&LoanTerms>) -> AnyElement {
        let money = self.snap.settings.money();
        let ds = self.snap.settings.date_style;
        let card = v_flex().p(px(space::XL)).gap(px(space::M)).rounded(px(10.)).bg(p.raised).border_1().border_color(p.rule).h_full();
        let Some(terms) = terms else {
            return card
                .justify_center()
                .items_center()
                .child(icon(IconName::Calculator, p.faint, 20.))
                .child(
                    div()
                        .max_w(px(240.))
                        .text_center()
                        .text_color(p.muted)
                        .child("Enter the amount, rate and term to preview the repayment schedule."),
                )
                .into_any_element();
        };
        let Ok(schedule) = terms.schedule() else {
            return card.child("This combination of terms isn't valid.").into_any_element();
        };
        let (_, interest) = schedule_totals(&schedule);
        let headline: (String, String) = match terms.method {
            InterestMethod::Simple => ("Due at maturity".into(), money.format(schedule[0].total())),
            InterestMethod::Amortized => (
                format!("{} installment", terms.frequency.label()),
                money.format(level_payment(terms.principal, terms.rate, terms.term_count, terms.frequency)),
            ),
            InterestMethod::InterestOnly => (format!("{} interest", terms.frequency.label()), money.format(schedule[0].interest)),
        };
        let rows: Vec<&Installment> = schedule.iter().take(6).collect();
        let more = schedule.len().saturating_sub(rows.len());
        let row_el = |r: &Installment| {
            h_flex()
                .h(px(28.))
                .gap(px(space::S))
                .font_features(tabular())
                .child(div().w(px(22.)).text_color(p.faint).child(r.seq.to_string()))
                .child(div().flex_1().text_color(p.muted).child(ds.format(r.due_date)))
                .child(div().w(px(92.)).text_right().text_color(p.muted).child(money.format(r.interest)))
                .child(div().w(px(104.)).text_right().text_color(p.text).child(money.format(r.total())))
        };
        card.child(
            v_flex()
                .gap(px(2.))
                .child(div().text_size(px(size::META)).text_color(p.muted).child(headline.0))
                .child(num(headline.1).text_size(px(size::FIGURE)).line_height(px(28.)).font_weight(FontWeight::MEDIUM)),
        )
        .child(
            v_flex()
                .gap(px(2.))
                .child(crate::ui::components::kv(p, "Total interest", money.format(interest)))
                .child(crate::ui::components::kv(p, "Total to be repaid", money.format(terms.principal + interest)))
                .child(crate::ui::components::kv(p, "Maturity", ds.format(terms.maturity_date())))
                .child(crate::ui::components::kv(
                    p,
                    "Installments",
                    format!("{} {}", schedule.len(), if schedule.len() == 1 { "payment" } else { "payments" }),
                )),
        )
        .child(
            v_flex()
                .pt(px(space::S))
                .border_t_1()
                .border_color(p.border)
                .child(
                    h_flex()
                        .h(px(26.))
                        .gap(px(space::S))
                        .text_size(px(size::META))
                        .text_color(p.faint)
                        .child(div().w(px(22.)).child("#"))
                        .child(div().flex_1().child("Due"))
                        .child(div().w(px(92.)).text_right().child("Interest"))
                        .child(div().w(px(104.)).text_right().child("Payment")),
                )
                .children(rows.into_iter().map(row_el))
                .when(more > 0, |d| {
                    d.child(div().pt(px(4.)).text_size(px(size::META)).text_color(p.muted).child(format!(
                        "and {more} more, ending {}",
                        ds.format(terms.maturity_date())
                    )))
                }),
        )
        .child(
            div()
                .text_size(px(size::META))
                .line_height(px(16.))
                .text_color(p.faint)
                .child("Interest accrues daily on the outstanding principal (Actual/365). Paying early reduces interest."),
        )
        .into_any_element()
    }
}

impl Render for LoanForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let parsed = self.parse(cx);
        let show = self.show_errors;
        let money = self.snap.settings.money();
        let available = parsed
            .terms
            .as_ref()
            .map(|t| self.snap.book.min_cash_from(t.start_date))
            .unwrap_or_else(|| self.snap.book.cash_on(crate::utils::dates::today()));
        let over_cash = parsed.terms.as_ref().is_some_and(|t| t.principal > available);
        let principal_hint = Some(format!("Available capital: {}", money.format(available.max(0))));
        let principal_err = if over_cash {
            Some(format!("Only {} is available to lend on this date", money.format(available.max(0))))
        } else if show {
            parsed.principal_err.clone()
        } else {
            None
        };
        let client_err = (show && self.client_id.is_none()).then(|| {
            if self.snap.clients.iter().all(|c| c.is_archived()) { "Add a client first".to_string() } else { "Choose a client".to_string() }
        });
        let term_label = if self.method == InterestMethod::Simple { "Term (months)" } else { "Number of payments" };
        let can_submit = self.client_id.is_some() && parsed.terms.is_some() && parsed.start_err.is_none() && !over_cash;

        h_flex()
            .items_start()
            .gap(px(space::XL))
            .child(
                v_flex()
                    .w(px(430.))
                    .flex_none()
                    .gap(px(space::L))
                    .children(error_banner(&p, &self.error))
                    .child(field(&p, "Client", Select::new(&self.client).placeholder("Choose a client").search_placeholder("Search clients"), None, client_err))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(space::M))
                            .child(div().flex_1().child(field(
                                &p,
                                "Principal",
                                Input::new(&self.principal).prefix(div().text_color(p.muted).child(money.symbol.clone())),
                                principal_hint,
                                principal_err,
                            )))
                            .child(div().w(px(150.)).child(field(
                                &p,
                                "Annual rate",
                                Input::new(&self.rate).suffix(div().text_color(p.muted).child("%")),
                                None,
                                if show { parsed.rate_err.clone() } else { None },
                            ))),
                    )
                    .child(field(
                        &p,
                        "Interest method",
                        h_flex()
                            .gap(px(space::S))
                            .items_stretch()
                            .children(InterestMethod::ALL.map(|m| self.method_tile(&p, m, cx).into_any_element())),
                        None,
                        None,
                    ))
                    .child(
                        h_flex()
                            .items_start()
                            .gap(px(space::M))
                            .when(self.method != InterestMethod::Simple, |d| {
                                d.child(div().flex_1().child(field(&p, "Repayment frequency", Select::new(&self.frequency_select), None, None)))
                            })
                            .child(div().w(px(150.)).child(field(
                                &p,
                                term_label,
                                Input::new(&self.term),
                                None,
                                if show { parsed.term_err.clone() } else { None },
                            )))
                            .child(div().flex_1().child(field(&p, "Start date", DatePicker::new(&self.start), None, parsed.start_err.clone()))),
                    )
                    .child(field(&p, "Notes", Textarea::new(&self.notes), None, None)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .self_stretch()
                    .gap(px(space::S))
                    .child(div().text_size(px(size::META + 1.)).font_weight(FontWeight::MEDIUM).child("Repayment preview"))
                    .child(div().flex_1().child(self.preview(&p, parsed.terms.as_ref())))
                    .child(footer(&p, "Create loan", can_submit, false, cx.listener(|this, _, window, cx| this.submit(window, cx)))),
            )
    }
}
