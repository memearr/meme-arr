//! Scheduled background tasks (System -> Tasks), modeled on Sonarr's Tasks
//! page and Jellyfin's scheduled tasks: each task runs on its own interval,
//! shows its last and next run, and can be run on demand.
//!
//! The schedule and each task's last outcome live in the `scheduled_task`
//! table, so they survive restarts -- a restart no longer means a full
//! library rescan, only a scan that's actually due. Which tasks are running
//! right now is in-memory (`AppState::running_tasks`).

use crate::diskstats::fmt_bytes;
use crate::state::{AppState, JobStatus, LogLevel};
use crate::sync::{MutexExt, RwLockExt};
use crate::{db, diskstats, index};
use anyhow::Result;
use serde::Serialize;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

const HOUR: i64 = 60 * 60;
const DAY: i64 = 24 * HOUR;
/// Activity log entries older than this are dropped by `TrimActivityLog`.
const ACTIVITY_RETENTION_DAYS: i64 = 30;
/// Upper bound on any task's interval.
const MAX_INTERVAL: i64 = 90 * DAY;
const TICK: Duration = Duration::from_secs(30);
/// Let the server finish starting before the first catch-up pass.
const STARTUP_DELAY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Task {
    LibraryScan,
    OptimizeDatabase,
    ClearPreviewCache,
    TrimActivityLog,
    CheckHealth,
    BackupSettings,
    RecheckRequests,
}

/// How long a subtitle request keeps being re-checked automatically. After
/// this it's presumably stuck for a reason a re-check can't fix (the Requests
/// tab still has Re-index and Diagnose).
const RECHECK_WINDOW_DAYS: i64 = 3;
/// At most this many waiting requests are looked at per run.
const RECHECK_BATCH: usize = 50;

impl Task {
    pub const ALL: [Task; 7] = [
        Task::BackupSettings,
        Task::CheckHealth,
        Task::RecheckRequests,
        Task::LibraryScan,
        Task::OptimizeDatabase,
        Task::ClearPreviewCache,
        Task::TrimActivityLog,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Task::LibraryScan => "library_scan",
            Task::OptimizeDatabase => "optimize_database",
            Task::ClearPreviewCache => "clear_preview_cache",
            Task::TrimActivityLog => "trim_activity_log",
            Task::CheckHealth => "check_health",
            Task::BackupSettings => "backup_settings",
            Task::RecheckRequests => "recheck_requests",
        }
    }

    pub fn from_id(id: &str) -> Option<Task> {
        Task::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Task::LibraryScan => "Library scan",
            Task::OptimizeDatabase => "Optimize database",
            Task::ClearPreviewCache => "Clear preview cache",
            Task::TrimActivityLog => "Trim activity log",
            Task::CheckHealth => "Check health",
            Task::BackupSettings => "Back up settings",
            Task::RecheckRequests => "Re-check waiting subtitle requests",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Task::LibraryScan => {
                "Walks every mapped Plex library and indexes new or changed files' subtitles."
            }
            Task::OptimizeDatabase => {
                "VACUUM + FTS5 optimize: reclaims space left behind by reindexing."
            }
            Task::ClearPreviewCache => {
                "Deletes cached preview clips. Skipped while an export is running."
            }
            Task::TrimActivityLog => "Removes Activity log entries older than 30 days.",
            Task::BackupSettings => {
                "Saves your settings, path mappings and schedules (not the index) to a file; keeps the newest 14."
            }
            Task::CheckHealth => {
                "Looks for misconfiguration and failures (Plex, mappings, tools, disk space, updates)."
            }
            Task::RecheckRequests => {
                "Re-indexes items whose requested subtitle is still Waiting (up to 3 days old), so you don't have to press Re-index."
            }
        }
    }

    fn default_interval(self) -> i64 {
        match self {
            Task::LibraryScan => DAY,
            Task::OptimizeDatabase => 7 * DAY,
            Task::ClearPreviewCache => 7 * DAY,
            Task::TrimActivityLog => DAY,
            Task::CheckHealth => 6 * HOUR,
            Task::BackupSettings => DAY,
            Task::RecheckRequests => HOUR,
        }
    }

    /// Whether a freshly created task (first install, or the first boot
    /// after upgrading to a version with Tasks) should run right away. Only
    /// the library scan: an immediate full VACUUM of an existing multi-GB
    /// database on upgrade would be a nasty surprise.
    fn runs_on_first_boot(self) -> bool {
        matches!(self, Task::LibraryScan | Task::CheckHealth)
    }

    /// Runs on every startup regardless of schedule, because its result
    /// lives in memory: without this, the Health panel would sit empty
    /// after a restart until the next interval.
    fn runs_every_boot(self) -> bool {
        self == Task::CheckHealth
    }

    /// Tasks that must never run at the same time as this one. A VACUUM
    /// rewrites the whole database file; doing that while the indexer writes
    /// to it means lock contention at best.
    fn conflicts_with(self) -> &'static [Task] {
        match self {
            Task::LibraryScan => &[Task::OptimizeDatabase],
            Task::OptimizeDatabase => &[Task::LibraryScan],
            _ => &[],
        }
    }

    pub fn min_interval(self) -> i64 {
        match self {
            // A full VACUUM rewrites the whole (multi-GB) database file.
            Task::OptimizeDatabase => DAY,
            Task::CheckHealth | Task::RecheckRequests => 15 * 60,
            _ => HOUR,
        }
    }
}

