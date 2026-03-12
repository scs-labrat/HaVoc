//! Inline SQL schema (run once on first open).

pub const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Keystore: encrypted identity seed bytes
CREATE TABLE IF NOT EXISTS keystore (
    id              TEXT PRIMARY KEY,
    handle          TEXT NOT NULL,
    bio             TEXT DEFAULT '',
    created_at      INTEGER NOT NULL,
    encrypted_seed  TEXT NOT NULL,
    kdf_salt        TEXT NOT NULL
);

-- Known public identities (profiles fetched from DHT)
CREATE TABLE IF NOT EXISTS identities (
    author_id       TEXT PRIMARY KEY,
    handle          TEXT NOT NULL,
    bio             TEXT DEFAULT '',
    public_key      TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    raw_json        TEXT NOT NULL
);

-- Threads
CREATE TABLE IF NOT EXISTS threads (
    object_id       TEXT PRIMARY KEY,
    author_id       TEXT NOT NULL,
    title           TEXT NOT NULL,
    tags            TEXT NOT NULL DEFAULT '[]',
    created_at      INTEGER NOT NULL,
    post_count      INTEGER NOT NULL DEFAULT 0,
    last_post_at    INTEGER,
    raw_json        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS threads_author  ON threads(author_id);
CREATE INDEX IF NOT EXISTS threads_created ON threads(created_at DESC);

-- Posts
CREATE TABLE IF NOT EXISTS posts (
    object_id       TEXT PRIMARY KEY,
    thread_id       TEXT NOT NULL,
    parent_id       TEXT,
    author_id       TEXT NOT NULL,
    body            TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    tombstoned      INTEGER NOT NULL DEFAULT 0,
    attachment_meta TEXT,
    raw_json        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS posts_thread ON posts(thread_id, created_at ASC);
CREATE INDEX IF NOT EXISTS posts_author ON posts(author_id);

-- Direct messages (decrypted local copy)
CREATE TABLE IF NOT EXISTS messages (
    object_id       TEXT PRIMARY KEY,
    sender_id       TEXT NOT NULL,
    recipient_id    TEXT NOT NULL,
    body            TEXT NOT NULL,
    sent_at         INTEGER NOT NULL,
    received_at     INTEGER,
    direction       TEXT NOT NULL CHECK (direction IN ('sent', 'received')),
    read            INTEGER NOT NULL DEFAULT 0,
    raw_envelope    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_conv ON messages(sender_id, recipient_id, sent_at ASC);

-- Contacts
CREATE TABLE IF NOT EXISTS contacts (
    author_id       TEXT PRIMARY KEY,
    nickname        TEXT,
    added_at        INTEGER NOT NULL,
    blocked         INTEGER NOT NULL DEFAULT 0
);

-- DHT key registry: maps logical paths to Veilid RecordKeys
CREATE TABLE IF NOT EXISTS dht_keys (
    logical_key     TEXT PRIMARY KEY,
    record_key      TEXT NOT NULL,
    owner_secret    TEXT,
    is_owned        INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL
);

-- Tombstones
CREATE TABLE IF NOT EXISTS tombstones (
    object_id       TEXT PRIMARY KEY,
    target_id       TEXT NOT NULL,
    author_id       TEXT NOT NULL,
    reason          TEXT,
    created_at      INTEGER NOT NULL,
    raw_json        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS tombstones_target ON tombstones(target_id);

-- Board index: shared DHT record key for thread discovery
CREATE TABLE IF NOT EXISTS board_index (
    board_name      TEXT NOT NULL DEFAULT 'default',
    thread_dht_key  TEXT NOT NULL,
    thread_id       TEXT NOT NULL,
    added_at        INTEGER NOT NULL,
    PRIMARY KEY (board_name, thread_id)
);
"#;
