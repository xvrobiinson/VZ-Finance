-- Acervus Finance schema v7: cloud sync. Adds tables only; no existing row
-- changes. They stay empty until a database is linked to a business in the
-- cloud; a linked database is a copy of that business's books, where each
-- record's id is its record number in the cloud.

-- Which cloud record each local row is, and the version it was at.
-- Late fees are keyed by their loan and installment (`seq`); every other
-- record has seq 0.
CREATE TABLE remote_ids (
    entity    TEXT NOT NULL CHECK (entity IN ('client', 'loan', 'payment', 'late_fee', 'capital', 'entry')),
    local_id  INTEGER NOT NULL,
    seq       INTEGER NOT NULL DEFAULT 0,
    remote_id TEXT NOT NULL CHECK (length(remote_id) BETWEEN 1 AND 64),
    version   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (entity, local_id, seq),
    UNIQUE (entity, remote_id)
) STRICT;

-- Changes made on this PC that the cloud hasn't accepted yet, sent in
-- order. `op_id` makes sending twice harmless. A change the cloud refused
-- keeps its reason for review and is no longer sent.
CREATE TABLE outbox (
    id         INTEGER PRIMARY KEY,
    op_id      TEXT NOT NULL UNIQUE,
    kind       TEXT NOT NULL,
    args       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    refused_at TEXT,
    error      TEXT NOT NULL DEFAULT ''
) STRICT;

-- The linked business, and what the last copy from the cloud said.
CREATE TABLE sync_state (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    business_id      TEXT NOT NULL,
    business_name    TEXT NOT NULL,
    timezone         TEXT NOT NULL,
    user_id          TEXT NOT NULL,
    role             TEXT NOT NULL CHECK (role IN ('admin', 'collector', 'viewer')),
    -- "Today" in the business's time zone when the copy was made.
    today            TEXT NOT NULL CHECK (today GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    settings_version INTEGER NOT NULL DEFAULT 0,
    -- The business language in the cloud (on this PC the language is the
    -- person's own choice; settings sent back keep the cloud's).
    language         TEXT NOT NULL DEFAULT 'es',
    applied_at       TEXT NOT NULL
) STRICT;

-- Files downloaded from the cloud and which version of them: `logo` (tag:
-- the logo's address) and `photo:<client cloud id>` (tag: the client's
-- version, which changes with the photo). ID photos are kept only on
-- admins' PCs, encrypted for this Windows user (DPAPI), in client_photos.
CREATE TABLE cloud_files (
    key TEXT PRIMARY KEY NOT NULL,
    tag TEXT NOT NULL
) STRICT;