/// Whether a task is due at `now`, one interval after `baseline` -- its last
/// start, or for a never-run task its creation time. `None` (a never-run
/// task meant to run on first boot) is due immediately.
pub fn is_due(now: i64, baseline: Option<i64>, interval_secs: i64) -> bool {
    baseline.map_or(true, |t| now >= t.saturating_add(interval_secs))
}

/// When a task is next due: `now` if it's already due.
pub fn next_run(now: i64, baseline: Option<i64>, interval_secs: i64) -> i64 {
    match baseline {
        Some(t) => t.saturating_add(interval_secs).max(now),
        None => now,
    }
}

fn baseline(row: &db::TaskRow) -> Option<i64> {
    row.last_started.or(row.created_at)
}

/// `Err` with a human-readable reason if `interval_secs` is out of range.
pub fn validate_interval(task: Task, interval_secs: i64) -> std::result::Result<(), String> {
    let min = task.min_interval();
    if interval_secs < min || interval_secs > MAX_INTERVAL {
        return Err(format!(
            "{}'s interval must be between {} and {} hours",
            task.label(),
            min / HOUR,
            MAX_INTERVAL / HOUR
        ));
    }
    Ok(())
}

/// Create rows for any task that doesn't have one yet. `index_on_start`
/// (the `INDEX_ON_START` env var) only decides whether a *new* library-scan
/// row starts enabled; after that the Tasks page owns the schedule.
pub fn init(conn: &rusqlite::Connection, index_on_start: bool) -> Result<()> {
    let now = now_secs();
    for task in Task::ALL {
        let enabled = task != Task::LibraryScan || index_on_start;
        let created_at = (!task.runs_on_first_boot()).then_some(now);
        db::ensure_task(
            conn,
            task.id(),
            enabled,
            task.default_interval(),
            created_at,
        )?;
    }
    Ok(())
}

/// What a finished run reports.
#[derive(Debug)]
pub enum Outcome {
    /// Did its work: a one-line summary, plus task-specific detail returned
    /// by the synchronous endpoints (e.g. the vacuum's before/after sizes).
    Done {
        summary: String,
        detail: serde_json::Value,
    },
    /// Deliberately didn't run this time (e.g. an export was in progress).
    /// Not an error -- it'll try again next time it's due.
    Skipped(String),
    Failed(String),
}

/// A task's schedule and last outcome, as served by `GET /api/tasks`.
#[derive(Debug, Serialize)]
pub struct TaskInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub enabled: bool,
    pub interval_secs: i64,
    pub min_interval_secs: i64,
    pub max_interval_secs: i64,
    pub running: bool,
    pub last_started: Option<i64>,
    pub last_finished: Option<i64>,
    pub last_duration_ms: Option<i64>,
    pub last_error: Option<String>,
    pub last_summary: Option<String>,
    /// `None` while disabled.
    pub next_run: Option<i64>,
}

