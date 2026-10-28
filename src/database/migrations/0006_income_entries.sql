-- Acervus Finance schema v6: income from outside the business (money
-- received that is neither a loan payment nor a transfer between the two
-- accounts). SQLite can't change a CHECK constraint in place, so the table
-- is rebuilt with every existing row copied unchanged.

CREATE TABLE cash_entries_v6 (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer', 'adjustment')),
    account     TEXT NOT NULL CHECK (account IN ('capital', 'interest')),
    amount      INTEGER NOT NULL CHECK (amount <> 0 AND abs(amount) <= 1000000000000),
    occurred_on TEXT NOT NULL CHECK (occurred_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    description TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 2000),
    created_at  TEXT NOT NULL,
    CHECK (kind = 'adjustment' OR amount > 0)
) STRICT;

INSERT INTO cash_entries_v6 (id, kind, account, amount, occurred_on, description, created_at)
SELECT id, kind, account, amount, occurred_on, description, created_at FROM cash_entries;

DROP TABLE cash_entries;
ALTER TABLE cash_entries_v6 RENAME TO cash_entries;

CREATE INDEX cash_entries_date_idx ON cash_entries (occurred_on);
