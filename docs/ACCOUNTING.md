# VZ Finance accounting rules

These are the rules the calculation engine follows. They are implemented in
`src/domain/` and covered by unit tests with hand-checked figures.

## Units and rounding

- **Money** is stored and computed as whole minor units (cents) in 64-bit
  integers. No floating point is used for balances, interest or allocations.
- **Rates** are annual rates stored in parts per million (12.5% = `125000`),
  entered with up to four decimals of a percent.
- **Rounding** is half-up (half away from zero) to the cent, applied once per
  accrual segment (see below). Displayed totals are sums of stored cents.
- One **base currency** per database. Changing it changes the symbol only;
  amounts are never converted.

## Interest accrual

- **Simple, daily, Actual/365 Fixed.** Interest for a period is
  `principal × rate × days / 365`, on the outstanding principal only.
- **No compounding.** Unpaid interest stays receivable and does not earn
  interest.
- An **accrual segment** runs from the previous event (loan start or a payment)
  to the next. Each segment is rounded to the cent when it closes.
- Interest **keeps accruing after maturity** at the contract rate until the
  loan is paid off.
- Interest **stops on the default date** of a defaulted loan. Reinstating the
  loan resumes accrual as if it had not defaulted.

## Loan methods

The method is fixed when the loan is created and is never mixed.

| Method | Schedule |
| --- | --- |
| Simple interest | One payment at maturity: principal plus interest for the full term. Term in months. |
| Amortized | Level installments (weekly, every 2 weeks, monthly or quarterly). |
| Interest-only | Each installment is that period's interest; the last also repays the principal. |

**Schedules are contractual projections**: what is due on each date if every
installment is paid on its due date. Installment interest uses the same daily
Actual/365 rule as the accrual engine, so a borrower who pays exactly on
schedule has allocations that match the schedule to the cent.

- **Amortized level payment** is the standard annuity amount at the periodic
  rate (annual rate ÷ periods per year), rounded to the cent. This is the only
  place a floating-point value is used, and only to choose that fixed contract
  amount. The **final installment** repays the remaining principal exactly,
  absorbing rounding and differences in month length.
- **Due dates** are counted from the start date (`start + k months`, or
  `start + 7k / 14k days`), so month-end dates never drift; Jan 31 + 1 month
  is the last day of February.

## Payments

- **Allocation order:** fee (entered explicitly with the payment), then
  accrued interest, then principal.
- **Partial payments** pay interest first; anything left reduces principal.
- **Early and extra payments** reduce principal immediately, which lowers
  future interest. The contractual schedule is not rewritten.
- **Overpayments** above the payoff amount on the payment date are refused,
  with the payoff shown, so no unexplained credit balance is ever created.
- Payments must be dated on or after the loan start, on or after the latest
  recorded payment for that loan, and not in the future.
- A payment that brings principal and accrued interest to zero marks the loan
  **Paid off** in the same database transaction.
- **Corrections:** a payment is never edited or deleted. It is *reversed*: the
  row stays in the history with the reversal time and reason and is excluded
  from every calculation. Only the latest live payment on a loan can be
  reversed, because each allocation depends on the ones before it. A reversal
  that would make available cash negative (the money was already lent or
  withdrawn) is refused.

## Delinquency

- Payments (excluding fees) cover installments **oldest first** by cumulative
  scheduled amount.
- An installment is **overdue the day after its due date** if not fully covered.
  Days overdue are counted from the oldest uncovered installment.
- **Overdue amount** = scheduled amounts due before today − amounts paid,
  capped at the payoff. After maturity, everything still owed is overdue.
- **Due soon** = the next uncovered installment falls within 7 days.

## Loan status

The contractual status is stored (`active`, `paid_off`, `defaulted`,
`restructured`) with a dated history in `loan_status_events`. *Due soon* and
*Overdue* are calculated on the fly and never stored.

## Capital and portfolio

| Figure | Definition |
| --- | --- |
| Net capital invested | Contributions − withdrawals |
| Available cash | Net capital invested − principal disbursed + all payments received |
| Outstanding principal | Principal still owed on performing (non-defaulted) loans |
| Interest receivable | Accrued, unpaid interest on performing loans |
| **Portfolio value** | Available cash + outstanding principal + interest receivable |
| Defaulted balance | Principal and interest owed on defaulted loans; excluded from value |
| Portfolio gain | Portfolio value − net capital invested |
| Realized income | Interest received + fees received |
| Return on invested capital | Gain ÷ net capital invested (simple, not annualized) |
| Expected interest remaining | Scheduled interest on active loans not yet received (projection) |

Consequences, all tested:

- Lending money is **not a loss**: cash falls and outstanding principal rises by
  the same amount.
- A principal repayment is **not income**: it moves value from principal back
  to cash.
- Capital contributions and withdrawals are **not profit or loss**.
- Ledger invariant, checked day by day:
  `cash + all principal still owed = net capital invested + interest received + fees received`.
- Gain identity: `gain = realized income + interest receivable − defaulted principal`.

A loan or withdrawal larger than the cash available on its date, and on every
later date in the ledger, is refused.

## History and charts

Chart points are end-of-day portfolio values recomputed from the ledger for
each date (daily up to 120 days, every 3 days up to ~13 months, weekly beyond).
Lines are drawn as straight segments between real values, with no smoothing.

## Storage

- SQLite at `%LOCALAPPDATA%\VZFinance\data\portfolio.db`, WAL mode, foreign
  keys enforced, CHECK constraints on every amount, rate, date and enum.
- A trigger rejects any payment allocation that does not add up to the payment
  amount; another blocks edits to recorded payments and allocations.
- Every write runs inside a single `IMMEDIATE` transaction.
- Migrations are versioned in `schema_migrations`. A database from a newer app
  version is refused without being changed.
- Backups use SQLite's online backup API and are verified (integrity check,
  required tables, schema version, foreign keys) before a restore. A restore
  first saves a safety copy of the current data.
- The database is **not encrypted** by the app. Use BitLocker or device
  encryption to protect data at rest.
