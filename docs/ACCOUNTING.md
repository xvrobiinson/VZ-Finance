# Acervus Finance accounting rules

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

A loan is an amount lent on a date at a fixed rate per **interest period**,
repaid on a **payment frequency**. The two are chosen separately; each is
daily, weekly, every 15 days or monthly.

- Daily and weekly due dates are 1 and 7 days apart.
- Monthly due dates fall on the same day of each month as the loan date
  (Jan 31 → Feb 28 → Mar 31: always counted from the loan date, never drifting).
- Every-15-days due dates are 15 days after the loan day and on the loan day of
  each following month (Jan 5 → Jan 20 → Feb 5 → Feb 20 …).

There are three loan types:

| Type | Installments | Interest of each payment period |
| --- | --- | --- |
| **Installments, declining balance** (default) | yes | rate × capital still owed when the period starts |
| **Flat installments** | yes | rate × amount lent |
| **Interest only** | no fixed term | rate × capital still owed when the period starts |

### Converting the rate

When the interest period and the payment frequency differ, the rate is
converted proportionally on a **30-day month**: a day is 1, a week 7, every 15
days 15 and a month 30 days. Interest for a payment period =
capital × rate × days(payment period) ÷ days(interest period), rounded half-up
to the cent once. Example: 10% a month paid weekly is 10% × 7 ⁄ 30 ≈ 2.33% a
week; 1% a day paid weekly is 7% a week.

## Installment loans

- The capital is split into the chosen number of installments (1 to 1,000) in
  equal parts; any cents left over go on the last installment.
- **Declining balance:** installment *k*'s interest is charged on the capital
  still owed when its period starts (the previous due date, or the loan date).
  10,000 at 10% a month in 5 installments: 3,000, 2,800, 2,600, 2,400 and
  2,200, so 3,000 of interest and 13,000 in total.
- **Flat:** every installment's interest is rate × amount lent. The same loan
  is 3,000 five times: 5,000 of interest and 15,000 in total.
- With declining balance the interest follows what really happens: capital
  paid early lowers the interest of the installments that follow, and capital
  paid late keeps it at the higher amount. No interest is charged once the
  capital reaches zero.
- **Edited schedules.** Before saving, every installment's due date, capital,
  interest or total can be changed (changing the total changes the interest).
  The app checks that due dates are after the loan date and in order, that no
  amount is negative, and that the capital adds up to the amount lent; it
  offers to put any difference on the last installment, never silently. An
  edited schedule is stored as written, its interest is used as typed, and the
  loan is marked "set by hand". An unedited schedule follows the terms as you
  type them; once edited, changing the amount, rate, frequency, number of
  installments or date shows a notice and recalculates the schedule only when
  you choose to.

### Late fees

- An installment that is not fully paid by its due date is charged a **late
  fee** on the next day: the loan's late-fee rate (5% by default, set per loan;
  0% means none) × what is still unpaid on it (capital + interest).
  10,000 in 5 at 10%: if only 1,000 of the first 3,000 is paid, the fee is
  5% × 2,000 = 100.
- It is charged **once per installment**, never again for the same
  installment and never on the fee itself. The database enforces one fee per
  installment.
- Fees are brought up to date when the app starts, within a minute of the
  date changing while it is open, and whenever a loan or payment changes. If an earlier payment is added
  or edited so an installment was in fact paid on time, its fee is removed;
  if the unpaid amount changes, the fee is recalculated. Both changes are
  written to the change history.
- A fee can be **waived** with a reason, and restored. A waived fee counts as
  zero; payments are worked out again without it.
- Unpaid late fees are part of what the client owes and of portfolio value.
  Fees collected are income, like interest.
- Late fees and interest rates may be limited by law where you lend. The app
  does not check this.

### Payments on installment loans

A payment is applied in one of three ways, chosen when it is recorded:

1. **One installment** (partial payments allowed): to that installment's late
   fee, then its interest, then its capital. More than it owes is refused.
2. **Spread over installments:** *oldest first* (each installment's fee,
   interest and capital before moving to the next; anything beyond what is due
   goes to capital of later installments, oldest first), or *amounts you
   choose* for each installment.
3. **Custom split:** amounts for late fees, interest and capital; each part is
   applied to the oldest installments first. More than is owed of any part is
   refused. Capital paid before it is due lowers the capital of later
   installments, so with declining balance the interest that follows goes down.

How each payment was applied is stored per installment, kept when other
payments change, and shown on the loan page, the receipt and the CSV export.
An installment is **overdue** the day after its due date while anything on it
is unpaid; days overdue count from the oldest such due date.

## Interest (interest-only loans)

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

## Payments (interest-only loans)

- **Order:** fee (only if entered with the payment), then interest owed (oldest
  period first), then capital.
- A payment can be any amount. Paying more than the interest always reduces the
  capital, which lowers the next period's interest.
