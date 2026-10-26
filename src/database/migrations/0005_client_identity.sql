-- Acervus Finance schema v5: an optional ID number, address and photo for
-- each client. Adds columns and a table only; no existing row changes.

ALTER TABLE clients ADD COLUMN id_number TEXT CHECK (id_number IS NULL OR length(id_number) <= 40);
ALTER TABLE clients ADD COLUMN address TEXT CHECK (address IS NULL OR length(address) <= 300);

-- A photo of the client or of their identity document. Sensitive: kept only
-- in this database (so backups carry it), shown only on the client's page,
-- never printed on receipts, exported to CSV or written to the log.
CREATE TABLE client_photos (
    client_id  INTEGER PRIMARY KEY REFERENCES clients (id) ON DELETE CASCADE,
    mime       TEXT NOT NULL CHECK (mime IN ('image/jpeg', 'image/png')),
    data       BLOB NOT NULL CHECK (length(data) > 0),
    updated_at TEXT NOT NULL
) STRICT;
