//! Health, status, metrics, activity and server logs, storage, backups, and scheduled tasks.

use super::*;
use crate::sync::{MutexExt, RwLockExt};

pub(super) async fn healthz() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

/// Lightweight "is it alive" check for uptime monitors — unlike `/healthz`
/// (a bare `{"ok":true}`, meant for things like Docker's own HEALTHCHECK),
/// this carries just enough to be useful on a status page at a glance:
/// version, how long this process has been up, and whether an index pass
/// is currently running. No DB query — in-memory only, so it stays fast
/// even if SQLite were somehow stuck.
pub(super) async fn status(State(st): State<AppState>) -> impl IntoResponse {
    let index_running = st.index.locked().running;
    Json(json!({
        "status": "ok",
        "version": st.config.app_version,
        "uptime_seconds": st.uptime_secs(),
        "index_running": index_running,
    }))
}

/// Current build version and whether a newer GitHub release tag exists.
pub(super) async fn version(State(st): State<AppState>) -> impl IntoResponse {
    let current = st.config.app_version.clone();
    let repo = st.config.github_repo.clone();
    let repo_url = format!("https://github.com/{repo}");
    let releases_url = format!("{repo_url}/releases");

    let (latest, update_available, checked, error) =
        match crate::version::latest_tag(&repo, st.config.github_token.as_deref()).await {
            Ok(Some(l)) => {
                let newer = crate::version::is_newer(&l, &current);
                (Some(l), newer, true, None)
            }
            Ok(None) => (None, false, true, None),
            // Surface the real reason (e.g. "GitHub returned 404 Not Found" --
            // the usual symptom of a private repo with no/invalid
            // GITHUB_TOKEN) instead of silently looking identical to "checked,
            // nothing new" from the UI's point of view.
            Err(e) => (None, false, false, Some(format!("{e:#}"))),
        };

    Json(json!({
        "current": current,
        "latest": latest,
        "update_available": update_available,
        "checked": checked,
        "error": error,
        "repo_url": repo_url,
        "releases_url": releases_url,
    }))
}

