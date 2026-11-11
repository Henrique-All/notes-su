use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use notes_core::{DEFAULT_COLORS, normalize_search_text};
use rusqlite::{Connection, functions::FunctionFlags, params};
use uuid::Uuid;

use crate::error::StoreResult;

pub const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_session (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    user_id TEXT NOT NULL,
    email TEXT NOT NULL,
    name TEXT,
    access_token TEXT NOT NULL,
    refresh_token TEXT NOT NULL,
    device_id TEXT NOT NULL,
    sync_cursor INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS colors (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    color_code TEXT NOT NULL,
    sort_index INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    headline TEXT NOT NULL DEFAULT '' CHECK (locked = 0 OR headline = ''),
    is_generated_title INTEGER NOT NULL DEFAULT 0,
    pinned INTEGER NOT NULL DEFAULT 0,
    favorite INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    readonly INTEGER NOT NULL DEFAULT 0,
    local_only INTEGER NOT NULL DEFAULT 0,
    conflicted INTEGER NOT NULL DEFAULT 0,
    locked INTEGER NOT NULL DEFAULT 0,
    color_id TEXT REFERENCES colors(id) ON DELETE SET NULL,
    expiry_date INTEGER,
    expiry_modified_at INTEGER,
    trashed_at INTEGER,
    deleted_by TEXT CHECK (deleted_by IS NULL OR deleted_by IN ('user', 'app', 'expired')),
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_edited INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS note_contents (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL UNIQUE REFERENCES notes(id) ON DELETE CASCADE,
    format TEXT NOT NULL DEFAULT 'markdown' CHECK (format IN ('markdown', 'tiptap')),
    locked INTEGER NOT NULL DEFAULT 0,
    data TEXT NOT NULL DEFAULT '',
    conflicted_data TEXT,
    local_only INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_edited INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    date_resolved INTEGER,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS notebooks (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES notebooks(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    pinned INTEGER NOT NULL DEFAULT 0,
    trashed_at INTEGER,
    deleted_by TEXT CHECK (deleted_by IS NULL OR deleted_by IN ('user', 'app')),
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_edited INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS note_notebooks (
    id TEXT PRIMARY KEY,
    notebook_id TEXT NOT NULL REFERENCES notebooks(id) ON DELETE CASCADE,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    UNIQUE(notebook_id, note_id)
);

CREATE TABLE IF NOT EXISTS tags (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL COLLATE BINARY,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_tags_active_title
    ON tags(title COLLATE BINARY) WHERE deleted = 0;

CREATE TABLE IF NOT EXISTS note_tags (
    id TEXT PRIMARY KEY,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    UNIQUE(tag_id, note_id)
);

CREATE TABLE IF NOT EXISTS shortcuts (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL,
    item_type TEXT NOT NULL CHECK (item_type IN ('notebook', 'tag')),
    sort_index INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_shortcuts_active_item
    ON shortcuts(item_type, item_id) WHERE deleted = 0;

CREATE TABLE IF NOT EXISTS reminders (
    id TEXT PRIMARY KEY,
    note_id TEXT REFERENCES notes(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    date INTEGER NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('once', 'repeat', 'permanent')),
    recurring_mode TEXT CHECK (recurring_mode IS NULL OR recurring_mode IN ('day', 'week', 'month', 'year')),
    selected_days TEXT NOT NULL DEFAULT '[]',
    priority TEXT NOT NULL DEFAULT 'vibrate' CHECK (priority IN ('silent', 'vibrate', 'urgent')),
    disabled INTEGER NOT NULL DEFAULT 0,
    snooze_until INTEGER,
    local_only INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    hash TEXT NOT NULL,
    filename TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    size INTEGER NOT NULL,
    encrypted INTEGER NOT NULL DEFAULT 0,
    encryption_meta TEXT,
    local_blob BLOB,
    storage_path TEXT,
    date_uploaded INTEGER,
    failed TEXT,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_attachments_active_hash
    ON attachments(hash) WHERE deleted = 0;

CREATE TABLE IF NOT EXISTS note_attachments (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    attachment_id TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    UNIQUE(note_id, attachment_id)
);

CREATE TABLE IF NOT EXISTS vault (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT 'Default',
    key_canary TEXT NOT NULL,
    lock_after_seconds INTEGER NOT NULL DEFAULT 300,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS history_sessions (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    session_key TEXT,
    title TEXT NOT NULL,
    format TEXT NOT NULL DEFAULT 'markdown',
    data TEXT NOT NULL,
    locked INTEGER NOT NULL DEFAULT 0,
    compressed INTEGER NOT NULL DEFAULT 0,
    device_id TEXT,
    date_created INTEGER NOT NULL,
    date_modified INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS monographs (
    id TEXT PRIMARY KEY,
    note_id TEXT NOT NULL UNIQUE REFERENCES notes(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    content TEXT,
    encrypted_content TEXT,
    self_destruct INTEGER NOT NULL DEFAULT 0,
    view_count INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_published INTEGER NOT NULL,
    date_updated INTEGER NOT NULL,
    date_modified INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    local_only INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    synced INTEGER NOT NULL DEFAULT 0,
    date_modified INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

-- Standalone FTS5 virtual table: NEVER stores locked note body!
CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
    note_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS trg_notes_ai AFTER INSERT ON notes BEGIN
    DELETE FROM notes_fts WHERE note_id = NEW.id;
    INSERT INTO notes_fts(note_id, title, body)
    SELECT NEW.id, NEW.title,
           CASE WHEN NEW.locked = 0 AND COALESCE(nc.locked, 0) = 0 AND COALESCE(nc.deleted, 0) = 0
                THEN COALESCE(nc.data, '') ELSE '' END
    FROM (SELECT 1)
    LEFT JOIN note_contents nc ON nc.note_id = NEW.id
    WHERE NEW.deleted = 0;
END;

CREATE TRIGGER IF NOT EXISTS trg_notes_au AFTER UPDATE ON notes BEGIN
    DELETE FROM notes_fts WHERE note_id = OLD.id;
    INSERT INTO notes_fts(note_id, title, body)
    SELECT NEW.id, NEW.title,
           CASE WHEN NEW.locked = 0 AND COALESCE(nc.locked, 0) = 0 AND COALESCE(nc.deleted, 0) = 0
                THEN COALESCE(nc.data, '') ELSE '' END
    FROM (SELECT 1)
    LEFT JOIN note_contents nc ON nc.note_id = NEW.id
    WHERE NEW.deleted = 0;
END;

CREATE TRIGGER IF NOT EXISTS trg_notes_ad AFTER DELETE ON notes BEGIN
    DELETE FROM notes_fts WHERE note_id = OLD.id;
END;

CREATE TRIGGER IF NOT EXISTS trg_note_contents_ai AFTER INSERT ON note_contents BEGIN
    DELETE FROM notes_fts WHERE note_id = NEW.note_id;
    INSERT INTO notes_fts(note_id, title, body)
    SELECT n.id, n.title,
           CASE WHEN n.locked = 0 AND NEW.locked = 0 AND NEW.deleted = 0
                THEN NEW.data ELSE '' END
    FROM notes n
    WHERE n.id = NEW.note_id AND n.deleted = 0;
END;

CREATE TRIGGER IF NOT EXISTS trg_note_contents_au AFTER UPDATE ON note_contents BEGIN
    DELETE FROM notes_fts WHERE note_id = NEW.note_id;
    INSERT INTO notes_fts(note_id, title, body)
    SELECT n.id, n.title,
           CASE WHEN n.locked = 0 AND NEW.locked = 0 AND NEW.deleted = 0
                THEN NEW.data ELSE '' END
    FROM notes n
    WHERE n.id = NEW.note_id AND n.deleted = 0;
END;

CREATE TRIGGER IF NOT EXISTS trg_note_contents_ad AFTER DELETE ON note_contents BEGIN
    DELETE FROM notes_fts WHERE note_id = OLD.note_id;
    INSERT INTO notes_fts(note_id, title, body)
    SELECT n.id, n.title, ''
    FROM notes n
    WHERE n.id = OLD.note_id AND n.deleted = 0;
END;

CREATE INDEX IF NOT EXISTS idx_notes_active_list ON notes(deleted, trashed_at, archived, pinned DESC, date_edited DESC);
CREATE INDEX IF NOT EXISTS idx_notes_unsynced ON notes(synced) WHERE synced = 0;
CREATE INDEX IF NOT EXISTS idx_note_contents_unsynced ON note_contents(synced) WHERE synced = 0;
CREATE INDEX IF NOT EXISTS idx_notebooks_parent ON notebooks(parent_id, deleted, trashed_at);
CREATE INDEX IF NOT EXISTS idx_note_notebooks_nb ON note_notebooks(notebook_id, deleted);
CREATE INDEX IF NOT EXISTS idx_note_notebooks_note ON note_notebooks(note_id, deleted);
CREATE INDEX IF NOT EXISTS idx_note_tags_note ON note_tags(note_id, deleted);
CREATE INDEX IF NOT EXISTS idx_history_note ON history_sessions(note_id, date_modified DESC);
"#;

#[derive(Clone)]
pub struct NotesStore {
    pub(crate) conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for NotesStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotesStore").finish_non_exhaustive()
    }
}

impl NotesStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let p = path.as_ref();
        if let Some(parent) = p.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(p)?;
        Self::init_connection(conn)
    }

    pub fn in_memory() -> StoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(conn)
    }

    pub(crate) fn lock_conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn init_connection(conn: Connection) -> StoreResult<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;

        let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;

        conn.create_scalar_function("norm_fold", 1, flags, |ctx| {
            let text: Option<String> = ctx.get(0)?;
            Ok(normalize_search_text(&text.unwrap_or_default()))
        })?;

        conn.create_scalar_function("norm_contains", 2, flags, |ctx| {
            let haystack: Option<String> = ctx.get(0)?;
            let needle: Option<String> = ctx.get(1)?;
            let h = normalize_search_text(&haystack.unwrap_or_default());
            let n = normalize_search_text(&needle.unwrap_or_default());
            Ok(if n.is_empty() { true } else { h.contains(&n) })
        })?;

        conn.execute_batch(SCHEMA_DDL)?;

        let now_ms = Utc::now().timestamp_millis();
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
            params![now_ms],
        )?;

        ensure_default_colors(&conn, now_ms)?;
        ensure_local_device_id(&conn, now_ms)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn journal_mode(&self) -> StoreResult<String> {
        let conn = self.lock_conn();
        let mode: String = conn.query_row("PRAGMA journal_mode;", [], |r| r.get(0))?;
        Ok(mode)
    }

    pub fn foreign_keys_enabled(&self) -> StoreResult<bool> {
        let conn = self.lock_conn();
        let enabled: i64 = conn.query_row("PRAGMA foreign_keys;", [], |r| r.get(0))?;
        Ok(enabled == 1)
    }
}

pub(crate) fn ensure_default_colors(conn: &Connection, now_ms: i64) -> StoreResult<()> {
    let existing_count: i64 = conn.query_row("SELECT COUNT(*) FROM colors", [], |r| r.get(0))?;
    if existing_count > 0 {
        return Ok(());
    }
    for (idx, (title, code)) in DEFAULT_COLORS.iter().enumerate() {
        conn.execute(
            "INSERT INTO colors (id, title, color_code, sort_index, deleted, synced, date_created, date_modified, revision)
             VALUES (?1, ?2, ?3, ?4, 0, 1, ?5, ?5, 0)",
            params![Uuid::new_v4().to_string(), title, code, idx as i32, now_ms],
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_local_device_id(conn: &Connection, now_ms: i64) -> StoreResult<String> {
    if let Ok(dev_id) = conn.query_row(
        "SELECT value FROM settings WHERE key = 'local:deviceId' AND deleted = 0",
        [],
        |r| r.get::<_, String>(0),
    ) {
        let parsed: String = serde_json::from_str(&dev_id).unwrap_or(dev_id);
        if !parsed.is_empty() {
            return Ok(parsed);
        }
    }
    let generated = Uuid::new_v4().to_string();
    let json_val = serde_json::to_string(&generated)?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (id, key, value, local_only, deleted, synced, date_modified, revision)
         VALUES (?1, 'local:deviceId', ?2, 1, 0, 1, ?3, 0)",
        params![Uuid::new_v4().to_string(), json_val, now_ms],
    )?;
    Ok(generated)
}

pub(crate) fn ms_to_utc(ms: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_else(Utc::now)
}

pub(crate) fn opt_ms_to_utc(opt: Option<i64>) -> Option<DateTime<Utc>> {
    opt.and_then(DateTime::<Utc>::from_timestamp_millis)
}

pub(crate) fn monotonic_now_ms(prev_ms: i64) -> i64 {
    Utc::now().timestamp_millis().max(prev_ms.saturating_add(1))
}