pub async fn list(st: &AppState) -> Result<Vec<TaskInfo>> {
    let pool = st.pool.clone();
    let rows = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        db::list_tasks(&conn)
    })
    .await??;
    let now = now_secs();
    let running = st.running_tasks.locked().clone();
    // Serve in Task::ALL order (a stable, meaningful order), not table order.
    Ok(Task::ALL
        .into_iter()
        .filter_map(|task| {
            let row = rows.iter().find(|r| r.name == task.id())?;
            Some(TaskInfo {
                id: task.id(),
                label: task.label(),
                description: task.description(),
                enabled: row.enabled,
                interval_secs: row.interval_secs,
                min_interval_secs: task.min_interval(),
                max_interval_secs: MAX_INTERVAL,
                running: running.contains(&task),
                last_started: row.last_started,
                last_finished: row.last_finished,
                last_duration_ms: row.last_duration_ms,
                last_error: row.last_error.clone(),
                last_summary: row.last_summary.clone(),
                next_run: row
                    .enabled
                    .then(|| next_run(now, baseline(row), row.interval_secs)),
            })
        })
        .collect())
}

/// Start `task` in the background. `Err` says why it can't start right now
/// (already running, or blocked by a conflicting task).
pub fn start(st: &AppState, task: Task) -> std::result::Result<(), String> {
    claim(st, task)?;
    let st = st.clone();
    tokio::spawn(async move {
        execute(&st, task).await;
    });
    Ok(())
}

/// Run `task` now and wait for it. `Err` says why it can't start right now.
pub async fn run_now(st: &AppState, task: Task) -> std::result::Result<Outcome, String> {
    claim(st, task)?;
    Ok(execute(st, task).await)
}

/// Run every enabled task that's due -- once 10s after startup (the
/// catch-up pass), then every 30s.
pub fn spawn_scheduler(st: AppState) {
    tokio::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        let mut tick = tokio::time::interval(TICK);
        let mut first_pass = true;
        loop {
            tick.tick().await;
            if let Err(e) = start_due(&st, first_pass).await {
                warn!("task scheduler pass failed: {e:#}");
            }
            first_pass = false;
        }
    });
}

async fn start_due(st: &AppState, first_pass: bool) -> Result<()> {
    let pool = st.pool.clone();
    let rows = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        db::list_tasks(&conn)
    })
    .await??;
    let now = now_secs();
    for row in rows {
        let Some(task) = Task::from_id(&row.name) else {
            continue;
        };
        let due = is_due(now, baseline(&row), row.interval_secs)
            || (first_pass && task.runs_every_boot());
        if !row.enabled || !due {
            continue;
        }
        // Blocked or already running: it'll be picked up on a later tick.
        if start(st, task).is_ok() {
            info!("scheduled task due, starting: {}", task.id());
        }
    }
    Ok(())
}

/// Mark `task` running, unless it already is or a conflicting task is.
/// One lock covers the check and the insert, so two tasks that conflict can
/// never both win.
fn claim(st: &AppState, task: Task) -> std::result::Result<(), String> {
    let mut running = st.running_tasks.locked();
    if running.contains(&task) {
        return Err(format!("{} is already running", task.label()));
    }
    if let Some(other) = task.conflicts_with().iter().find(|t| running.contains(t)) {
        return Err(format!(
            "{} can't run while {} is running — try again once it finishes",
            task.label(),
            other.label().to_lowercase()
        ));
    }
    running.insert(task);
    Ok(())
}

/// Releases the running claim even if the task future panics or is dropped.
struct Claim<'a> {
    st: &'a AppState,
    task: Task,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.st.running_tasks.locked().remove(&self.task);
    }
}

/// Run an already-claimed task, recording its start and outcome.
async fn execute(st: &AppState, task: Task) -> Outcome {
    let _claim = Claim { st, task };
    let started_ts = now_secs();
    let started = Instant::now();
    persist(st, move |conn| {
        db::record_task_start(conn, task.id(), started_ts)
    })
    .await;

    let outcome = run(st, task).await;

    let duration_ms = started.elapsed().as_millis() as i64;
    let (error, summary) = match &outcome {
        Outcome::Done { summary, .. } => (None, Some(summary.clone())),
        Outcome::Skipped(why) => (None, Some(format!("Skipped — {why}"))),
        Outcome::Failed(e) => {
            warn!("task {} failed: {e}", task.id());
            st.log_event(
                LogLevel::Error,
                format!("Scheduled task '{}' failed: {e}", task.label()),
            );
            (Some(e.clone()), None)
        }
    };
    let finished_ts = now_secs();
    persist(st, move |conn| {
        db::record_task_finish(
            conn,
            task.id(),
            finished_ts,
            duration_ms,
            error.as_deref(),
            summary.as_deref(),
        )
    })
    .await;
    outcome
}

