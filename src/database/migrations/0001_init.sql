-- VZ Finance schema v1.
-- Money: INTEGER minor units (cents). Rates: INTEGER parts-per-million.
-- Dates: TEXT 'YYYY-MM-DD'. Timestamps: TEXT ISO-8601 UTC.

CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE clients (
    id          INTEGER PRIMARY KEY,
    full_name   TEXT NOT NULL CHECK (length(trim(full_name)) BETWEEN 1 AND 200),
    phone       TEXT CHECK (phone IS NULL OR length(phone) <= 60),
    email       TEXT CHECK (email IS NULL OR length(email) <= 200),
    notes       TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),
    created_at  TEXT NOT NULL,
    archived_at TEXT
) STRICT;

CREATE INDEX clients_name_idx ON clients (full_name COLLATE NOCASE);

CREATE TABLE loans (
    id             INTEGER PRIMARY KEY,
    client_id      INTEGER NOT NULL REFERENCES clients (id) ON DELETE RESTRICT,
    principal      INTEGER NOT NULL CHECK (principal > 0 AND principal <= 1000000000000),
    annual_rate    INTEGER NOT NULL CHECK (annual_rate >= 0 AND annual_rate <= 10000000),
    method         TEXT NOT NULL CHECK (method IN ('simple', 'amortized', 'interest_only')),
    frequency      TEXT NOT NULL CHECK (frequency IN ('at_maturity', 'weekly', 'biweekly', 'monthly', 'quarterly')),
    term_count     INTEGER NOT NULL CHECK (term_count BETWEEN 1 AND 600),
    start_date     TEXT NOT NULL CHECK (start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    maturity_date  TEXT NOT NULL CHECK (maturity_date > start_date),
    status         TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paid_off', 'defaulted', 'restructured')),
    status_date    TEXT,
    notes          TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),
    created_at     TEXT NOT NULL,
    CHECK ((method = 'simple') = (frequency = 'at_maturity'))
) STRICT;

CREATE INDEX loans_client_idx ON loans (client_id);
CREATE INDEX loans_status_idx ON loans (status);

CREATE TABLE loan_schedules (
    id        INTEGER PRIMARY KEY,
    loan_id   INTEGER NOT NULL REFERENCES loans (id) ON DELETE CASCADE,
    seq       INTEGER NOT NULL CHECK (seq >= 1),
    due_date  TEXT NOT NULL,
    principal INTEGER NOT NULL CHECK (principal >= 0),
    interest  INTEGER NOT NULL CHECK (interest >= 0),
    UNIQUE (loan_id, seq)
) STRICT;

CREATE TABLE payments (
    id              INTEGER PRIMARY KEY,
    loan_id         INTEGER NOT NULL REFERENCES loans (id) ON DELETE RESTRICT,
    paid_on         TEXT NOT NULL CHECK (paid_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    amount          INTEGER NOT NULL CHECK (amount > 0),
    notes           TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 2000),
    created_at      TEXT NOT NULL,
    -- Audited reversal: the row is kept and excluded from all calculations.
    reversed_at     TEXT,
    reversal_reason TEXT,
    CHECK ((reversed_at IS NULL) = (reversal_reason IS NULL))
) STRICT;

CREATE INDEX payments_loan_idx ON payments (loan_id, paid_on, id);
CREATE INDEX payments_date_idx ON payments (paid_on);

-- Exactly one allocation row per payment; amounts must add up to the payment.
CREATE TABLE payment_allocations (
    payment_id INTEGER PRIMARY KEY REFERENCES payments (id) ON DELETE CASCADE,
    principal  INTEGER NOT NULL CHECK (principal >= 0),
    interest   INTEGER NOT NULL CHECK (interest >= 0),
    fee        INTEGER NOT NULL DEFAULT 0 CHECK (fee >= 0)
) STRICT;

CREATE TRIGGER payment_allocations_balance
BEFORE INSERT ON payment_allocations
WHEN NEW.principal + NEW.interest + NEW.fee <> (SELECT amount FROM payments WHERE id = NEW.payment_id)
BEGIN
    SELECT RAISE(ABORT, 'allocation does not equal payment amount');
END;

CREATE TRIGGER payment_allocations_immutable
BEFORE UPDATE ON payment_allocations
BEGIN
    SELECT RAISE(ABORT, 'allocations are immutable; reverse the payment instead');
END;

CREATE TRIGGER payments_amount_immutable
BEFORE UPDATE OF loan_id, paid_on, amount ON payments
BEGIN
    SELECT RAISE(ABORT, 'payments are immutable; reverse the payment instead');
END;

CREATE TABLE capital_transactions (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('contribution', 'withdrawal')),
    amount      INTEGER NOT NULL CHECK (amount > 0 AND amount <= 1000000000000),
    occurred_on TEXT NOT NULL CHECK (occurred_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    notes       TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 2000),
    created_at  TEXT NOT NULL
) STRICT;

CREATE INDEX capital_date_idx ON capital_transactions (occurred_on);

CREATE TABLE loan_status_events (
    id          INTEGER PRIMARY KEY,
    loan_id     INTEGER NOT NULL REFERENCES loans (id) ON DELETE CASCADE,
    status      TEXT NOT NULL CHECK (status IN ('active', 'paid_off', 'defaulted', 'restructured')),
    occurred_on TEXT NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL
) STRICT;

CREATE INDEX loan_status_events_loan_idx ON loan_status_events (loan_id, id);