/// Prometheus text-format metrics. Public, same as `/healthz` — Prometheus
/// scraping doesn't carry a session cookie, and this is meant to sit on the
/// same trusted LAN as this app's other exporters (sonarr-exporter etc.),
/// not be reachable from outside it. Hand-rolled rather than pulling in the
/// `prometheus` crate's registry machinery for what's really just a handful
/// of point-in-time gauges read fresh on every scrape.
pub(super) async fn metrics(State(st): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let db_path = st.config.db_path();
    let (stats, usage) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        Ok((db::stats(&conn)?, db::db_usage(&conn, &db_path)?))
    })
    .await??;

    let index_status = st.index.locked().clone();
    let jobs: Vec<Job> = st.jobs.locked().values().cloned().collect();
    let compact = st.compact.read_locked();

    let mut out = String::new();

    out.push_str("# HELP meme_arr_build_info Build version (always 1; version is a label).\n");
    out.push_str("# TYPE meme_arr_build_info gauge\n");
    out.push_str(&format!(
        "meme_arr_build_info{{version=\"{}\"}} 1\n",
        st.config.app_version
    ));

    push_gauge(
        &mut out,
        "meme_arr_media_indexed_total",
        "Media items (movies/episodes) indexed.",
        stats.media_count as f64,
    );
    push_gauge(
        &mut out,
        "meme_arr_subtitle_lines_total",
        "Subtitle lines indexed for dialogue search.",
        stats.cue_count as f64,
    );

    push_gauge(
        &mut out,
        "meme_arr_index_running",
        "Whether a library index pass is currently running (0/1).",
        if index_status.running { 1.0 } else { 0.0 },
    );
    push_gauge(
        &mut out,
        "meme_arr_index_last_run_done",
        "Items indexed in the most recently completed pass.",
        index_status.done as f64,
    );
    push_gauge(
        &mut out,
        "meme_arr_index_last_run_skipped",
        "Items skipped (unchanged) in the most recently completed pass.",
        index_status.skipped as f64,
    );
    push_gauge(
        &mut out,
        "meme_arr_index_last_run_errors",
        "Items that failed to index in the most recently completed pass.",
        index_status.errors as f64,
    );
    if let Some(ts) = index_status.finished_at {
        push_gauge(
            &mut out,
            "meme_arr_index_last_run_timestamp_seconds",
            "Unix time the most recently completed index pass finished.",
            ts as f64,
        );
    }

    out.push_str("# HELP meme_arr_db_bytes Database disk usage by component.\n");
    out.push_str("# TYPE meme_arr_db_bytes gauge\n");
    for (component, value) in [
        ("file", usage.db_file_bytes),
        ("wal", usage.wal_bytes),
        ("subtitle_text", usage.subtitle_text_bytes),
        ("media_text", usage.media_text_bytes),
        ("index_overhead", usage.index_overhead_bytes),
    ] {
        out.push_str(&format!(
            "meme_arr_db_bytes{{component=\"{component}\"}} {value}\n"
        ));
    }

    out.push_str(
        "# HELP meme_arr_export_jobs Export jobs tracked by this running process, by status.\n",
    );
    out.push_str("# TYPE meme_arr_export_jobs gauge\n");
    for status in ["queued", "running", "done", "failed"] {
        let count = jobs
            .iter()
            .filter(|j| job_status_str(j.status) == status)
            .count();
        out.push_str(&format!(
            "meme_arr_export_jobs{{status=\"{status}\"}} {count}\n"
        ));
    }

    // The research compact dialogue index (see `compactindex`) -- shadow
    // mode only, but its own size/shape is worth watching if this ever
    // moves toward production use.
    if let Some(c) = compact.as_ref() {
        push_gauge(
            &mut out,
            "meme_arr_compact_index_rows",
            "Rows in the research compact dialogue index (shadow mode).",
            c.row_count() as f64,
        );
        push_gauge(
            &mut out,
            "meme_arr_compact_index_terms",
            "Distinct terms in the research compact dialogue index (shadow mode).",
            c.term_count() as f64,
        );
        push_gauge(
            &mut out,
            "meme_arr_compact_index_bytes",
            "Byte size of the research compact dialogue index (shadow mode).",
            c.size_bytes() as f64,
        );
    }

    Ok((
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    ))
}

pub(super) fn push_gauge(out: &mut String, name: &str, help: &str, value: f64) {
    out.push_str(&format!(
        "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}\n"
    ));
}

pub(super) fn job_status_str(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

pub(super) async fn index_status(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let status = st.index.locked().clone();
    let pool = st.pool.clone();
    let stats = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::stats(&conn)
    })
    .await??;
    Ok(Json(json!({ "status": status, "stats": stats })))
}

#[derive(Debug, Deserialize)]
pub(super) struct LogParams {
    #[serde(default = "log_limit")]
    limit: usize,
}

pub(super) fn log_limit() -> usize {
    200
}

/// Recent activity-log entries (indexing start/finish, skipped libraries,
/// errors), newest first. Persisted in SQLite, so this survives a restart —
/// no need to tail `docker logs` to see history from before the app last
/// came up.
pub(super) async fn activity_log(
    State(st): State<AppState>,
    Query(p): Query<LogParams>,
) -> Result<impl IntoResponse, AppError> {
    let limit = p.limit.clamp(1, 500);
    let entries = tokio::task::spawn_blocking(move || st.log_entries(limit)).await?;
    Ok(Json(json!({ "entries": entries })))
}

/// Wipe the activity log (the Activity tab's "Clear" button).
pub(super) async fn clear_activity_log(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    tokio::task::spawn_blocking(move || st.clear_log()).await??;
    Ok(Json(json!({ "ok": true })))
}