/// The task's actual work.
async fn run(st: &AppState, task: Task) -> Outcome {
    match task {
        Task::LibraryScan => library_scan(st).await,
        Task::OptimizeDatabase => optimize_database(st).await,
        Task::ClearPreviewCache => clear_preview_cache(st).await,
        Task::TrimActivityLog => trim_activity_log(st).await,
        Task::CheckHealth => check_health(st).await,
        Task::BackupSettings => backup_settings(st).await,
        Task::RecheckRequests => recheck_requests(st).await,
    }
}

/// Re-index the items of recent subtitle requests that are still Waiting. The
/// per-item work is the same as the Re-index button (including reading the
/// subtitle Plex keeps); the Activity log only hears about the ones that
/// actually produced subtitle lines, so this doesn't add a line per item per run.
async fn recheck_requests(st: &AppState) -> Outcome {
    if !st.settings().configured() {
        return Outcome::Skipped("The media server isn't configured yet".into());
    }
    if st.index.locked().running {
        return Outcome::Skipped("A library scan is running; it will pick these up".into());
    }
    let pool = st.pool.clone();
    let cutoff = now_secs() - RECHECK_WINDOW_DAYS * DAY;
    let waiting = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
        let conn = pool.get()?;
        let mut keys: Vec<String> = Vec::new();
        for r in db::list_subtitle_requests(&conn, 500)? {
            if r.state == "waiting" && r.requested_at >= cutoff && !keys.contains(&r.rating_key) {
                keys.push(r.rating_key);
            }
        }
        keys.truncate(RECHECK_BATCH);
        Ok(keys)
    })
    .await;
    let waiting = match waiting {
        Ok(Ok(w)) => w,
        Ok(Err(e)) => return Outcome::Failed(format!("{e:#}")),
        Err(e) => return Outcome::Failed(format!("re-check task panicked: {e}")),
    };
    if waiting.is_empty() {
        return Outcome::Done {
            summary: "no waiting requests".into(),
            detail: serde_json::json!({ "checked": 0, "indexed": 0, "errors": 0 }),
        };
    }
    let (mut indexed, mut errors) = (0usize, 0usize);
    let mut last_error = None;
    for rk in &waiting {
        match index::index_item_quietly(st, rk).await {
            Ok(Some(r)) if r.lines > 0 => indexed += 1,
            Ok(_) => {}
            Err(e) => {
                warn!("re-check of request for {rk} failed: {e:#}");
                errors += 1;
                last_error = Some(format!("{e:#}"));
            }
        }
    }
    if errors == waiting.len() {
        return Outcome::Failed(last_error.unwrap_or_default());
    }
    Outcome::Done {
        summary: format!(
            "{} checked, {indexed} now indexed, {errors} error(s)",
            waiting.len()
        ),
        detail: serde_json::json!({ "checked": waiting.len(), "indexed": indexed, "errors": errors }),
    }
}

async fn backup_settings(st: &AppState) -> Outcome {
    let pool = st.pool.clone();
    let dir = st.config.backup_dir();
    let version = st.config.app_version.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        crate::backup::create(&conn, &dir, now_secs(), &version)
    })
    .await;
    match result {
        Ok(Ok(name)) => Outcome::Done {
            summary: format!("saved {name}"),
            detail: serde_json::json!({ "name": name }),
        },
        Ok(Err(e)) => Outcome::Failed(format!("{e:#}")),
        Err(e) => Outcome::Failed(format!("backup task panicked: {e}")),
    }
}

