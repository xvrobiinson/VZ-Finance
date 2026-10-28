//! Portfolio accounting: the two cash accounts, portfolio value, returns,
//! history and monthly cash flow. Everything is derived from persisted
//! transactions (capital movements, loans, payments, expenses, transfers and
//! adjustments).
//!
//! Definitions (see `docs/ACCOUNTING.md`):
//! - Cash is held in two accounts. **Available capital** receives capital
//!   added and the capital part of payments; **available interest** receives
//!   interest and fees. A loan is paid from capital first, then interest.
//! - Net capital invested = contributions - withdrawals + capital
//!   adjustments - expenses paid from capital.
//! - Portfolio value = available capital + available interest + outstanding
//!   principal on performing loans + interest earned and not yet collected on
//!   performing loans. Interest counts as earned on its period's due date.
//!   Defaulted balances are excluded (written down) and reported separately.
//! - Portfolio gain = portfolio value - net capital invested.
//! - Return on invested capital = gain / net capital invested (simple, not
//!   annualized).

use std::collections::BTreeMap;

use chrono::NaiveDate;

use crate::domain::installments::{self, Plan, PlanState};
use crate::domain::ledger::{
    ContractStatus, Delinquency, DisplayStatus, LoanFacts, PaymentFact, Position, delinquency, display_status, position,
};
use crate::domain::loans::{Frequency, Method, convert_per_period};
use crate::domain::money::Cents;
use crate::i18n::t;
use crate::utils::dates::{add_months, month_start};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapitalKind {
    Contribution,
    Withdrawal,
}

impl CapitalKind {
    pub fn key(self) -> &'static str {
        match self {
            CapitalKind::Contribution => "contribution",
            CapitalKind::Withdrawal => "withdrawal",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "contribution" => Some(CapitalKind::Contribution),
            "withdrawal" => Some(CapitalKind::Withdrawal),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            CapitalKind::Contribution => t("Capital added", "Capital agregado"),
            CapitalKind::Withdrawal => t("Capital withdrawn", "Capital retirado"),
        }
    }
}

/// The two places cash is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Account {
    Capital,
    Interest,
}

impl Account {
    pub const ALL: [Account; 2] = [Account::Capital, Account::Interest];

    pub fn key(self) -> &'static str {
        match self {
            Account::Capital => "capital",
            Account::Interest => "interest",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "capital" => Some(Account::Capital),
            "interest" => Some(Account::Interest),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Account::Capital => t("Available capital", "Capital disponible"),
            Account::Interest => t("Available interest", "Interés disponible"),
        }
    }
    pub fn other(self) -> Account {
        match self {
            Account::Capital => Account::Interest,
            Account::Interest => Account::Capital,
        }
    }
}

/// Cash movements that are not loans, payments or capital.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Money spent outside the business, paid from one account.
    Expense,
    /// Money received from outside the business (not a loan payment and
    /// not a transfer), into one account. Into available interest it is
    /// income; into available capital it counts as capital invested.
    Income,
    /// Money moved from one account to the other.
    Transfer,
    /// A correction of one account's balance (signed).
    Adjustment,
}

