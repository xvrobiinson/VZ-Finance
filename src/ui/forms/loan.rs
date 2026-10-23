use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::date_field::{DateChanged, DateField};
use super::{date_field, error_banner, field, footer, invalid_date, picked_date, value};
use crate::app::{Route, VzApp};
use crate::domain::loans::{Frequency, LoanTerms, MAX_PERIOD_RATE, RATE_PRESETS, period_interest};
use crate::domain::money::{ParseError, Rate, parse_money};
use crate::i18n::t;
use crate::services::Snapshot;
use crate::tr;
use crate::ui::components::{icon, kv, num};
use crate::ui::theme::{Palette, palette, size, space};

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
    /// A preset rate, or `None` for a custom rate typed in `custom_rate`.
    preset: Option<Rate>,
    custom_rate: Entity<InputState>,
    frequency: Frequency,
    start: Entity<DateField>,
    notes: Entity<TextareaState>,
    error: Option<String>,
    show_errors: bool,
    _subs: Vec<Subscription>,
}

struct Parsed {
    terms: Option<LoanTerms>,
    principal_err: Option<String>,
    rate_err: Option<String>,
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
        let principal = cx.new(|cx| InputState::new(window, cx).placeholder("0.00"));
        let default_rate = snap.settings.default_rate;
        let preset = RATE_PRESETS.contains(&default_rate).then_some(default_rate);
        let custom_rate = cx.new(|cx| {
            let s = InputState::new(window, cx).placeholder(t("e.g. 12", "ej. 12"));
            if preset.is_none() { s.default_value(default_rate.percent_label().trim_end_matches('%').to_string()) } else { s }
        });
        let start = date_field(crate::utils::dates::today(), window, cx);
        let notes = cx.new(|cx| TextareaState::new(window, cx).rows(2).placeholder(t("Purpose, collateral, agreement", "Motivo, garantía, acuerdo")));
        let frequency = snap.settings.default_frequency;

        let mut subs = vec![
            cx.subscribe_in(&client, window, |this, _, ev: &SelectEvent<Vec<Choice>>, _, cx| {
                let SelectEvent::Confirm(v) = ev;
                this.client_id = *v;
                cx.notify();
            }),
            cx.subscribe(&start, |_, _, _: &DateChanged, cx| cx.notify()),
        ];
        for input in [&principal, &custom_rate] {
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
            preset,
            custom_rate,
            frequency,
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
            Some(d) if d > today => Some(t("The loan date can't be in the future", "La fecha del préstamo no puede ser futura").to_string()),
            _ => None,
        };
        let terms = match (principal, &rate, start) {
            (Ok(p), Ok(r), Some(s)) if p > 0 => Some(LoanTerms { principal: p, rate: *r, frequency: self.frequency, start_date: s }),
            _ => None,
        };
        Parsed { terms, principal_err, rate_err: rate.err(), start_err }
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
                    app.notify_success(tr!("Loan of {amount} to {name} created", "Préstamo de {amount} a {name} creado"), window, cx);
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

    fn chip(&self, p: &Palette, id: &'static str, label: String, selected: bool, on: impl Fn(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id(id)
            .h(px(34.))
            .px(px(14.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(if selected { p.accent } else { p.control })
            .when(selected, |d| d.bg(p.accent_tint).text_color(p.accent_text).font_weight(FontWeight::MEDIUM))
            .when(!selected, |d| d.hover(|s| s.bg(p.hover)))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| {
                on(this, cx);
                cx.notify();
            }))
    }