async fn check_health(st: &AppState) -> Outcome {
    let issues = crate::health::run(st).await;
    let count = |l: crate::health::Level| issues.iter().filter(|i| i.level == l).count();
    let (errors, warnings) = (
        count(crate::health::Level::Error),
        count(crate::health::Level::Warning),
    );
    let summary = if errors + warnings == 0 {
        "no problems found".to_string()
    } else {
        format!("{errors} error(s), {warnings} warning(s)")
    };
    *st.health.write_locked() = crate::health::Report {
        checked_at: Some(now_secs()),
        issues,
    };
    Outcome::Done {
        summary,
        detail: serde_json::json!({ "errors": errors, "warnings": warnings }),
    }
}

async fn library_scan(st: &AppState) -> Outcome {
    if !st.settings().configured() {
        return Outcome::Skipped("The media server isn't configured yet".into());
    }
    index::run(st.clone()).await;
    let status = st.index.locked().clone();
    match status.last_error {
        Some(e) => Outcome::Failed(e),
        None => Outcome::Done {
            summary: format!(
                "{} indexed, {} unchanged, {} errors",
                status.done, status.skipped, status.errors
            ),
            detail: serde_json::json!({
                "done": status.done,
                "skipped": status.skipped,
                "errors": status.errors,
            }),
        },
    }
}

async fn optimize_database(st: &AppState) -> Outcome {
    if st.index.locked().running {
        return Outcome::Skipped("a library scan is running".into());
    }
    st.log_event(
        LogLevel::Info,
        "Vacuum & optimize started (this may take a while)",
    );
    let pool = st.pool.clone();
    let db_path = st.config.db_path();
    let result = tokio::task::spawn_blocking(move || -> Result<db::VacuumResult> {
        let conn = pool.get()?;
        db::vacuum_and_optimize(&conn, &db_path)
    })
    .await;
    match result {
        Ok(Ok(r)) => {
            let summary = format!(
                "freed {} in {:.1}s",
                fmt_bytes(r.saved_bytes),
                r.duration_ms as f64 / 1000.0
            );
            st.log_event(
                LogLevel::Info,
                format!("Vacuum & optimize finished — {summary}"),
            );
            Outcome::Done {
                summary,
                detail: serde_json::to_value(&r).unwrap_or_default(),
            }
        }
        Ok(Err(e)) => Outcome::Failed(format!("{e:#}")),
        Err(e) => Outcome::Failed(format!("vacuum task panicked: {e}")),
    }
}

async fn clear_preview_cache(st: &AppState) -> Outcome {
    // In-progress exports write their caption files and sequence parts to
    // the same temp dir -- clearing it mid-export would break them.
    let exporting = st
        .jobs
        .locked()
        .values()
        .any(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running));
    if exporting {
        return Outcome::Skipped("an export is in progress".into());
    }
    let tmp_dir = st.config.tmp_dir();
    let (bytes, files) = tokio::task::spawn_blocking(move || diskstats::clear_dir(&tmp_dir))
        .await
        .unwrap_or((0, 0));
    let summary = format!("cleared {} ({files} files)", fmt_bytes(bytes as i64));
    st.log_event(LogLevel::Info, format!("Cleared preview cache: {summary}"));
    Outcome::Done {
        summary,
        detail: serde_json::json!({ "bytes": bytes, "files": files }),
    }
}

async fn trim_activity_log(st: &AppState) -> Outcome {
    let cutoff = now_secs() - ACTIVITY_RETENTION_DAYS * DAY;
    let pool = st.pool.clone();
    match tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        db::prune_log_before(&conn, cutoff)
    })
    .await
    {
        Ok(Ok(n)) => Outcome::Done {
            summary: format!("removed {n} entries older than {ACTIVITY_RETENTION_DAYS} days"),
            detail: serde_json::json!({ "removed": n }),
        },
        Ok(Err(e)) => Outcome::Failed(format!("{e:#}")),
        Err(e) => Outcome::Failed(format!("trim task panicked: {e}")),
    }
}