/// Disk usage: the dialogue index database (with a text-vs-overhead
/// breakdown), the on-demand preview cache, and exported memes.
pub(super) async fn system_storage(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let db_path = st.config.db_path();
    let db = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::db_usage(&conn, &db_path)
    })
    .await??;

    let tmp_dir = st.config.tmp_dir();
    let output_dir = st.config.output_dir.clone();
    let (preview_cache, exports) = tokio::task::spawn_blocking(move || {
        let (bytes, files) = diskstats::dir_size(&tmp_dir);
        let (out_bytes, out_files) = diskstats::dir_size(&output_dir);
        ((bytes, files), (out_bytes, out_files))
    })
    .await?;

    Ok(Json(json!({
        "database": db,
        "preview_cache": { "bytes": preview_cache.0, "files": preview_cache.1 },
        "exports": { "bytes": exports.0, "files": exports.1 },
    })))
}

/// List exported memes still sitting in `OUTPUT_DIR`, newest first — lets
/// the System tab browse and re-download past exports instead of only
/// seeing them in the current browser session's (reload-loses-it) job list.
pub(super) async fn list_exports(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let output_dir = st.config.output_dir.clone();
    let files = tokio::task::spawn_blocking(move || diskstats::list_files(&output_dir)).await?;
    Ok(Json(json!({ "files": files })))
}

/// Deep-clean the dialogue index: merge FTS5's internal segments and
/// physically compact the database file. Refuses to run while an index pass
/// is in progress (both would be fighting over the same connection pool, and
/// a reindex is exactly when the free space this reclaims gets created).
pub(super) async fn system_vacuum(State(st): State<AppState>) -> Response {
    sync_task_response(tasks::run_now(&st, Task::OptimizeDatabase).await)
}

/// Delete everything in the on-demand preview cache. Safe any time — previews
/// regenerate on request — so this needs no confirmation beyond the button
/// press itself.
pub(super) async fn clear_preview_cache(State(st): State<AppState>) -> Response {
    sync_task_response(tasks::run_now(&st, Task::ClearPreviewCache).await)
}

/// The System tab's maintenance buttons wait for their task and show its
/// result: the task's detail on success, 409 if it's already running or was
/// skipped (e.g. "an export is in progress"), 500 if it failed.
pub(super) fn sync_task_response(outcome: Result<tasks::Outcome, String>) -> Response {
    match outcome {
        Err(why) => (StatusCode::CONFLICT, Json(json!({ "error": why }))).into_response(),
        Ok(tasks::Outcome::Done { detail, .. }) => Json(detail).into_response(),
        Ok(tasks::Outcome::Skipped(why)) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("skipped: {why}") })),
        )
            .into_response(),
        Ok(tasks::Outcome::Failed(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

pub(super) async fn list_backups(State(st): State<AppState>) -> impl IntoResponse {
    let dir = st.config.backup_dir();
    let files = tokio::task::spawn_blocking(move || crate::backup::list(&dir))
        .await
        .unwrap_or_default();
    Json(json!({ "backups": files, "keep": crate::backup::KEEP }))
}

pub(super) async fn create_backup(State(st): State<AppState>) -> Response {
    sync_task_response(tasks::run_now(&st, Task::BackupSettings).await)
}

pub(super) async fn download_backup(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> Response {
    if !crate::backup::is_valid_name(&name) {
        return not_found("no such backup");
    }
    match tokio::fs::read(st.config.backup_dir().join(&name)).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/json".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{name}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => not_found("no such backup"),
    }
}

/// Replace the app's configuration with an uploaded backup file.
pub(super) async fn restore_backup(
    State(st): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let file = match crate::backup::parse(&body) {
        Ok(f) => f,
        Err(e) => return Ok(bad_request(format!("{e:#}"))),
    };
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut conn = pool.get()?;
        crate::backup::restore(&mut conn, &file)
    })
    .await??;
    st.reload_settings()?;
    st.log_event(LogLevel::Warn, "Settings restored from a backup file");
    let _ = tasks::start(&st, Task::CheckHealth);
    Ok(Json(json!({ "ok": true })).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct ServerLogParams {
    #[serde(default = "default_log_min_level")]
    level: String,
    #[serde(default)]
    q: String,
    #[serde(default)]
    after: Option<u64>,
    #[serde(default = "default_log_page")]
    limit: usize,
}

pub(super) fn default_log_min_level() -> String {
    "info".into()
}

pub(super) fn default_log_page() -> usize {
    300
}

/// Recent server log output, oldest first. `level` is the minimum severity
/// to show, `q` a text filter, `after` an entry id (return only newer ones).
pub(super) async fn get_logs(
    State(st): State<AppState>,
    Query(p): Query<ServerLogParams>,
) -> impl IntoResponse {
    let entries = st
        .logs
        .query(&p.level, &p.q, p.after, p.limit.clamp(1, 2000));
    Json(json!({ "entries": entries, "level": st.logs.level() }))
}

pub(super) async fn get_log_level(State(st): State<AppState>) -> impl IntoResponse {
    Json(json!({ "level": st.logs.level(), "options": crate::logs::LEVELS }))
}

#[derive(Debug, Deserialize)]
pub(super) struct LogLevelBody {
    level: String,
}

pub(super) async fn put_log_level(
    State(st): State<AppState>,
    Json(body): Json<LogLevelBody>,
) -> Result<Response, AppError> {
    if let Err(msg) = st.logs.set_level(&body.level) {
        return Ok(bad_request(msg));
    }
    let pool = st.pool.clone();
    let level = body.level.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::set_setting(&conn, "log_level", &level)
    })
    .await??;
    st.log_event(LogLevel::Info, format!("Log level set to {}", body.level));
    Ok(Json(json!({ "ok": true, "level": body.level })).into_response())
}