impl EntryKind {
    pub fn key(self) -> &'static str {
        match self {
            EntryKind::Expense => "expense",
            EntryKind::Income => "income",
            EntryKind::Transfer => "transfer",
            EntryKind::Adjustment => "adjustment",
        }
    }
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "expense" => Some(EntryKind::Expense),
            "income" => Some(EntryKind::Income),
            "transfer" => Some(EntryKind::Transfer),
            "adjustment" => Some(EntryKind::Adjustment),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Expense => t("Expense", "Gasto"),
            EntryKind::Income => t("Income", "Ingreso"),
            EntryKind::Transfer => t("Money moved", "Dinero movido"),
            EntryKind::Adjustment => t("Balance adjustment", "Ajuste de saldo"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CapitalFact {
    pub id: i64,
    pub kind: CapitalKind,
    pub amount: Cents,
    pub occurred_on: NaiveDate,
}

#[derive(Clone, Debug)]
pub struct EntryFact {
    pub id: i64,
    pub kind: EntryKind,
    /// Expense: the account that paid. Transfer: the account money left.
    /// Adjustment: the account corrected.
    pub account: Account,
    /// Positive for expenses and transfers; signed for adjustments.
    pub amount: Cents,
    pub occurred_on: NaiveDate,
}

/// One loan with everything needed to evaluate it.
#[derive(Clone, Debug)]
pub struct LoanBook {
    pub id: i64,
    pub client_id: i64,
    pub facts: LoanFacts,
    /// Payment totals (capital, interest, fees) in ledger order.
    pub payments: Vec<PaymentFact>,
    /// The installment schedule and stored splits; `None` for interest-only
    /// loans.
    pub plan: Option<Plan>,
}

impl LoanBook {
    /// The installment loan replayed to the end of `as_of`.
    pub fn plan_state(&self, as_of: NaiveDate) -> Option<PlanState> {
        let plan = self.plan.as_ref()?;
        match installments::run(&self.facts, plan, &plan.payments, as_of, NaiveDate::MAX) {
            Ok(s) => Some(s),
            Err((i, e)) => {
                log::error!("loan {} could not be replayed at payment {i}: {e:?}", self.id);
                None
            }
        }
    }
    pub fn position(&self, as_of: NaiveDate) -> Position {
        if self.plan.is_some() {
            return self.plan_state(as_of).map(|s| s.position()).unwrap_or_default();
        }
        position(&self.facts, &self.payments, as_of)
    }
    pub fn delinquency(&self, today: NaiveDate) -> Delinquency {
        if self.plan.is_some() {
            return self.plan_state(today).map(|s| s.delinquency(self.facts.status, today)).unwrap_or_default();
        }
        delinquency(&self.facts, &self.payments, today)
    }
    /// The interest the loan produces per month at its current capital.
    pub fn monthly_interest(&self, pos: &Position) -> Cents {
        let per_period = match &self.plan {
            Some(p) if p.method == Method::Flat && pos.principal_outstanding > 0 => self.facts.rate.interest(self.facts.principal),
            _ => self.facts.rate.interest(pos.principal_outstanding),
        };
        convert_per_period(per_period, self.facts.frequency, Frequency::Monthly)
    }
    pub fn display_status(&self, today: NaiveDate) -> DisplayStatus {
        display_status(&self.facts, &self.delinquency(today), today)
    }
    /// Status as it was on `as_of`, for history: a loan defaulted later was
    /// still performing before its default date.
    fn performing_on(&self, as_of: NaiveDate) -> bool {
        match (self.facts.status, self.facts.closed_on) {
            (ContractStatus::Defaulted, Some(d)) => as_of < d,
            (ContractStatus::Defaulted, None) => false,
            _ => true,
        }
    }
    /// Interest earned (periods past their due date) and not yet collected.
    pub fn interest_receivable(&self, pos: &Position, as_of: NaiveDate) -> Cents {
        let earned: Cents = pos.periods.iter().filter(|p| p.due <= as_of).map(|p| p.interest).sum();
        (earned - pos.interest_paid).max(0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Book {
    pub loans: Vec<LoanBook>,
    pub capital: Vec<CapitalFact>,
    pub entries: Vec<EntryFact>,
}

/// Cash in each account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Balances {
    pub capital: Cents,
    pub interest: Cents,
}

impl Balances {
    pub fn total(&self) -> Cents {
        self.capital + self.interest
    }
    pub fn get(&self, a: Account) -> Cents {
        match a {
            Account::Capital => self.capital,
            Account::Interest => self.interest,
        }
    }
    fn add(&mut self, a: Account, amount: Cents) {
        match a {
            Account::Capital => self.capital += amount,
            Account::Interest => self.interest += amount,
        }
    }
}

/// The first point at which an account would hold less than nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortfall {
    pub date: NaiveDate,
    /// The account that went below zero, or `None` for a loan larger than
    /// both accounts together.
    pub account: Option<Account>,
    /// What that account (or both together) would hold.
    pub balance: Cents,
}

/// One cash movement, applied in date order and, within a day, money in
/// before money out: capital and adjustments, then payments, transfers,
/// loans, expenses and withdrawals.
#[derive(Clone, Copy, Debug)]
enum Move {
    Add(Account, Cents),
    Transfer(Account, Cents),
    Lend(Cents),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub contributions: Cents,
    pub withdrawals: Cents,
    pub principal_disbursed: Cents,
    pub principal_repaid: Cents,
    pub interest_collected: Cents,
    pub fees_collected: Cents,
    pub capital_expenses: Cents,
    pub interest_expenses: Cents,
    /// Outside income put into available capital (counts as invested).
    pub capital_income: Cents,
    /// Outside income put into available interest (counts as income).
    pub other_income: Cents,
    pub capital_adjustments: Cents,
    pub interest_adjustments: Cents,
    pub available_capital: Cents,
    pub available_interest: Cents,
    /// Both accounts together.
    pub available_cash: Cents,
    /// Outstanding principal on performing (non-defaulted) loans.
    pub outstanding_principal: Cents,
    /// Earned, uncollected interest on performing loans.
    pub interest_receivable: Cents,
    /// Late fees assessed and not yet collected on performing loans.
    pub fees_receivable: Cents,
    pub defaulted_principal: Cents,
    pub defaulted_interest: Cents,
    pub portfolio_value: Cents,
    pub active_loans: usize,
    pub overdue_loans: usize,
    pub due_soon_loans: usize,
    pub overdue_amount: Cents,
    /// Interest active loans produce per month at their current capital.
    pub expected_monthly_interest: Cents,
}

impl Summary {
    pub fn net_invested(&self) -> Cents {
        self.contributions - self.withdrawals + self.capital_adjustments + self.capital_income - self.capital_expenses
    }
    pub fn gain(&self) -> Cents {
        self.portfolio_value - self.net_invested()
    }
    pub fn total_received(&self) -> Cents {
        self.principal_repaid + self.interest_collected + self.fees_collected
    }
    /// Realized income: interest and fees actually received.
    pub fn realized_income(&self) -> Cents {
        self.interest_collected + self.fees_collected
    }
    pub fn expenses(&self) -> Cents {
        self.capital_expenses + self.interest_expenses
    }
    /// Outside income recorded with New record, into either account.
    pub fn outside_income(&self) -> Cents {
        self.capital_income + self.other_income
    }
    /// Interest, fees and outside income counted as income.
    pub fn total_income(&self) -> Cents {
        self.realized_income() + self.other_income
    }
}

impl Book {
    /// Every cash movement in the order it is applied.
    fn moves(&self) -> Vec<(NaiveDate, u8, Move)> {
        let mut out = Vec::new();
        for c in &self.capital {
            match c.kind {
                CapitalKind::Contribution => out.push((c.occurred_on, 0, Move::Add(Account::Capital, c.amount))),
                CapitalKind::Withdrawal => out.push((c.occurred_on, 5, Move::Add(Account::Capital, -c.amount))),
            }
        }
        for e in &self.entries {
            match e.kind {
                EntryKind::Adjustment => out.push((e.occurred_on, 0, Move::Add(e.account, e.amount))),
                EntryKind::Transfer => out.push((e.occurred_on, 2, Move::Transfer(e.account, e.amount))),
                EntryKind::Income => out.push((e.occurred_on, 0, Move::Add(e.account, e.amount))),
                EntryKind::Expense => out.push((e.occurred_on, 4, Move::Add(e.account, -e.amount))),
            }
        }
        for l in &self.loans {
            out.push((l.facts.start_date, 3, Move::Lend(l.facts.principal)));
            for p in &l.payments {
                out.push((p.paid_on, 1, Move::Add(Account::Capital, p.principal)));
                out.push((p.paid_on, 1, Move::Add(Account::Interest, p.interest + p.fee)));
            }
        }
        // Stable: equal keys keep their recorded order.
        out.sort_by_key(|(d, order, _)| (*d, *order));
        out
    }

    /// Replays every movement, calling `visit` with the balances after each.
    /// Stops early when `visit` returns false.
    fn replay_cash(&self, mut visit: impl FnMut(NaiveDate, Move, Balances) -> bool) {
        let mut b = Balances::default();
        for (date, _, m) in self.moves() {
            match m {
                Move::Add(a, amount) => b.add(a, amount),
                Move::Transfer(from, amount) => {
                    b.add(from, -amount);
                    b.add(from.other(), amount);
                }
                Move::Lend(amount) => {
                    let from_capital = amount.min(b.capital.max(0));
                    b.capital -= from_capital;
                    b.interest -= amount - from_capital;
                }
            }
            if !visit(date, m, b) {
                return;
            }
        }
    }

    /// Account balances at the end of `as_of`.
    pub fn balances_on(&self, as_of: NaiveDate) -> Balances {
        let mut last = Balances::default();
        self.replay_cash(|d, _, b| {
            if d > as_of {
                return false;
            }
            last = b;
            true
        });
        last
    }

    /// Cash available at the end of `as_of`, both accounts together.
    pub fn cash_on(&self, as_of: NaiveDate) -> Cents {
        self.balances_on(as_of).total()
    }

    /// The most a new outflow dated `from` could take without leaving
    /// `account` (both accounts together when `None`, as for a loan) below
    /// zero then or at any later point.
    pub fn available_from(&self, from: NaiveDate, account: Option<Account>) -> Cents {
        let pick = |b: &Balances| account.map_or(b.total(), |a| b.get(a));
        let mut low = pick(&self.balances_on(from));
        self.replay_cash(|d, _, b| {
            if d > from {
                low = low.min(pick(&b));
            }
            true
        });
        low
    }

    /// The first point where recorded movements leave an account below zero,
    /// if any. Writes are refused when they would cause one.
    pub fn shortfall(&self) -> Option<Shortfall> {
        let mut found = None;
        self.replay_cash(|date, m, b| {
            if b.capital >= 0 && b.interest >= 0 {
                return true;
            }
            found = Some(match m {
                Move::Lend(_) => Shortfall { date, account: None, balance: b.total() },
                _ if b.capital < 0 => Shortfall { date, account: Some(Account::Capital), balance: b.capital },
                _ => Shortfall { date, account: Some(Account::Interest), balance: b.interest },
            });
            false
        });
        found
    }

    /// The portfolio position at the end of `as_of`, with delinquency judged
    /// against `as_of` as "today".
    pub fn summary(&self, as_of: NaiveDate) -> Summary {
        let mut s = Summary::default();
        for c in self.capital.iter().filter(|c| c.occurred_on <= as_of) {
            match c.kind {
                CapitalKind::Contribution => s.contributions += c.amount,
                CapitalKind::Withdrawal => s.withdrawals += c.amount,
            }
        }
        for e in self.entries.iter().filter(|e| e.occurred_on <= as_of) {
            match (e.kind, e.account) {
                (EntryKind::Expense, Account::Capital) => s.capital_expenses += e.amount,
                (EntryKind::Expense, Account::Interest) => s.interest_expenses += e.amount,
                (EntryKind::Income, Account::Capital) => s.capital_income += e.amount,
                (EntryKind::Income, Account::Interest) => s.other_income += e.amount,
                (EntryKind::Adjustment, Account::Capital) => s.capital_adjustments += e.amount,
                (EntryKind::Adjustment, Account::Interest) => s.interest_adjustments += e.amount,
                (EntryKind::Transfer, _) => {}
            }
        }
        for loan in &self.loans {
            if loan.facts.start_date > as_of {
                continue;
            }
            s.principal_disbursed += loan.facts.principal;
            let pos = loan.position(as_of);
            s.principal_repaid += pos.principal_paid;
            s.interest_collected += pos.interest_paid;
            s.fees_collected += pos.fees_paid;
            if loan.performing_on(as_of) {
                s.outstanding_principal += pos.principal_outstanding;
                s.interest_receivable += loan.interest_receivable(&pos, as_of);
                s.fees_receivable += pos.fees_outstanding;
            } else {
                s.defaulted_principal += pos.principal_outstanding;
                s.defaulted_interest += loan.interest_receivable(&pos, as_of);
            }
            let active = loan.facts.status == ContractStatus::Active && pos.payoff() > 0;
            if active {
                s.active_loans += 1;
                let q = loan.delinquency(as_of);
                s.overdue_amount += q.overdue_amount;
                match display_status(&loan.facts, &q, as_of) {
                    DisplayStatus::Overdue => s.overdue_loans += 1,
                    DisplayStatus::DueSoon => s.due_soon_loans += 1,
                    _ => {}
                }
                s.expected_monthly_interest += loan.monthly_interest(&pos);
            }
        }
        let b = self.balances_on(as_of);
        s.available_capital = b.capital;
        s.available_interest = b.interest;
        s.available_cash = b.total();
        s.portfolio_value = s.available_cash + s.outstanding_principal + s.interest_receivable + s.fees_receivable;
        s
    }

    pub fn first_activity(&self) -> Option<NaiveDate> {
        let dates = self
            .capital
            .iter()
            .map(|c| c.occurred_on)
            .chain(self.loans.iter().map(|l| l.facts.start_date))
            .chain(self.entries.iter().map(|e| e.occurred_on));
        dates.min()
    }

    /// Daily or weekly points between `from` and `to` (inclusive), always
    /// including `to`. Values are end-of-day positions, never interpolated.
    pub fn history(&self, from: NaiveDate, to: NaiveDate) -> Vec<HistoryPoint> {
        if from > to {
            return Vec::new();
        }
        let days = (to - from).num_days();
        let step = if days <= 120 {
            1
        } else if days <= 400 {
            3
        } else {
            7
        };
        let mut points = Vec::new();
        let mut d = from;
        while d < to {
            points.push(self.point(d));
            d += chrono::Duration::days(step);
        }
        points.push(self.point(to));
        points
    }

    fn point(&self, d: NaiveDate) -> HistoryPoint {
        let s = self.summary(d);
        HistoryPoint {
            date: d,
            portfolio_value: s.portfolio_value,
            net_invested: s.net_invested(),
            interest_collected: s.interest_collected,
        }
    }

    /// Cash flow per calendar month from the first activity through `to`.
    /// Capital adjustments count as capital in or out, interest adjustments
    /// as interest; transfers move money between accounts and cancel out.
    pub fn monthly_cash_flow(&self, to: NaiveDate) -> Vec<MonthFlow> {
        let Some(first) = self.first_activity() else {
            return Vec::new();
        };
        let mut months: BTreeMap<NaiveDate, MonthFlow> = BTreeMap::new();
        let mut m = month_start(first);
        while m <= to {
            months.insert(m, MonthFlow { month: m, ..Default::default() });
            m = add_months(m, 1);
        }
        for c in self.capital.iter().filter(|c| c.occurred_on <= to) {
            if let Some(f) = months.get_mut(&month_start(c.occurred_on)) {
                match c.kind {
                    CapitalKind::Contribution => f.contributions += c.amount,
                    CapitalKind::Withdrawal => f.withdrawals += c.amount,
                }
            }
        }
        for e in self.entries.iter().filter(|e| e.occurred_on <= to) {
            if let Some(f) = months.get_mut(&month_start(e.occurred_on)) {
                match (e.kind, e.account) {
                    (EntryKind::Expense, _) => f.expenses += e.amount,
                    (EntryKind::Income, _) => f.income += e.amount,
                    (EntryKind::Adjustment, Account::Capital) if e.amount > 0 => f.contributions += e.amount,
                    (EntryKind::Adjustment, Account::Capital) => f.withdrawals -= e.amount,
                    (EntryKind::Adjustment, Account::Interest) => f.interest_received += e.amount,
                    (EntryKind::Transfer, _) => {}
                }
            }
        }
        for l in self.loans.iter().filter(|l| l.facts.start_date <= to) {
            if let Some(f) = months.get_mut(&month_start(l.facts.start_date)) {
                f.disbursed += l.facts.principal;
            }
            for p in l.payments.iter().filter(|p| p.paid_on <= to) {
                if let Some(f) = months.get_mut(&month_start(p.paid_on)) {
                    f.principal_returned += p.principal;
                    f.interest_received += p.interest;
                    f.fees_received += p.fee;
                }
            }
        }
        let mut cash = 0;
        months
            .into_values()
            .map(|mut f| {
                cash += f.net();
                f.ending_cash = cash;
                f
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryPoint {
    pub date: NaiveDate,
    pub portfolio_value: Cents,
    pub net_invested: Cents,
    pub interest_collected: Cents,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MonthFlow {
    pub month: NaiveDate,
    pub contributions: Cents,
    pub withdrawals: Cents,
    pub disbursed: Cents,
    pub principal_returned: Cents,
    pub interest_received: Cents,
    pub fees_received: Cents,
    pub expenses: Cents,
    /// Outside income, into either account.
    pub income: Cents,
    pub ending_cash: Cents,
}

impl MonthFlow {
    pub fn net(&self) -> Cents {
        self.contributions - self.withdrawals - self.disbursed
            + self.principal_returned
            + self.interest_received
            + self.fees_received
            + self.income
            - self.expenses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ledger::allocate;
    use crate::domain::loans::PeriodRate;
    use crate::domain::money::Rate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn pay(l: &mut LoanBook, amount: Cents, on: NaiveDate) {
        let a = allocate(&l.facts, &l.payments, amount, 0, on, d(2030, 1, 1)).unwrap();
        l.payments.push(PaymentFact { paid_on: on, principal: a.principal, interest: a.interest, fee: 0 });
        if a.pays_off() {
            l.facts.status = ContractStatus::PaidOff;
        }
    }

    fn loan(id: i64, principal: Cents, start: NaiveDate) -> LoanBook {
        LoanBook {
            id,
            client_id: 1,
            facts: LoanFacts {
                principal,
                rate: PeriodRate::new(Rate(100_000), Frequency::Monthly, Frequency::Monthly),
                frequency: Frequency::Monthly,
                start_date: start,
                status: ContractStatus::Active,
                closed_on: None,
            },
            payments: Vec::new(),
            plan: None,
        }
    }

    fn capital(id: i64, kind: CapitalKind, amount: Cents, on: NaiveDate) -> CapitalFact {
        CapitalFact { id, kind, amount, occurred_on: on }
    }

    fn entry(id: i64, kind: EntryKind, account: Account, amount: Cents, on: NaiveDate) -> EntryFact {
        EntryFact { id, kind, account, amount, occurred_on: on }
    }

    /// A fixture with known results:
    /// - Jan 1: add 20,000.00 of capital; lend 10,000.00 at 10% a month
    /// - Feb 1: client pays 3,000.00 = 1,000.00 interest + 2,000.00 capital
    /// - Mar 1: client pays 800.00 = February's interest on 8,000.00
    /// - Apr 15: withdraw 1,000.00 (April's 800.00 is left unpaid)
    fn fixture() -> Book {
        let mut l = loan(1, 1_000_000, d(2026, 1, 1));
        pay(&mut l, 300_000, d(2026, 2, 1));
        pay(&mut l, 80_000, d(2026, 3, 1));
        Book {
            loans: vec![l],
            capital: vec![
                capital(1, CapitalKind::Contribution, 2_000_000, d(2026, 1, 1)),
                capital(2, CapitalKind::Withdrawal, 100_000, d(2026, 4, 15)),
            ],
            entries: Vec::new(),
        }
    }

    #[test]
    fn lending_is_not_a_loss_and_capital_back_is_not_income() {
        let book = fixture();
        let s = book.summary(d(2026, 1, 1));
        assert_eq!(s.available_cash, 1_000_000);
        assert_eq!(s.outstanding_principal, 1_000_000);
        // January's interest is earned on its due date, not on the day lent.
        assert_eq!(s.interest_receivable, 0);
        assert_eq!(s.portfolio_value, 2_000_000);
        assert_eq!(s.gain(), 0);

        let s = book.summary(d(2026, 2, 1));
        assert_eq!(s.interest_collected, 100_000);
        assert_eq!(s.principal_repaid, 200_000);
        assert_eq!(s.outstanding_principal, 800_000);
        assert_eq!(s.realized_income(), 100_000);
        assert_eq!(s.gain(), 100_000);
    }

    #[test]
    fn payments_fill_the_two_accounts() {
        let book = fixture();
        let s = book.summary(d(2026, 3, 1));
        // 20,000 - 10,000 lent + 2,000 capital back; 1,000 + 800 interest.
        assert_eq!((s.available_capital, s.available_interest), (1_200_000, 180_000));
        // The withdrawal comes out of capital.
        let s = book.summary(d(2026, 4, 15));
        assert_eq!((s.available_capital, s.available_interest), (1_100_000, 180_000));
    }

    #[test]
    fn loans_use_capital_first_then_interest() {
        let mut book = fixture();
        // Mar 2: lend 12,500 with 12,000 capital and 1,800 interest available.
        book.loans.push(loan(2, 1_250_000, d(2026, 3, 2)));
        let b = book.balances_on(d(2026, 3, 2));
        assert_eq!((b.capital, b.interest), (0, 130_000));
        // Repaid capital always returns to capital.
        pay(&mut book.loans[1], 1_250_000 + 125_000, d(2026, 4, 2));
        let b = book.balances_on(d(2026, 4, 2));
        assert_eq!((b.capital, b.interest), (1_250_000, 255_000));
    }

    #[test]
    fn expenses_and_transfers_move_only_their_account() {
        let mut book = fixture();
        book.entries.push(entry(1, EntryKind::Expense, Account::Interest, 50_000, d(2026, 3, 5)));
        book.entries.push(entry(2, EntryKind::Transfer, Account::Interest, 100_000, d(2026, 3, 6)));
        book.entries.push(entry(3, EntryKind::Expense, Account::Capital, 20_000, d(2026, 3, 7)));
        let before = book.summary(d(2026, 3, 1));
        let s = book.summary(d(2026, 3, 7));
        assert_eq!(s.available_interest, 180_000 - 50_000 - 100_000);
        assert_eq!(s.available_capital, 1_200_000 + 100_000 - 20_000);
        assert_eq!(s.interest_collected, before.interest_collected, "interest earned is unchanged");
        // An expense from capital lowers invested capital, not the gain.
        assert_eq!(s.net_invested(), before.net_invested() - 20_000);
        assert_eq!(s.gain(), before.gain() - 50_000);
    }

    #[test]
    fn adjustments_correct_an_account() {
        let mut book = fixture();
        book.entries.push(entry(1, EntryKind::Adjustment, Account::Capital, -30_000, d(2026, 3, 10)));
        book.entries.push(entry(2, EntryKind::Adjustment, Account::Interest, 5_000, d(2026, 3, 10)));
        let s = book.summary(d(2026, 3, 10));
        assert_eq!((s.available_capital, s.available_interest), (1_170_000, 185_000));
        assert_eq!(s.net_invested(), 2_000_000 - 30_000);
    }

    #[test]
    fn shortfall_finds_the_first_negative_account() {
        let mut book = fixture();
        assert_eq!(book.shortfall(), None);
        book.entries.push(entry(1, EntryKind::Expense, Account::Interest, 200_000, d(2026, 3, 5)));
        assert_eq!(book.shortfall(), Some(Shortfall { date: d(2026, 3, 5), account: Some(Account::Interest), balance: -20_000 }));
        book.entries.clear();
        book.loans.push(loan(2, 2_000_000, d(2026, 3, 2)));
        assert_eq!(book.shortfall(), Some(Shortfall { date: d(2026, 3, 2), account: None, balance: 1_380_000 - 2_000_000 }));
    }

    #[test]
    fn availability_looks_ahead() {
        let book = fixture();
        assert_eq!(book.available_from(d(2026, 1, 1), None), 1_000_000);
        assert_eq!(book.available_from(d(2026, 3, 15), None), 1_280_000);
        assert_eq!(book.available_from(d(2026, 3, 15), Some(Account::Capital)), 1_100_000);
        assert_eq!(book.available_from(d(2026, 3, 15), Some(Account::Interest)), 180_000);
    }

    #[test]
    fn unpaid_interest_is_receivable_once_due() {
        let book = fixture();
        assert_eq!(book.summary(d(2026, 3, 31)).interest_receivable, 0);
        let s = book.summary(d(2026, 4, 1));
        assert_eq!(s.interest_receivable, 80_000);
        assert_eq!(s.gain(), 100_000 + 80_000 + 80_000);
        assert_eq!(s.expected_monthly_interest, 80_000);
    }

    #[test]
    fn withdrawal_is_not_a_loss() {
        let book = fixture();
        let before = book.summary(d(2026, 4, 14));
        let after = book.summary(d(2026, 4, 15));
        assert_eq!(after.net_invested(), 1_900_000);
        assert_eq!(after.gain(), before.gain());
    }

    #[test]
    fn ledger_invariants_hold_every_day() {
        let mut book = fixture();
        book.entries.push(entry(1, EntryKind::Expense, Account::Interest, 30_000, d(2026, 3, 5)));
        book.entries.push(entry(2, EntryKind::Transfer, Account::Interest, 50_000, d(2026, 3, 9)));
        book.entries.push(entry(3, EntryKind::Adjustment, Account::Capital, 7_000, d(2026, 5, 1)));
        book.entries.push(entry(4, EntryKind::Income, Account::Interest, 12_000, d(2026, 6, 2)));
        book.entries.push(entry(5, EntryKind::Income, Account::Capital, 40_000, d(2026, 7, 3)));
        let mut day = d(2025, 12, 30);
        while day <= d(2026, 12, 31) {
            let s = book.summary(day);
            assert_eq!(
                s.available_cash + s.outstanding_principal + s.defaulted_principal,
                s.net_invested() + s.interest_collected + s.fees_collected + s.interest_adjustments + s.other_income - s.interest_expenses,
                "{day}"
            );
            assert_eq!(
                s.gain(),
                s.total_income() + s.interest_adjustments - s.interest_expenses + s.interest_receivable - s.defaulted_principal,
                "{day}"
            );
            assert_eq!(book.cash_on(day), s.available_cash, "{day}");
            day += chrono::Duration::days(1);
        }
    }

    #[test]
    fn default_writes_down_the_balance() {
        let mut book = fixture();
        book.loans[0].facts.status = ContractStatus::Defaulted;
        book.loans[0].facts.closed_on = Some(d(2026, 5, 10));
        assert_eq!(book.summary(d(2026, 5, 9)).defaulted_principal, 0);
        let s = book.summary(d(2026, 6, 1));
        assert_eq!(s.outstanding_principal, 0);
        assert_eq!(s.defaulted_principal, 800_000);
        assert_eq!(s.active_loans, 0);
        assert_eq!(s.portfolio_value, s.available_cash);
    }

    #[test]
    fn overdue_totals_roll_up() {
        let book = fixture();
        let s = book.summary(d(2026, 4, 10));
        assert_eq!(s.overdue_loans, 1);
        assert_eq!(s.overdue_amount, 80_000);
    }

    #[test]
    fn history_uses_real_end_of_day_values() {
        let book = fixture();
        let h = book.history(d(2026, 1, 1), d(2026, 3, 1));
        assert_eq!(h.first().unwrap().date, d(2026, 1, 1));
        assert_eq!(h.last().unwrap().date, d(2026, 3, 1));
        assert_eq!(h.len(), 60);
        for p in &h {
            assert_eq!(p.portfolio_value, book.summary(p.date).portfolio_value);
        }
        assert!(h.windows(2).all(|w| w[1].portfolio_value >= w[0].portfolio_value));
    }

    #[test]
    fn monthly_cash_flow_reconciles_to_cash() {
        let mut book = fixture();
        book.entries.push(entry(1, EntryKind::Expense, Account::Interest, 30_000, d(2026, 4, 3)));
        let flows = book.monthly_cash_flow(d(2026, 4, 20));
        assert_eq!(flows.len(), 4);
        assert_eq!(flows[0].contributions, 2_000_000);
        assert_eq!(flows[0].disbursed, 1_000_000);
        assert_eq!((flows[1].interest_received, flows[1].principal_returned), (100_000, 200_000));
        assert_eq!(flows[2].interest_received, 80_000);
        assert_eq!((flows[3].withdrawals, flows[3].expenses), (100_000, 30_000));
        assert_eq!(flows.last().unwrap().ending_cash, book.cash_on(d(2026, 4, 20)));
    }
}