    fn frequency_tile(&self, p: &Palette, f: Frequency, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.frequency == f;
        let desc = match f {
            Frequency::Monthly => t("Interest once a month", "Interés una vez al mes"),
            Frequency::Biweekly => t("Interest twice a month (quincenal)", "Interés dos veces al mes"),
        };
        v_flex()
            .id(f.key())
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
            .child(div().font_weight(FontWeight::MEDIUM).text_color(if selected { p.accent_text } else { p.text }).child(f.label()))
            .child(div().text_size(px(size::META)).text_color(p.muted).child(desc))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.frequency = f;
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
                .child(div().max_w(px(260.)).text_center().text_color(p.muted).child(t(
                    "Enter the amount and choose a rate to see the interest per period.",
                    "Escribe el monto y elige la tasa para ver el interés por período.",
                )))
                .into_any_element();
        };
        let interest = terms.interest_per_period();
        let half = terms.principal / 2;
        let example_pay = interest + half;
        let next_interest = period_interest(terms.principal - half, terms.rate);
        let period_label = match terms.frequency {
            Frequency::Monthly => t("Interest each month", "Interés cada mes"),
            Frequency::Biweekly => t("Interest every 15 days", "Interés cada 15 días"),
        };
        let rate = terms.rate.percent_label();
        let principal = money.format(terms.principal);
        let (i, h, pay, next) = (money.format(interest), money.format(half), money.format(example_pay), money.format(next_interest));
        let bullet = |text: String| {
            h_flex()
                .items_start()
                .gap(px(8.))
                .child(div().mt(px(7.)).size(px(5.)).rounded_full().bg(p.accent).flex_none())
                .child(div().text_color(p.text).child(text))
        };
        card.child(
            v_flex()
                .gap(px(2.))
                .child(div().text_size(px(size::META)).text_color(p.muted).child(period_label))
                .child(num(i.clone()).text_size(px(size::FIGURE + 2.)).line_height(px(30.)).font_weight(FontWeight::MEDIUM))
                .child(div().text_size(px(size::META)).text_color(p.muted).child(tr!("{rate} of {principal}", "{rate} de {principal}"))),
        )
        .child(
            v_flex()
                .child(kv(p, t("First payment due", "Primer pago"), ds.format(terms.first_due())))
                .child(kv(p, t("Loan date", "Fecha del préstamo"), ds.format(terms.start_date))),
        )
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
        let avail = money.format(available.max(0));
        let principal_hint = Some(tr!("Available capital: {avail}", "Capital disponible: {avail}"));
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
        let can_submit = self.client_id.is_some() && parsed.terms.is_some() && parsed.start_err.is_none() && !over_cash;
        // A typed custom rate is checked as you type; an empty one only on submit.
        let typed = self.preset.is_none() && !value(&self.custom_rate, cx).trim().is_empty();
        let rate_err = parsed.rate_err.clone().filter(|_| show || typed);

        let mut chips = h_flex().gap(px(space::S)).flex_wrap();
        for (i, r) in RATE_PRESETS.iter().enumerate() {
            let r = *r;
            let id: &'static str = ["rate-5", "rate-10", "rate-15"][i];
            chips = chips.child(self.chip(&p, id, r.percent_label(), self.preset == Some(r), move |this, _| this.preset = Some(r), cx));
        }
        chips = chips.child(self.chip(&p, "rate-other", t("Other", "Otra").to_string(), self.preset.is_none(), |this, _| this.preset = None, cx));
        let rate_control = v_flex()
            .gap(px(space::S))
            .child(chips)
            .when(self.preset.is_none(), |d| d.child(div().w(px(150.)).child(Input::new(&self.custom_rate).suffix(div().text_color(p.muted).child("%")))));

        h_flex()
            .items_start()
            .gap(px(space::XL))
            .child(
                v_flex()
                    .w(px(430.))
                    .flex_none()
                    .gap(px(space::L))
                    .children(error_banner(&p, &self.error))
                    .child(field(
                        &p,
                        t("Client", "Cliente"),
                        Select::new(&self.client).placeholder(t("Choose a client", "Elige un cliente")).search_placeholder(t("Search clients", "Buscar clientes")),
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
                        t("Interest rate per period", "Tasa de interés por período"),
                        rate_control,
                        None,
                        rate_err,
                    ))
                    .child(field(
                        &p,
                        t("Payments", "Pagos"),
                        h_flex().gap(px(space::S)).items_stretch().children(Frequency::ALL.map(|f| self.frequency_tile(&p, f, cx).into_any_element())),
                        None,
                        None,
                    ))
                    .child(field(&p, t("Loan date", "Fecha del préstamo"), self.start.clone(), None, parsed.start_err.clone().filter(|_| show)))
                    .child(field(&p, t("Notes", "Notas"), Textarea::new(&self.notes), None, None)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .self_stretch()
                    .gap(px(space::S))
                    .child(div().text_size(px(size::META + 1.)).font_weight(FontWeight::MEDIUM).child(t("Preview", "Vista previa")))
                    .child(div().flex_1().child(self.preview(&p, parsed.terms.as_ref())))
                    .child(footer(&p, t("Create loan", "Crear préstamo"), can_submit, false, cx.listener(|this, _, window, cx| this.submit(window, cx)))),
            )
    }
}
