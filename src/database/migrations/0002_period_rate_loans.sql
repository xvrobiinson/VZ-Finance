-- VZ Finance schema v2: loans charge a fixed percentage per period
-- (monthly or every 15 days) on the capital still owed, and stay open until
-- the capital is repaid. Replaces annual rates, methods, terms, maturity
-- dates and stored schedules.
--
-- Runs with foreign keys disabled (table rebuild); integrity is re-checked
-- before commit. Existing loans keep their id, client, principal, dates,
-- status and payments; an annual rate becomes the equivalent per-period rate.

CREATE TABLE loans_v2 (
    id           INTEGER PRIMARY KEY,
    client_id    INTEGER NOT NULL REFERENCES clients (id) ON DELETE RESTRICT,
    principal    INTEGER NOT NULL CHECK (principal > 0 AND principal <= 1000000000000),
    period_rate  INTEGER NOT NULL CHECK (period_rate >= 0 AND period_rate <= 1000000),
    frequency    TEXT NOT NULL CHECK (frequency IN ('monthly', 'biweekly')),
    start_date   TEXT NOT NULL CHECK (start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    status       TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paid_off', 'defaulted', 'restructured')),
    status_date  TEXT,
    notes        TEXT NOT NULL DEFAULT '' CHECK (length(notes) <= 10000),
    created_at   TEXT NOT NULL
) STRICT;

INSERT INTO loans_v2 (id, client_id, principal, period_rate, frequency, start_date, status, status_date, notes, created_at)
SELECT
    id,
    client_id,
    principal,
    MIN(1000000, CASE frequency WHEN 'biweekly' THEN annual_rate / 24 ELSE annual_rate / 12 END),
    CASE frequency WHEN 'biweekly' THEN 'biweekly' ELSE 'monthly' END,
    start_date,
    status,
    status_date,
    notes,
    created_at
FROM loans;

DROP TABLE loan_schedules;
DROP TABLE loans;
ALTER TABLE loans_v2 RENAME TO loans;

CREATE INDEX loans_client_idx ON loans (client_id);
CREATE INDEX loans_status_idx ON loans (status);