pub(super) async fn health_report(State(st): State<AppState>) -> impl IntoResponse {
    let report = st.health.read_locked().clone();
    let running = st.running_tasks.locked().contains(&Task::CheckHealth);
    Json(json!({
        "checked_at": report.checked_at,
        "issues": report.issues,
        "running": running,
    }))
}

/// Re-run the health checks now and return the fresh report ("Re-check" in
/// the Health panel).
pub(super) async fn health_check_now(State(st): State<AppState>) -> Response {
    if tasks::run_now(&st, Task::CheckHealth).await.is_err() {
        // One is already running (saving settings starts one). Wait for it
        // instead of failing, so the caller gets a report that reflects
        // what it just changed, not the stale one from before.
        for _ in 0..120 {
            if !st.running_tasks.locked().contains(&Task::CheckHealth) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    health_report(State(st)).await.into_response()
}

pub(super) async fn list_tasks(State(st): State<AppState>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(tasks::list(&st).await?))
}

pub(super) async fn run_task(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(task) = Task::from_id(&id) else {
        return not_found("no such task");
    };
    match tasks::start(&st, task) {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({ "started": true }))).into_response(),
        Err(why) => (StatusCode::CONFLICT, Json(json!({ "error": why }))).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskSchedule {
    enabled: bool,
    interval_secs: i64,
}

pub(super) async fn update_task(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TaskSchedule>,
) -> Result<Response, AppError> {
    let Some(task) = Task::from_id(&id) else {
        return Ok(not_found("no such task"));
    };
    if let Err(msg) = tasks::validate_interval(task, body.interval_secs) {
        return Ok(bad_request(msg));
    }
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::set_task_schedule(&conn, task.id(), body.enabled, body.interval_secs)
    })
    .await??;
    Ok(Json(json!({ "ok": true })).into_response())
}

/// Delete every exported meme in `/output`. Unlike the preview cache these are
/// the user's actual deliverables — destructive and irreversible; the
/// confirmation warning lives in the UI, same pattern as deleting Settings.
pub(super) async fn clear_exports(State(st): State<AppState>) -> impl IntoResponse {
    let output_dir = st.config.output_dir.clone();
    let (bytes, files) = tokio::task::spawn_blocking(move || diskstats::clear_dir(&output_dir))
        .await
        .unwrap_or((0, 0));
    st.log_event(
        crate::state::LogLevel::Warn,
        format!(
            "Cleared exported memes: {} ({files} files)",
            fmt_bytes(bytes as i64)
        ),
    );
    Json(json!({ "bytes": bytes, "files": files }))
}
