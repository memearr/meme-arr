//! Scheduled-task rows (schedule and last outcome).

use super::*;

/// A scheduled task's persisted schedule and last-run outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub name: String,
    pub enabled: bool,
    pub interval_secs: i64,
    pub created_at: Option<i64>,
    pub last_started: Option<i64>,
    pub last_finished: Option<i64>,
    pub last_duration_ms: Option<i64>,
    pub last_error: Option<String>,
    pub last_summary: Option<String>,
}

/// Create a task's row with its defaults if it doesn't exist yet; an existing
/// row (and any schedule the user changed) is left alone.
pub fn ensure_task(
    conn: &rusqlite::Connection,
    name: &str,
    enabled: bool,
    interval_secs: i64,
    created_at: Option<i64>,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO scheduled_task(name, enabled, interval_secs, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![name, enabled, interval_secs, created_at],
    )?;
    Ok(())
}

pub fn list_tasks(conn: &rusqlite::Connection) -> Result<Vec<TaskRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, enabled, interval_secs, created_at, last_started, last_finished,
                last_duration_ms, last_error, last_summary
         FROM scheduled_task ORDER BY name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(TaskRow {
            name: r.get(0)?,
            enabled: r.get(1)?,
            interval_secs: r.get(2)?,
            created_at: r.get(3)?,
            last_started: r.get(4)?,
            last_finished: r.get(5)?,
            last_duration_ms: r.get(6)?,
            last_error: r.get(7)?,
            last_summary: r.get(8)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

pub fn set_task_schedule(
    conn: &rusqlite::Connection,
    name: &str,
    enabled: bool,
    interval_secs: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_task SET enabled = ?2, interval_secs = ?3 WHERE name = ?1",
        params![name, enabled, interval_secs],
    )?;
    Ok(())
}

pub fn record_task_start(conn: &rusqlite::Connection, name: &str, ts: i64) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_task SET last_started = ?2 WHERE name = ?1",
        params![name, ts],
    )?;
    Ok(())
}

pub fn record_task_finish(
    conn: &rusqlite::Connection,
    name: &str,
    finished_ts: i64,
    duration_ms: i64,
    error: Option<&str>,
    summary: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_task
         SET last_finished = ?2, last_duration_ms = ?3, last_error = ?4, last_summary = ?5
         WHERE name = ?1",
        params![name, finished_ts, duration_ms, error, summary],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_rows_keep_user_schedule_across_ensure_and_record_runs() {
        let pool = mem_pool();
        let conn = pool.get().unwrap();
        ensure_task(&conn, "library_scan", true, 86_400, Some(500)).unwrap();
        set_task_schedule(&conn, "library_scan", false, 3_600).unwrap();
        // A restart re-runs ensure_task with the defaults -- it must not
        // clobber the schedule the user changed, or reset created_at.
        ensure_task(&conn, "library_scan", true, 86_400, Some(900)).unwrap();
        record_task_start(&conn, "library_scan", 1_000).unwrap();
        record_task_finish(&conn, "library_scan", 1_005, 5_000, None, Some("ok")).unwrap();

        let row = list_tasks(&conn).unwrap().pop().unwrap();
        assert_eq!(
            row,
            TaskRow {
                name: "library_scan".into(),
                enabled: false,
                interval_secs: 3_600,
                created_at: Some(500),
                last_started: Some(1_000),
                last_finished: Some(1_005),
                last_duration_ms: Some(5_000),
                last_error: None,
                last_summary: Some("ok".into()),
            }
        );
    }
}