- Paying early inside a period still covers that period's interest first.
- Amounts above what is owed (capital plus interest owed) are refused, with the
  total shown, so no unexplained credit is ever created.
- Payments must be dated on or after the loan date and not in the future. A
  payment may be dated before payments already recorded.
- A payment that brings capital and interest owed to zero closes the loan
  (**Paid off**) in the same database transaction.

## Editing and deleting

- Loans (client, amount, rate, frequency, date, notes), payments (amount, fee,
  date, notes), capital entries, expenses, transfers, adjustments and clients
  can all be edited or deleted.
- **Splits are always recalculated.** Whenever a loan or any of its payments is
  added, edited or deleted, every payment on that loan is split again, oldest
  first, by the same rules as a new payment. The loan is marked paid off, or
  reopened, to match. A receipt printed before the change shows the old split;
  printing it again shows the new one.
- A change is refused, and nothing is saved, if it would make a payment more
  than the client owed, put a payment before the loan date, or leave an
  account below zero at any point (see below).
- Deleting a loan deletes its payments. A client can only be deleted when they
  have no loans (loans can be moved to another client by editing them).
- **Change history:** every edit records each changed field's old and new value,
  the time and an optional reason; every deletion records what the record was.
  The history is shown in Portfolio Analytics and on each loan's page, and
  exported as CSV.
- Payments reversed with earlier versions stay in the history, excluded from
  every calculation; they can be deleted.

## Receipts

Every figure on a receipt comes from the same records and rules as the loan
page: stored payment splits, the stored per-installment splits, and the
ledger. A receipt shows the loan as it stood right after its payment, so a
reprint of an old receipt shows the same figures.

- **Payment receipt** (Payments, the loan page, or right after recording a
  payment): the amount paid and its split, the balance after it (capital
  before and after, interest and late fees pending, what is due and unpaid,
  the total still owed), and **every earlier payment on the loan**, oldest
  first (by date, then by the order they were recorded), with this payment
  marked. Each payment shows what it went to and the capital left after it.
  - **Installment loans** also list every installment of the schedule, no
    more and no fewer, with its amount, what this payment put on it, what is
    paid, what is left and its status: **Paid** (nothing left, with a green
    check), **Partly paid**, **Overdue** (past its due date with something
    unpaid, partly paid or not) or **Pending**. A status comes from the
    per-installment splits, never from the payment total. A payment in the
    history is **Settled** (green check) only when every installment it went
    to is paid in full; otherwise it shows **Partly paid** and what is left.
    The next installment due is shown with its date.
  - **Open (interest-only) loans** have no schedule, so their receipts show
    no due dates and no installments. A payment in the history shows
    **Interest paid to date** (green check) when no interest charged by then
    is left, **Partly paid** with the interest left otherwise, and **Loan paid
    off** when nothing is owed after it.
  - A reversed payment from older versions is listed as **Void · not
    counted**, crossed out, and is left out of every figure.
- **Loan receipt** (the loan page, or right after creating a loan): what the
  client received and on which date, the rate, how often payments are made,
  how payments work (in smaller type) and lines for both signatures, which
  sit at the foot of the page (on the last page when it runs longer).
  Installment loans add the whole schedule (due date, capital, interest and
  total of each installment), the late-fee rule and whether the schedule was
  set by agreement. Open loans show the interest per payment and no dates.
- **Client details**: Settings → Receipts → "Client info to add to every
  receipt" chooses the name, ID number, phone and address (all on by default).
  Only ticked details are printed, and only when the client has them. When the
  name is off it is left out everywhere, including the signature line. Client
  photos are never printed.
- **Paper**: both receipts are laid out on US Letter (8.5 × 11 in, 0.5 in
  margins) with the same layout on screen and in print. The usual receipt
  fits on one page (up to about 26 history rows, or 12 installments and 12
  payments; a loan receipt up to 20 installments). A longer one continues on
  the next page with the column headings repeated, every page is numbered
  ("Page 1 of 2"), and the first page says it continues. Nothing is left out.
- **Company details**: Settings → Receipts → "Company info to add to every
  receipt" ticks the company name (Settings → Company name, or "Acervus
  Finance" when empty), address and phone for the top of every receipt. A
  detail that is off or empty leaves no label or gap. With the name off, the
  company signature caption says "For the lender".
- **Wording**: Settings → Receipts → Receipt wording changes the "How payments
  work" heading, its lines (one list per loan type: reorder, add, remove), the
  two signature captions and the closing line of each receipt. Lines show the
  loan's figures through tags such as `{interest}` and `{late_fee}`; a line
  that only applies to some loans (late fee, converted rate, edited schedule)
  keeps that rule. Text left unchanged is the built-in wording, identical to
  earlier versions, and follows the language setting; "Restore original
  wording" asks first, then puts it all back. Wording never changes a loan's
  terms or figures. Long custom text is never cut: the receipt continues on
  the next page and says so (measured in Edge for 10–20 lines of 130–500
  characters).
