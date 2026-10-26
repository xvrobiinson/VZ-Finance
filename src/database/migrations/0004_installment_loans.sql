-- Acervus Finance schema v4: installment loans with a separate interest
-- period, daily and weekly frequencies, stored schedules, late fees and
-- per-installment payment splits.
--
-- Runs with foreign keys disabled (table rebuild); integrity is re-checked
-- before commit. Existing loans become interest-only loans with the same
-- terms: their interest period is their payment frequency, so every figure
-- stays the same.

CREATE TABLE loans_v4 (
    id              INTEGER PRIMARY KEY,
    client_id       INTEGER NOT NULL REFERENCES clients (id) ON DELETE RESTRICT,
    principal       INTEGER NOT NULL CHECK (principal > 0 AND principal <= 1000000000000),
    -- Rate per interest period, parts per million.
    period_rate     INTEGER NOT NULL CHECK (period_rate >= 0 AND period_rate <= 1000000),
    interest_period TEXT NOT NULL CHECK (interest_period IN ('daily', 'weekly', 'biweekly', 'monthly')),
    frequency       TEXT NOT NULL CHECK (frequency IN ('daily', 'weekly', 'biweekly', 'monthly')),
    method          TEXT NOT NULL CHECK (method IN ('declining', 'flat', 'interest_only')),
    installments    INTEGER CHECK (installments IS NULL OR installments BETWEEN 1 AND 1000),
    -- The stored schedule was changed by hand (its interest is used as written).
    schedule_edited INTEGER NOT NULL DEFAULT 0 CHECK (schedule_edited IN (0, 1)),
    -- Charged once on what is unpaid when an installment becomes overdue.
    late_fee_rate   INTEGER NOT NULL DEFAULT 0 CHECK (late_fee_rate >= 0 AND late_fee_rate <= 1000000),
    start_date      TEXT NOT NULL CHECK (start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paid_off', 'defaulted', 'restructured')),
    status_date     TEXT,
    notes           TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),
    created_at      TEXT NOT NULL,
    CHECK ((method = 'interest_only') = (installments IS NULL))
) STRICT;

INSERT INTO loans_v4 (id, client_id, principal, period_rate, interest_period, frequency, method, installments,
                      schedule_edited, late_fee_rate, start_date, status, status_date, notes, created_at)
SELECT id, client_id, principal, period_rate, frequency, frequency, 'interest_only', NULL,
       0, 0, start_date, status, status_date, notes, created_at
FROM loans;

DROP TABLE loans;
ALTER TABLE loans_v4 RENAME TO loans;

CREATE INDEX loans_client_idx ON loans (client_id);
CREATE INDEX loans_status_idx ON loans (status);

-- The installment schedule of an installment loan.
CREATE TABLE loan_installments (
    loan_id   INTEGER NOT NULL REFERENCES loans (id) ON DELETE CASCADE,
    seq       INTEGER NOT NULL CHECK (seq >= 1),
    due_date  TEXT NOT NULL CHECK (due_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    principal INTEGER NOT NULL CHECK (principal >= 0),
    interest  INTEGER NOT NULL CHECK (interest >= 0),
    PRIMARY KEY (loan_id, seq)
) STRICT;

-- How a payment was asked to be applied:
--   auto        oldest first (late fee, interest, capital of each installment)
--   installment to `target_seq` only
--   spread      chosen amounts per installment (payment_targets)
--   custom      chosen capital / interest / fee totals (payment_allocations)
ALTER TABLE payments ADD COLUMN allocation_mode TEXT NOT NULL DEFAULT 'auto'
    CHECK (allocation_mode IN ('auto', 'installment', 'spread', 'custom'));
ALTER TABLE payments ADD COLUMN target_seq INTEGER;

CREATE TABLE payment_targets (
    payment_id INTEGER NOT NULL REFERENCES payments (id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL CHECK (seq >= 1),
    amount     INTEGER NOT NULL CHECK (amount > 0),
    PRIMARY KEY (payment_id, seq)
) STRICT;

-- How each payment on an installment loan was split per installment. The
-- totals are in payment_allocations.
CREATE TABLE installment_allocations (
    payment_id INTEGER NOT NULL REFERENCES payments (id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL CHECK (seq >= 1),
    principal  INTEGER NOT NULL CHECK (principal >= 0),
    interest   INTEGER NOT NULL CHECK (interest >= 0),
    late_fee   INTEGER NOT NULL CHECK (late_fee >= 0),
    PRIMARY KEY (payment_id, seq)
) STRICT;

-- One late fee per overdue installment, assessed once. Waived fees are kept
-- with their reason and count as zero.
CREATE TABLE late_fees (
    loan_id     INTEGER NOT NULL REFERENCES loans (id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL CHECK (seq >= 1),
    assessed_on TEXT NOT NULL CHECK (assessed_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    base_amount INTEGER NOT NULL CHECK (base_amount >= 0),
    rate        INTEGER NOT NULL CHECK (rate >= 0 AND rate <= 1000000),
    amount      INTEGER NOT NULL CHECK (amount >= 0),
    waived_at   TEXT,
    waive_reason TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (loan_id, seq)
) STRICT;

-- The change history also records late fees (adjusted or waived).
CREATE TABLE change_log_v4 (
    id         INTEGER PRIMARY KEY,
    entity     TEXT NOT NULL CHECK (entity IN ('client', 'loan', 'payment', 'capital', 'entry', 'late_fee')),
    entity_id  INTEGER NOT NULL,
    loan_id    INTEGER,
    action     TEXT NOT NULL CHECK (action IN ('update', 'delete')),
    changes    TEXT NOT NULL DEFAULT '',
    reason     TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 2000),
    created_at TEXT NOT NULL
) STRICT;

INSERT INTO change_log_v4 SELECT id, entity, entity_id, loan_id, action, changes, reason, created_at FROM change_log;
DROP TABLE change_log;
ALTER TABLE change_log_v4 RENAME TO change_log;
CREATE INDEX change_log_entity_idx ON change_log (entity, entity_id);
CREATE INDEX change_log_loan_idx ON change_log (loan_id);
