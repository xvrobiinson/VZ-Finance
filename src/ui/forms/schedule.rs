//! An installment schedule open for editing: due date, capital, interest and
//! total of each installment. Changing the total adjusts the interest;
//! changing capital or interest updates the total.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::value;
use crate::domain::installments::Installment;
use crate::domain::money::{Cents, MoneyFormat, parse_money};
use crate::i18n::{self, t};
use crate::ui::components::th;
use crate::ui::theme::{Palette, RADIUS_CHIP, Typeset as _, palette, space, tabular, text};
use crate::utils::dates::{input_format, parse_input};

/// Emitted whenever a value in the schedule changes.
pub struct ScheduleChanged;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Due,
    Principal,
    Interest,
    Total,
}

struct Row {
    seq: u32,
    due: Entity<InputState>,
    principal: Entity<InputState>,
    interest: Entity<InputState>,
    total: Entity<InputState>,
    /// The row's values as first shown, to mark what was changed.
    original: Installment,
}

pub struct ScheduleEditor {
    rows: Vec<Row>,
    money: MoneyFormat,
    /// Set while this editor writes into its own inputs.
    syncing: bool,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ScheduleChanged> for ScheduleEditor {}

fn date_text(d: chrono::NaiveDate) -> String {
    d.format(input_format(i18n::get())).to_string()
}

impl ScheduleEditor {
    pub fn new(schedule: &[Installment], money: MoneyFormat, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut editor = Self { rows: Vec::new(), money, syncing: false, _subs: Vec::new() };
        editor.load(schedule, window, cx);
        editor
    }

    /// Replaces every row with `schedule`.
    pub fn load(&mut self, schedule: &[Installment], window: &mut Window, cx: &mut Context<Self>) {
        let mut subs = Vec::new();
        let mut rows = Vec::new();
        for x in schedule {
            let input =
                |text: String, window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx).default_value(text));
            let row = Row {
                seq: x.seq,
                due: input(date_text(x.due), window, cx),
                principal: input(MoneyFormat::input_value(x.principal), window, cx),
                interest: input(MoneyFormat::input_value(x.interest), window, cx),
                total: input(MoneyFormat::input_value(x.total()), window, cx),
                original: x.clone(),
            };
            for (cell, state) in
                [(Cell::Due, &row.due), (Cell::Principal, &row.principal), (Cell::Interest, &row.interest), (Cell::Total, &row.total)]
            {
                let i = rows.len();
                subs.push(cx.subscribe_in(state, window, move |this, _, ev: &InputEvent, window, cx| {
                    if let InputEvent::Change = ev
                        && !this.syncing
                    {
                        this.changed(i, cell, window, cx);
                    }
                }));
            }
            rows.push(row);
        }
        self.rows = rows;
        self._subs = subs;
        cx.notify();
    }

    /// Keeps the total and interest of row `i` consistent after `cell` changed.
    fn changed(&mut self, i: usize, cell: Cell, window: &mut Window, cx: &mut Context<Self>) {
        let row = &self.rows[i];
        let p = parse_money(&value(&row.principal, cx)).ok();
        let int = parse_money(&value(&row.interest, cx)).ok();
        let total = parse_money(&value(&row.total, cx)).ok();
        self.syncing = true;
        match cell {
            Cell::Principal | Cell::Interest => {
                if let (Some(p), Some(int)) = (p, int) {
                    let text = MoneyFormat::input_value(p + int);
                    row.total.update(cx, |s, cx| s.set_value(text, window, cx));
                }
            }
            Cell::Total => {
                if let (Some(p), Some(total)) = (p, total)
                    && total >= p
                {
                    let text = MoneyFormat::input_value(total - p);
                    row.interest.update(cx, |s, cx| s.set_value(text, window, cx));
                }
            }
            Cell::Due => {}
        }
        self.syncing = false;
        cx.emit(ScheduleChanged);
        cx.notify();
    }

    /// The schedule as typed, or the first row and column that can't be read.
    pub fn schedule(&self, cx: &App) -> Result<Vec<Installment>, String> {
        let lang = i18n::get();
        self.rows
            .iter()
            .map(|r| {
                let k = r.seq;
                let due = parse_input(&value(&r.due, cx), lang)
                    .ok_or_else(|| crate::tr!("Installment {k}: enter a valid date", "Cuota {k}: escribe una fecha válida"))?;
                let principal = parse_money(&value(&r.principal, cx))
                    .map_err(|e| crate::tr!("Installment {k}, capital: {e}", "Cuota {k}, capital: {e}"))?;
                let interest = parse_money(&value(&r.interest, cx))
                    .map_err(|e| crate::tr!("Installment {k}, interest: {e}", "Cuota {k}, interés: {e}"))?;
                Ok(Installment { seq: k, due, principal, interest })
            })
            .collect()
    }

    /// Puts `amount` more capital (negative: less) on the last installment.
    pub fn adjust_last(&mut self, amount: Cents, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.rows.len().checked_sub(1) else { return };
        let row = &self.rows[i];
        let Ok(p) = parse_money(&value(&row.principal, cx)) else { return };
        let text = MoneyFormat::input_value((p + amount).max(0));
        self.syncing = true;
        row.principal.update(cx, |s, cx| s.set_value(text, window, cx));
        self.syncing = false;
        self.changed(i, Cell::Principal, window, cx);
    }
}

