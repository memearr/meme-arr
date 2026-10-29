//! The persisted Activity log.

use super::*;

/// How many `activity_log` rows to keep — old ones are trimmed on insert so
/// the table (and the Activity tab) stay bounded without a separate cleanup
/// job. Far larger than the 500-entry cap the old in-memory log had, since
/// this one survives restarts and is meant to replace docker-logs tailing.
pub(super) const ACTIVITY_LOG_CAP: i64 = 5000;

/// Append one activity-log row and trim anything beyond [`ACTIVITY_LOG_CAP`].
/// The trim only deletes rows once the table is actually over the cap, so
/// it's a cheap no-op on every normal insert.
pub fn insert_log_entry(
    conn: &rusqlite::Connection,
    ts: i64,
    level: &str,
    message: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO activity_log(ts, level, message) VALUES (?1, ?2, ?3)",
        params![ts, level, message],
    )?;
    conn.execute(
        "DELETE FROM activity_log WHERE id <= (SELECT MAX(id) - ?1 FROM activity_log)",
        params![ACTIVITY_LOG_CAP],
    )?;
    Ok(())
}

/// Most recent activity-log entries, newest first.
pub fn recent_log_entries(
    conn: &rusqlite::Connection,
    limit: usize,
) -> Result<Vec<(i64, String, String)>> {
    let mut stmt =
        conn.prepare("SELECT ts, level, message FROM activity_log ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit as i64], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

/// Wipe the activity log — the "Clear" action in the Activity tab.
pub fn clear_log(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute("DELETE FROM activity_log", [])?;
    Ok(())
}

/// Drop activity log entries older than `cutoff_ts` (unix seconds). Returns
/// how many were removed.
pub fn prune_log_before(conn: &rusqlite::Connection, cutoff_ts: i64) -> Result<usize> {
    Ok(conn.execute("DELETE FROM activity_log WHERE ts < ?1", params![cutoff_ts])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_log_roundtrip_ordering_and_clear() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        assert!(recent_log_entries(&conn, 10).unwrap().is_empty());

        insert_log_entry(&conn, 100, "info", "first").unwrap();
        insert_log_entry(&conn, 200, "warn", "second").unwrap();
        insert_log_entry(&conn, 300, "error", "third").unwrap();

        assert_eq!(
            recent_log_entries(&conn, 10).unwrap(),
            vec![
                (300, "error".to_string(), "third".to_string()),
                (200, "warn".to_string(), "second".to_string()),
                (100, "info".to_string(), "first".to_string()),
            ]
        );
        assert_eq!(recent_log_entries(&conn, 2).unwrap().len(), 2);

        clear_log(&conn).unwrap();
        assert!(recent_log_entries(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn activity_log_trims_beyond_cap() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        for i in 0..(ACTIVITY_LOG_CAP + 10) {
            insert_log_entry(&conn, i, "info", "x").unwrap();
        }
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM activity_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, ACTIVITY_LOG_CAP);
        // The 10 oldest rows (ts 0..10) should have been trimmed away.
        let oldest_ts: i64 = conn
            .query_row("SELECT MIN(ts) FROM activity_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(oldest_ts, 10);
    }

    #[test]
    fn prune_log_before_removes_only_older_entries() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        for ts in [100, 200, 300] {
            insert_log_entry(&conn, ts, "info", "x").unwrap();
        }
        assert_eq!(prune_log_before(&conn, 250).unwrap(), 2);
        let left: Vec<i64> = recent_log_entries(&conn, 10)
            .unwrap()
            .into_iter()
            .map(|e| e.0)
            .collect();
        assert_eq!(left, vec![300]);
    }
}
