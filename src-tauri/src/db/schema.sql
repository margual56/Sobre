CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS accounts (
    id             INTEGER PRIMARY KEY,
    email          TEXT NOT NULL UNIQUE,
    display_name   TEXT NOT NULL DEFAULT '',
    imap_host      TEXT NOT NULL,
    imap_port      INTEGER NOT NULL,
    smtp_host      TEXT NOT NULL,
    smtp_port      INTEGER NOT NULL,
    smtp_starttls  INTEGER NOT NULL DEFAULT 0,
    username       TEXT NOT NULL,
    auth_kind      TEXT NOT NULL,            -- 'password' | 'oauth'
    oauth_provider TEXT,                     -- 'google' | 'microsoft'
    secret         TEXT,                     -- only used in passphrase mode
    created_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS folders (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    role        TEXT NOT NULL DEFAULT 'other', -- inbox|sent|drafts|junk|trash|archive|other
    uidvalidity INTEGER,
    UNIQUE (account_id, name)
);

CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER NOT NULL,
    message_id      TEXT,
    in_reply_to     TEXT,
    refs            TEXT,
    subject         TEXT NOT NULL DEFAULT '',
    from_name       TEXT NOT NULL DEFAULT '',
    from_addr       TEXT NOT NULL DEFAULT '',
    to_json         TEXT NOT NULL DEFAULT '[]',
    cc_json         TEXT NOT NULL DEFAULT '[]',
    reply_to        TEXT,
    date            INTEGER NOT NULL DEFAULT 0,
    size            INTEGER NOT NULL DEFAULT 0,
    seen            INTEGER NOT NULL DEFAULT 0,
    flagged         INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    preview         TEXT NOT NULL DEFAULT '',
    auth_results    TEXT,                    -- provider's Authentication-Results header
    auth_verdict    TEXT,                    -- verified | unverified | failed
    auth_detail     TEXT,                    -- JSON report
    UNIQUE (folder_id, uid)
);
CREATE INDEX IF NOT EXISTS messages_by_folder_date ON messages (folder_id, date DESC);
CREATE INDEX IF NOT EXISTS messages_by_from ON messages (from_addr);

CREATE TABLE IF NOT EXISTS bodies (
    message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    raw        BLOB NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5 (
    subject, sender, body, tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE IF NOT EXISTS blocked (
    pattern    TEXT PRIMARY KEY,             -- 'user@example.com' or 'example.com'
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS trusted_image_senders (
    addr TEXT PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS pending_ops (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    payload    TEXT NOT NULL,                -- JSON-encoded mail::ops::Op
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS icon_cache (
    domain     TEXT PRIMARY KEY,
    mime       TEXT,                         -- NULL records a miss
    data       BLOB,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS image_cache (
    url        TEXT PRIMARY KEY,
    mime       TEXT NOT NULL,
    data       BLOB NOT NULL,
    fetched_at INTEGER NOT NULL
);