/// Column widths of a schedule table (number, due date, capital, interest,
/// total), shared by the editor and the loan form's read-only schedule so
/// both line up. "Capital left" takes the rest.
pub const COLUMNS: [f32; 5] = [24.0, 112.0, 104.0, 96.0, 104.0];
/// Gap between schedule columns.
pub const COLUMN_GAP: f32 = 6.0;

/// One schedule row: the five fixed `cells` and what is left after it,
/// right-aligned. Money never truncates, so the last column keeps its width.
pub fn schedule_row(cells: [AnyElement; 5], money_right: bool, left: impl IntoElement) -> Div {
    let mut row = h_flex().gap(px(COLUMN_GAP)).font_features(tabular());
    for (i, (cell, w)) in cells.into_iter().zip(COLUMNS).enumerate() {
        row = row.child(div().w(px(w)).flex_none().when(money_right && i >= 2, |d| d.text_right()).child(cell));
    }
    row.child(div().flex_1().whitespace_nowrap().text_right().child(left))
}

/// Header of a schedule table. Amount headings sit over right-aligned
/// figures, or left over inputs.
pub fn schedule_header(p: &Palette, money_right: bool) -> Div {
    schedule_row(
        [
            th(p, "#").into_any_element(),
            th(p, t("Due date", "Vence")).into_any_element(),
            th(p, t("Capital", "Capital")).into_any_element(),
            th(p, t("Interest", "Interés")).into_any_element(),
            th(p, t("Total", "Total")).into_any_element(),
        ],
        money_right,
        th(p, t("Capital left", "Capital restante")),
    )
    .pb(px(space::XS))
}

impl Render for ScheduleEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        let lang = i18n::get();
        let mut balance: Option<Cents> = self.rows.iter().map(|r| parse_money(&value(&r.principal, cx)).ok()).sum();
        let rows = self.rows.iter().map(|r| {
            let due_ok = parse_input(&value(&r.due, cx), lang);
            let principal = parse_money(&value(&r.principal, cx)).ok();
            let interest = parse_money(&value(&r.interest, cx)).ok();
            if let (Some(b), Some(x)) = (balance, principal) {
                balance = Some(b - x);
            }
            let changed =
                due_ok != Some(r.original.due) || principal != Some(r.original.principal) || interest != Some(r.original.interest);
            let cell = |state: &Entity<InputState>| Input::new(state).small().into_any_element();
            schedule_row(
                [
                    div().typeset(text::BODY).text_color(p.faint).child(r.seq.to_string()).into_any_element(),
                    cell(&r.due),
                    cell(&r.principal),
                    cell(&r.interest),
                    cell(&r.total),
                ],
                false,
                div().typeset(text::CAPTION).text_color(p.muted).child(balance.map(|b| self.money.format(b)).unwrap_or_else(|| "—".into())),
            )
            .py(px(3.))
            .when(changed, |d| d.bg(p.accent_tint).rounded(px(RADIUS_CHIP)))
        });
        v_flex()
            .gap(px(2.))
            .child(schedule_header(&p, false))
            .child(div().id("schedule-rows").max_h(px(300.)).overflow_y_scroll().child(v_flex().gap(px(2.)).children(rows)))
            .child(div().pt(px(space::S)).typeset(text::CAPTION).text_color(p.faint).child(t(
                "Highlighted rows differ from the calculated schedule.",
                "Las filas resaltadas difieren del calendario calculado.",
            )))
    }
}
