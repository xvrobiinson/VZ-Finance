-- VZ Finance schema v3: editable records, two cash accounts, change history,
-- business logo.

-- Payments can now be edited and deleted. Every payment's split between
-- interest and capital is recalculated from the loan whenever the loan or
-- any of its payments change, so allocations are updated in place. They must
-- still always add up to the payment.
DROP TRIGGER payment_allocations_immutable;
DROP TRIGGER payments_amount_immutable;

CREATE TRIGGER payment_allocations_balance_update
BEFORE UPDATE ON payment_allocations
WHEN NEW.principal + NEW.interest + NEW.fee <> (SELECT amount FROM payments WHERE id = NEW.payment_id)
BEGIN
    SELECT RAISE(ABORT, 'allocation does not equal payment amount');
END;

-- Money moved outside loans and capital: expenses paid from an account,
-- transfers between the two accounts and balance adjustments.
--   expense:    `account` paid it; amount > 0
--   transfer:   moves `amount` from `account` to the other account; amount > 0
--   adjustment: corrects `account` by a signed amount
CREATE TABLE cash_entries (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('expense', 'transfer', 'adjustment')),
    account     TEXT NOT NULL CHECK (account IN ('capital', 'interest')),
    amount      INTEGER NOT NULL CHECK (amount <> 0 AND abs(amount) <= 1000000000000),
    occurred_on TEXT NOT NULL CHECK (occurred_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    created_at  TEXT NOT NULL,
    CHECK (kind = 'adjustment' OR amount > 0)
) STRICT;

CREATE INDEX cash_entries_date_idx ON cash_entries (occurred_on);

-- What every edit and deletion changed. `changes` holds one line per field:
-- field<TAB>old<TAB>new, with raw values (cents, ISO dates, keys).
CREATE TABLE change_log (
    id         INTEGER PRIMARY KEY,
    entity     TEXT NOT NULL CHECK (entity IN ('client', 'loan', 'payment', 'capital', 'entry')),
    entity_id  INTEGER NOT NULL,
    loan_id    INTEGER,
    action     TEXT NOT NULL CHECK (action IN ('update', 'delete')),
    changes    TEXT NOT NULL DEFAULT '',
    reason     TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 2000),
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX change_log_entity_idx ON change_log (entity, entity_id);
CREATE INDEX change_log_loan_idx ON change_log (loan_id);

-- Small binary files kept with the data, such as the business logo, so
-- backups carry them.
CREATE TABLE app_assets (
    key        TEXT PRIMARY KEY NOT NULL,
    mime       TEXT NOT NULL,
    data       BLOB NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