- Both carry the logo. They are saved as HTML files in
  `%LOCALAPPDATA%\VZFinance\receipts` and open in the default browser, which
  shows the print dialog.

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

## The two cash accounts

Cash is kept in two accounts:

- **Available capital** receives capital added and the capital part of every
  payment. Withdrawals come out of it.
- **Available interest** receives the interest and fees of every payment.

Movements between and out of them:

- **New loan:** paid from available capital first; if that is not enough, the
  rest comes from available interest. Capital repaid always returns to
  available capital.
- **Expense** (money used outside the business, from *New record*): paid from
  the account you choose. Only that account goes down; loans and interest
  earned are unchanged. An expense from capital lowers capital invested; an
  expense from interest lowers the portfolio gain. It needs a reason.
- **Income** (money received from outside the business, from *New record*):
  not a loan payment and not money moved between the accounts, so it is
  counted once, as new money. It goes into the account you choose and needs no
  balance there first. Into available interest it counts as income (it raises
  the portfolio gain and "Income received"); into available capital it counts
  as capital invested, like a contribution. It needs a source or reason.
- An expense or income submitted twice within ten seconds with the same
  account, amount, date and reason (a double click) is saved once.
- **Move money:** from one account to the other. The total is unchanged.
- **Adjust balances:** type what each account really holds on a date; an
  adjustment is recorded for each difference, with a reason. Capital
  adjustments count as capital invested; interest adjustments count as income.

Within a day, money coming in is applied before money going out: capital added,
income and adjustments, then payments, transfers, loans, expenses and
withdrawals.
Neither account may go below zero at any point; a new or edited record that
would cause it is refused and the date is named.

## Capital and portfolio

| Figure | Definition |
| --- | --- |
| Capital invested | Contributions − withdrawals + capital adjustments + income put into capital − expenses paid from capital |
| Available capital, available interest | The two cash accounts above |
| Capital lent out | Capital still owed on performing (non-defaulted) loans |
| Interest due, unpaid | Interest of periods past their due date, not yet collected |
| Late fees receivable | Late fees assessed and not waived, not yet collected |
| **Portfolio value** | Available capital + available interest + capital lent out + interest due, unpaid + late fees receivable |
| Defaulted balance | What defaulted loans still owe; excluded from value |
| Portfolio gain | Portfolio value − capital invested |
| Income received | Interest + fees collected + income put into available interest |
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
  `cash + all capital still owed = capital invested + interest received + fees received + interest adjustments + income into interest − expenses paid from interest`.

## History and charts

Chart points are end-of-day portfolio values recomputed from the ledger for
each date (daily up to 120 days, every 3 days up to about 13 months, weekly
beyond), drawn as straight segments between real values.

## Storage

- SQLite at `%LOCALAPPDATA%\VZFinance\data\portfolio.db`, WAL mode, foreign
  keys enforced, CHECK constraints on every amount, rate, date and enum.
- Triggers reject any payment split, new or updated, that does not add up to
  the payment.
- Every write runs inside a single transaction.
- Migrations are versioned. Before an existing database is upgraded, a copy is
  saved in `%LOCALAPPDATA%\VZFinance\backups`. A database from a newer app
  version is refused without being changed.
- Schema v2 replaced annual rates, methods, terms and stored schedules with the
  per-period model; an existing loan's annual rate became the equivalent
  monthly (÷12) or every-15-days (÷24) rate.
- Schema v3 made payments editable (splits are kept in step by recalculation),
  and added expenses, transfers and adjustments, the change history, and the
  business logo (stored in the database, so backups carry it).
- Schema v4 added installment loans: the interest period, loan type, number of
  installments, late-fee rate and edited flag on each loan; the stored
  schedule (`loan_installments`); how each payment was applied
  (`payments.allocation_mode`, `payment_targets`, `installment_allocations`);
  and `late_fees`. Existing loans became interest-only loans with the same
  numbers. Nothing is deleted by the upgrade.
- Schema v5 added a client's ID number, address and photo.
- Schema v6 allows `income` in `cash_entries` (the table is rebuilt with
  every row copied unchanged; a copy of the database is saved first). Company
  address and phone, the receipt checkboxes and the receipt wording are
  settings, stored in `app_settings`, so they need no table and backups carry
  them.
- **Reset all data** (Settings → Data) deletes clients, loans, payments,
  money movements and the change history, keeping settings, the receipt name
  and the logo. It only runs when you type RESET, and saves a complete copy
  (`before-reset-<time>.db`) in the backups folder first.
- Backups use SQLite's online backup API and are verified before a restore; a
  restore first saves a safety copy of the current data.
- The database is **not encrypted** by the app. Use BitLocker or device
  encryption to protect data at rest.
