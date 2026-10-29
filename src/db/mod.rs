//! SQLite storage: media rows, subtitle cues, and an FTS5 index over cue text.

use anyhow::Result;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::path::Path;

mod logs;
mod maintenance;
mod media;
mod requests;
mod sessions;
mod settings;
mod tasks;

pub use self::logs::*;
pub use self::maintenance::*;
pub use self::media::*;
pub use self::requests::*;
pub use self::sessions::*;
pub use self::settings::*;
pub use self::tasks::*;

pub type Pool = r2d2::Pool<SqliteConnectionManager>;

pub fn open_pool(db_path: &Path) -> Result<Pool> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manager = SqliteConnectionManager::file(db_path).with_init(|c| {
        c.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA auto_vacuum = INCREMENTAL;",
        )
    });
    let pool = r2d2::Pool::builder().max_size(8).build(manager)?;
    let conn = pool.get()?;
    migrate(&conn)?;
    drop(conn);
    Ok(pool)
}

fn migrate(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS media (
            id          INTEGER PRIMARY KEY,
            rating_key  TEXT NOT NULL UNIQUE,
            kind        TEXT NOT NULL,
            title       TEXT NOT NULL,
            show        TEXT,
            season      INTEGER,
            episode     INTEGER,
            year        INTEGER,
            file_path   TEXT NOT NULL,
            part_key    TEXT,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            access_mode TEXT NOT NULL,
            library     TEXT,
            file_mtime  INTEGER NOT NULL DEFAULT 0,
            file_size   INTEGER NOT NULL DEFAULT 0,
            indexed_at  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS subtitle_cue (
            id        INTEGER PRIMARY KEY,
            media_id  INTEGER NOT NULL REFERENCES media(id) ON DELETE CASCADE,
            start_ms  INTEGER NOT NULL,
            end_ms    INTEGER NOT NULL,
            text      TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cue_media ON subtitle_cue(media_id);

        -- External-content FTS5 index over cue text.
        CREATE VIRTUAL TABLE IF NOT EXISTS subtitle_fts USING fts5(
            text,
            content='subtitle_cue',
            content_rowid='id',
            tokenize='unicode61'
        );

        -- Keep FTS in sync with the content table.
        CREATE TRIGGER IF NOT EXISTS cue_ai AFTER INSERT ON subtitle_cue BEGIN
            INSERT INTO subtitle_fts(rowid, text) VALUES (new.id, new.text);
        END;
        CREATE TRIGGER IF NOT EXISTS cue_ad AFTER DELETE ON subtitle_cue BEGIN
            INSERT INTO subtitle_fts(subtitle_fts, rowid, text) VALUES('delete', old.id, old.text);
        END;

        -- App configuration set via the Settings page (key/value) and the
        -- ordered Plex-path -> container-path mappings.
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS path_mapping (
            id             INTEGER PRIMARY KEY,
            position       INTEGER NOT NULL DEFAULT 0,
            plex_prefix    TEXT NOT NULL,
            container_path TEXT NOT NULL,
            label          TEXT
        );

        -- Persistent activity/event log, backing the Activity tab's log
        -- view -- survives restarts, unlike the in-memory log this replaced,
        -- so 'not relying on docker logs' actually holds across a redeploy.
        CREATE TABLE IF NOT EXISTS activity_log (
            id      INTEGER PRIMARY KEY,
            ts      INTEGER NOT NULL,
            level   TEXT NOT NULL,
            message TEXT NOT NULL
        );

        -- Subtitle downloads this app has asked the media server for (the
        -- Wanted tab's Requests list). No foreign key to `media`: a scan can
        -- delete and recreate an item's row, and the request should survive
        -- that. Whether a request has become searchable subtitles is worked
        -- out when listing, from the item's current state, not stored here.
        CREATE TABLE IF NOT EXISTS subtitle_request (
            id           INTEGER PRIMARY KEY,
            rating_key   TEXT NOT NULL,
            server       TEXT NOT NULL,
            language     TEXT,
            subtitle     TEXT NOT NULL,
            provider     TEXT,
            requested_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_subtitle_request_item
            ON subtitle_request(rating_key);

        -- Login sessions. Only a SHA-256 hash of the cookie token is ever
        -- stored (see auth.rs) -- this table leaking doesn't hand out valid
        -- sessions on its own.
        CREATE TABLE IF NOT EXISTS sessions (
            token_hash TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL
        );

        -- Transient per-attempt state for an in-progress OIDC login (the
        -- CSRF state token, nonce, and PKCE verifier generated at
        -- /api/auth/oidc/login, looked up and deleted at the callback).
        -- Short-lived by design -- see OIDC_FLOW_TTL_SECS.
        CREATE TABLE IF NOT EXISTS oidc_flows (
            state         TEXT PRIMARY KEY,
            nonce         TEXT NOT NULL,
            pkce_verifier TEXT NOT NULL,
            created_at    INTEGER NOT NULL,
            expires_at    INTEGER NOT NULL
        );

        -- Transient per-attempt state for an in-progress Plex PIN login,
        -- same shape/reasoning as oidc_flows. `purpose` is 'login' (grants
        -- a session if the resulting account matches the linked allowlist)
        -- or 'link' (authenticated-only -- sets the allowlist itself).
        CREATE TABLE IF NOT EXISTS plex_flows (
            pin_id      INTEGER PRIMARY KEY,
            code        TEXT NOT NULL,
            purpose     TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            expires_at  INTEGER NOT NULL
        );

        -- One row per scheduled task (see tasks.rs): its schedule plus the
        -- outcome of its most recent run, so 'last run' and 'next run'
        -- survive a restart instead of every task looking overdue at boot.
        CREATE TABLE IF NOT EXISTS scheduled_task (
            name             TEXT PRIMARY KEY,
            enabled          INTEGER NOT NULL,
            interval_secs    INTEGER NOT NULL,
            -- When the row was created; the first run is due one interval
            -- after this (NULL = due immediately, e.g. the first library scan).
            created_at       INTEGER,
            last_started     INTEGER,
            last_finished    INTEGER,
            last_duration_ms INTEGER,
            last_error       TEXT,
            last_summary     TEXT
        );
        ",
    )?;
    // Columns added after the initial release (existing DBs won't have them).
    add_column_if_missing(conn, "media", "library", "TEXT")?;
    add_column_if_missing(
        conn,
        "media",
        "indexer_version",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // How many cues each item has, kept by `upsert_media` / `insert_cues`, so
    // the library views never have to count the (huge) cue table. Filled once,
    // here, for databases from before the column existed.
    if add_column_if_missing(conn, "media", "cue_count", "INTEGER NOT NULL DEFAULT 0")? {
        conn.execute(
            "UPDATE media SET cue_count =
                (SELECT COUNT(*) FROM subtitle_cue c WHERE c.media_id = media.id)",
            [],
        )?;
    }
    Ok(())
}

/// `ALTER TABLE ADD COLUMN` only if the column isn't already present, so repeated
/// startups on an existing database are a no-op.
/// Returns whether the column was just added.
fn add_column_if_missing(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<bool> {
    let present = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(Result::ok)
        .any(|name| name == column);
    if !present {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
            [],
        )?;
    }
    Ok(!present)
}

/// A single-connection in-memory database with the full schema, for tests in
/// any module. One connection only: each new `:memory:` connection would be
/// a separate, empty database.
#[cfg(test)]
pub(crate) fn test_pool() -> Pool {
    let manager = SqliteConnectionManager::memory();
    let pool = r2d2::Pool::builder().max_size(1).build(manager).unwrap();
    migrate(&pool.get().unwrap()).unwrap();
    pool
}

#[cfg(test)]
pub(super) fn mem_pool() -> Pool {
    test_pool()
}

#[cfg(test)]
pub(super) fn add_media(
    conn: &rusqlite::Connection,
    key: &str,
    kind: &str,
    title: &str,
    show: Option<&str>,
    access: &str,
    library: &str,
) -> i64 {
    upsert_media(
        conn,
        key,
        kind,
        title,
        show,
        show.map(|_| 1),
        show.map(|_| 2),
        Some(2020),
        &format!("/m/{key}.mkv"),
        None,
        1000,
        access,
        Some(library),
        1,
        1,
    )
    .unwrap()
}
