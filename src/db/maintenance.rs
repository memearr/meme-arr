//! Database statistics, size, checkpoint, and vacuum.

use super::*;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Stats {
    pub media_count: i64,
    pub cue_count: i64,
}

pub fn stats(conn: &rusqlite::Connection) -> Result<Stats> {
    let media_count: i64 = conn.query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0))?;
    let cue_count: i64 = conn.query_row("SELECT COUNT(*) FROM subtitle_cue", [], |r| r.get(0))?;
    Ok(Stats {
        media_count,
        cue_count,
    })
}

/// Disk usage of the dialogue index database, broken down into what we can
/// measure exactly (the raw text actually indexed) versus everything else
/// (the FTS5 search index, B-tree structure, free pages). A byte-exact
/// per-table breakdown would need SQLite's `dbstat` virtual table, which
/// requires a special compile-time flag we don't enable — this approach needs
/// nothing special and is always available.
#[derive(Debug, Clone, Serialize, Default)]
pub struct DbUsage {
    /// Size of the main database file on disk.
    pub db_file_bytes: i64,
    /// Size of the write-ahead log file, if present (0 otherwise).
    pub wal_bytes: i64,
    /// `db_file_bytes + wal_bytes` — the real total disk footprint.
    pub total_bytes: i64,
    pub media_rows: i64,
    pub cue_rows: i64,
    /// Exact: `SUM(LENGTH(text))` over every indexed subtitle line.
    pub subtitle_text_bytes: i64,
    /// Exact: combined length of the title/show/path/etc. text columns in `media`.
    pub media_text_bytes: i64,
    /// `total_bytes` minus the two text sums above — the FTS5 index plus
    /// ordinary SQLite storage overhead. Clamped at 0 (a freshly-vacuumed,
    /// mostly-empty DB can otherwise read slightly negative from rounding).
    pub index_overhead_bytes: i64,
}

pub fn db_usage(conn: &rusqlite::Connection, db_path: &std::path::Path) -> Result<DbUsage> {
    let db_file_bytes = std::fs::metadata(db_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    let wal_path = append_suffix(db_path, "-wal");
    let wal_bytes = std::fs::metadata(&wal_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    let total_bytes = db_file_bytes + wal_bytes;

    let media_rows: i64 = conn.query_row("SELECT COUNT(*) FROM media", [], |r| r.get(0))?;
    let cue_rows: i64 = conn.query_row("SELECT COUNT(*) FROM subtitle_cue", [], |r| r.get(0))?;
    let subtitle_text_bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(text)), 0) FROM subtitle_cue",
        [],
        |r| r.get(0),
    )?;
    let media_text_bytes: i64 = conn.query_row(
        "SELECT COALESCE(SUM(
            LENGTH(title) + LENGTH(COALESCE(show,'')) + LENGTH(file_path) +
            LENGTH(COALESCE(part_key,'')) + LENGTH(access_mode) + LENGTH(COALESCE(library,''))
         ), 0) FROM media",
        [],
        |r| r.get(0),
    )?;
    let index_overhead_bytes = (total_bytes - subtitle_text_bytes - media_text_bytes).max(0);

    Ok(DbUsage {
        db_file_bytes,
        wal_bytes,
        total_bytes,
        media_rows,
        cue_rows,
        subtitle_text_bytes,
        media_text_bytes,
        index_overhead_bytes,
    })
}

pub(super) fn append_suffix(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
}