/// Run a small DB write off the async runtime; failures are logged, not
/// fatal -- losing a task's bookkeeping must never fail the task itself.
async fn persist(
    st: &AppState,
    f: impl FnOnce(&rusqlite::Connection) -> Result<()> + Send + 'static,
) {
    let pool = st.pool.clone();
    let result = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;
        f(&conn)
    })
    .await;
    if let Ok(Err(e)) | Err(e) = result.map_err(anyhow::Error::from) {
        warn!("couldn't record task state: {e:#}");
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_run_tasks_are_due_immediately() {
        assert!(is_due(1_000, None, DAY));
        assert_eq!(next_run(1_000, None, DAY), 1_000);
    }

    #[test]
    fn a_task_is_due_once_its_interval_has_passed() {
        let last = 10_000;
        assert!(!is_due(last + HOUR - 1, Some(last), HOUR));
        assert!(is_due(last + HOUR, Some(last), HOUR));
        assert_eq!(next_run(last + 10, Some(last), HOUR), last + HOUR);
    }

    // A restart shortly after a scan must not look like a missed run.
    #[test]
    fn a_restart_right_after_a_run_is_not_due() {
        let last_scan = 50_000;
        let restarted_at = last_scan + 5 * 60;
        assert!(!is_due(restarted_at, Some(last_scan), DAY));
    }

    #[test]
    fn an_overdue_task_reports_now_as_its_next_run() {
        assert_eq!(next_run(100 * DAY, Some(0), DAY), 100 * DAY);
    }

    // On the first boot after upgrading, a brand-new optimize task must not
    // VACUUM the existing database right away -- it's due one interval
    // after the row was created. Only the library scan runs immediately.
    #[test]
    fn new_tasks_wait_one_interval_except_scan_and_health() {
        let pool = db::test_pool();
        let conn = pool.get().unwrap();
        init(&conn, true).unwrap();
        let now = now_secs();
        for row in db::list_tasks(&conn).unwrap() {
            let due = is_due(now, baseline(&row), row.interval_secs);
            let runs_now = row.name == "library_scan" || row.name == "check_health";
            assert_eq!(due, runs_now, "{} due={due}", row.name);
        }
    }

    #[test]
    fn scan_and_optimize_block_each_other_but_not_unrelated_tasks() {
        let mut running = std::collections::HashSet::new();
        running.insert(Task::LibraryScan);
        let blocked = |t: Task| t.conflicts_with().iter().any(|c| running.contains(c));
        assert!(blocked(Task::OptimizeDatabase));
        assert!(!blocked(Task::ClearPreviewCache));
        assert!(!blocked(Task::TrimActivityLog));
        assert!(Task::OptimizeDatabase
            .conflicts_with()
            .contains(&Task::LibraryScan));
    }

    #[test]
    fn only_the_health_check_reruns_on_every_boot() {
        for t in Task::ALL {
            assert_eq!(t.runs_every_boot(), t == Task::CheckHealth, "{}", t.id());
        }
    }

    #[test]
    fn interval_bounds_are_enforced_per_task() {
        assert!(validate_interval(Task::LibraryScan, HOUR).is_ok());
        assert!(validate_interval(Task::LibraryScan, HOUR - 1).is_err());
        assert!(validate_interval(Task::OptimizeDatabase, HOUR).is_err());
        assert!(validate_interval(Task::OptimizeDatabase, DAY).is_ok());
        assert!(validate_interval(Task::TrimActivityLog, MAX_INTERVAL + 1).is_err());
    }

    #[test]
    fn ids_round_trip() {
        for t in Task::ALL {
            assert_eq!(Task::from_id(t.id()), Some(t));
        }
        assert_eq!(Task::from_id("nope"), None);
    }

    #[test]
    fn init_respects_index_on_start_for_a_new_scan_row_only() {
        let pool = db::test_pool();
        let conn = pool.get().unwrap();
        init(&conn, false).unwrap();
        let rows = db::list_tasks(&conn).unwrap();
        assert_eq!(rows.len(), Task::ALL.len());
        let scan = rows.iter().find(|r| r.name == "library_scan").unwrap();
        assert!(!scan.enabled);
        assert!(rows
            .iter()
            .filter(|r| r.name != "library_scan")
            .all(|r| r.enabled));
        // A later start with INDEX_ON_START=true doesn't override the row.
        init(&conn, true).unwrap();
        let scan = db::list_tasks(&conn)
            .unwrap()
            .into_iter()
            .find(|r| r.name == "library_scan")
            .unwrap();
        assert!(!scan.enabled);
    }
}
