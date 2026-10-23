# VZ Finance accounting rules

These are the rules the calculation engine follows. They are implemented in
`src/domain/` and covered by unit tests with hand-checked figures.

## Units and rounding

- **Money** is stored and computed as whole minor units (cents) in 64-bit
  integers. No floating point is used anywhere in the money arithmetic.
- **Rates** are percentages **per period** (5%, 10%, 15% or any value up to
  100%), stored in parts per million (10% = `100000`), entered with up to four
  decimals.
- **Rounding** is half-up to the cent, once per period's interest.
- One **base currency** per database. Changing it changes the symbol only;
  amounts are never converted.

## Loans

A loan is an amount lent on a date at a fixed rate per period, paid either
**monthly** or **every 15 days** (quincenal). Loans have no fixed term: they
stay open until the capital is repaid.

- Monthly periods end on the same day of each month as the loan date
  (Jan 31 → Feb 28 → Mar 31: always counted from the loan date, never drifting).
- Every-15-days periods end 15 days after the loan day and on the loan day of
  each following month (Jan 5 → Jan 20 → Feb 5 → Feb 20 …).

## Interest

- **Each period's interest = rate × capital owed when the period begins.**
- The interest is owed from the start of the period and is **due on the
  period's end date**.
- A payment dated exactly on a due date belongs to the period ending that day;
  the next period's interest is computed after it.
- **No compounding:** unpaid interest stays owed but never earns interest.
- No interest is charged once the capital reaches zero, or for periods that
  start after a loan is marked as defaulted.

Example: 1,000 at 10% a month. The first month's interest is 100. If the client
pays 600 on the due date, 100 covers the interest and 500 reduces the capital to
500, so the next month's interest is 50. Paying only 50 then keeps the capital at
500.

## Payments

- **Order:** fee (only if entered with the payment), then interest owed (oldest
  period first), then capital.
- A payment can be any amount. Paying more than the interest always reduces the
  capital, which lowers the next period's interest.
- Paying early inside a period still covers that period's interest first.
- Amounts above what is owed (capital plus interest owed) are refused, with the
  total shown, so no unexplained credit is ever created.
- Payments must be dated on or after the loan date, on or after the latest
  recorded payment for that loan, and not in the future.
- A payment that brings capital and interest owed to zero closes the loan
  (**Paid off**) in the same database transaction.
- **Corrections:** a payment is never edited or deleted. It is *reversed*: the
  row stays in the history with the time and reason and is excluded from every
  calculation. Only the latest payment on a loan can be reversed, because each
  split depends on the ones before it.

## Receipts

Every payment has a printable receipt (Payments, or the loan page, or right
after recording a payment). It shows the amount paid, its split between
interest and capital, the capital before and after, interest still pending,
the total owed, the percentage of the original capital paid and remaining, the
share of what was owed that the payment covered, and the next payment. Receipts
are saved as HTML files in `%LOCALAPPDATA%\VZFinance\receipts` and open in the
default browser, which shows the print dialog.

## Overdue and due soon

- Interest paid covers periods oldest first.
- A period is **overdue** the day after its due date if its interest is not
  fully covered. Days overdue count from the oldest unpaid due date.
- **Next payment** = the next due date (today or later) and everything that must
  be paid by then to be current, including any overdue interest.
- **Due soon** = the next payment is due within 5 days.

## Loan status

The contractual status is stored (`active`, `paid_off`, `defaulted`,
`restructured`) with a dated history in `loan_status_events`. *Due soon* and
*Overdue* are calculated and never stored.

## Capital and portfolio

| Figure | Definition |
| --- | --- |
| Capital invested | Contributions − withdrawals |
| Available cash | Capital invested − capital lent + every payment received |
| Capital lent out | Capital still owed on performing (non-defaulted) loans |
| Interest due, unpaid | Interest of periods past their due date, not yet collected |
| **Portfolio value** | Available cash + capital lent out + interest due, unpaid |
| Defaulted balance | What defaulted loans still owe; excluded from value |
| Portfolio gain | Portfolio value − capital invested |
| Income received | Interest + fees collected |
| Return on invested capital | Gain ÷ capital invested (total, not annualized) |
| Expected interest per month | Rate × capital owed on active loans (twice for every-15-days loans) |

Interest counts toward portfolio value on its due date, not on the day it
starts, so lending money never shows an instant gain.

Consequences, all tested:

- Lending money is **not a loss**: cash falls and capital lent out rises by the
  same amount.
- A capital repayment is **not income**: it moves value from capital lent back
  to cash.
- Capital contributions and withdrawals are **not profit or loss**.
- Ledger invariant, checked day by day:
  `cash + all capital still owed = capital invested + interest received + fees received`.

A loan or withdrawal larger than the cash available on its date, and on every
later date, is refused.

## History and charts

Chart points are end-of-day portfolio values recomputed from the ledger for
each date (daily up to 120 days, every 3 days up to about 13 months, weekly
beyond), drawn as straight segments between real values.

## Storage

- SQLite at `%LOCALAPPDATA%\VZFinance\data\portfolio.db`, WAL mode, foreign
  keys enforced, CHECK constraints on every amount, rate, date and enum.
- A trigger rejects any payment split that does not add up to the payment;
  another blocks edits to recorded payments.
- Every write runs inside a single transaction.
- Migrations are versioned. Before an existing database is upgraded, a copy is
  saved in `%LOCALAPPDATA%\VZFinance\backups`. A database from a newer app
  version is refused without being changed.
- Schema v2 replaced annual rates, methods, terms and stored schedules with the
  per-period model; an existing loan's annual rate became the equivalent
  monthly (÷12) or every-15-days (÷24) rate.
- Backups use SQLite's online backup API and are verified before a restore; a
  restore first saves a safety copy of the current data.
- The database is **not encrypted** by the app. Use BitLocker or device
  encryption to protect data at rest.