/// Cheap, safe-to-run-often housekeeping: fold the write-ahead log back into
/// the main file, and reclaim a bounded batch of pages that `DELETE`/`UPDATE`
/// churn (reindexing replaces a changed item's cues) has freed but not yet
/// returned to the OS. Intended to run automatically after any reindex pass
/// that actually changed something, so free space doesn't quietly accumulate
/// between the occasional manual `vacuum_and_optimize` below.
pub fn checkpoint_and_trim(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA wal_checkpoint(TRUNCATE);
         PRAGMA incremental_vacuum(5000);",
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct VacuumResult {
    pub before_bytes: i64,
    pub after_bytes: i64,
    pub saved_bytes: i64,
    pub duration_ms: i64,
}

/// Heavier, user-triggered deep clean: merge the FTS5 dialogue index's
/// internal segments (`optimize`, undoes fragmentation from incremental
/// inserts) and physically compact the file (`VACUUM`, returns freed pages to
/// the OS — covers everything `checkpoint_and_trim`'s bounded reclaim hasn't
/// caught up on yet, including the one-time backlog from the mass
/// delete+reinsert of a forced full reindex). Also converts an existing
/// database to incremental-auto-vacuum mode if it wasn't already (see
/// `open_pool`), bounding future growth without needing another full vacuum.
///
/// Rewrites the whole file, so it briefly needs roughly its own size again in
/// free disk space and can take a while on a large library — meant to be
/// triggered explicitly, not run on a schedule.
pub fn vacuum_and_optimize(
    conn: &rusqlite::Connection,
    db_path: &std::path::Path,
) -> Result<VacuumResult> {
    let started = std::time::Instant::now();
    let before_bytes = std::fs::metadata(db_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0)
        + std::fs::metadata(append_suffix(db_path, "-wal"))
            .map(|m| m.len() as i64)
            .unwrap_or(0);

    conn.execute(
        "INSERT INTO subtitle_fts(subtitle_fts) VALUES('optimize')",
        [],
    )?;
    conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;

    let after_bytes = std::fs::metadata(db_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0)
        + std::fs::metadata(append_suffix(db_path, "-wal"))
            .map(|m| m.len() as i64)
            .unwrap_or(0);

    Ok(VacuumResult {
        before_bytes,
        after_bytes,
        saved_bytes: (before_bytes - after_bytes).max(0),
        duration_ms: started.elapsed().as_millis() as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subtitles::Cue;

    #[test]
    fn db_usage_reports_real_file_size_and_exact_text_sums() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("index.sqlite3");
        let pool = open_pool(&db_path).unwrap();
        let conn = pool.get().unwrap();

        upsert_media(
            &conn,
            "a",
            "movie",
            "Dune",
            None,
            None,
            None,
            None,
            "/m/dune.mkv",
            None,
            0,
            "direct",
            Some("Movies"),
            1,
            1,
        )
        .unwrap();
        let mut conn2 = pool.get().unwrap();
        let media_id = conn2
            .query_row("SELECT id FROM media WHERE rating_key='a'", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap();
        insert_cues(
            &mut conn2,
            media_id,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "spice must flow".into(),
            }],
        )
        .unwrap();

        let usage = db_usage(&conn, &db_path).unwrap();
        assert_eq!(usage.media_rows, 1);
        assert_eq!(usage.cue_rows, 1);
        assert_eq!(usage.subtitle_text_bytes, "spice must flow".len() as i64);
        assert!(usage.media_text_bytes > 0); // title/path/etc text was summed
        assert!(
            usage.db_file_bytes > 0,
            "a real sqlite file should be non-empty"
        );
        assert_eq!(usage.total_bytes, usage.db_file_bytes + usage.wal_bytes);
        // The text we can account for exactly should never exceed the file itself.
        assert!(usage.subtitle_text_bytes + usage.media_text_bytes <= usage.total_bytes);
        assert!(usage.index_overhead_bytes >= 0);
    }

    #[test]
    fn checkpoint_and_trim_runs_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("index.sqlite3");
        let pool = open_pool(&db_path).unwrap();
        let conn = pool.get().unwrap();
        upsert_media(
            &conn, "a", "movie", "A", None, None, None, None, "/a.mkv", None, 0, "direct", None, 1,
            1,
        )
        .unwrap();
        // Should succeed whether or not there's anything meaningful to reclaim yet.
        checkpoint_and_trim(&conn).unwrap();
    }

    #[test]
    fn vacuum_and_optimize_shrinks_or_holds_steady_and_reports_sane_timing() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("index.sqlite3");
        let pool = open_pool(&db_path).unwrap();
        let mut conn = pool.get().unwrap();

        // Create and then delete a bunch of cues to leave some free space behind.
        let media_id = upsert_media(
            &conn, "a", "movie", "A", None, None, None, None, "/a.mkv", None, 0, "direct", None, 1,
            1,
        )
        .unwrap();
        let cues: Vec<Cue> = (0..500)
            .map(|i| Cue {
                start_ms: i,
                end_ms: i + 1,
                text: format!("line number {i} with some padding text to take up space"),
            })
            .collect();
        insert_cues(&mut conn, media_id, &cues).unwrap();
        conn.execute(
            "DELETE FROM subtitle_cue WHERE media_id = ?1",
            params![media_id],
        )
        .unwrap();

        let result = vacuum_and_optimize(&conn, &db_path).unwrap();
        assert!(result.before_bytes > 0);
        assert!(result.after_bytes > 0);
        assert!(
            result.after_bytes <= result.before_bytes,
            "vacuum should never grow the file"
        );
        assert_eq!(result.saved_bytes, result.before_bytes - result.after_bytes);
        assert!(result.duration_ms >= 0);

        // The FTS index should still work correctly after optimize + vacuum.
        insert_cues(
            &mut conn,
            media_id,
            &[Cue {
                start_ms: 0,
                end_ms: 1,
                text: "spice must flow".into(),
            }],
        )
        .unwrap();
        let hits = search_dialogue(&conn, "spice must flow", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }
}
