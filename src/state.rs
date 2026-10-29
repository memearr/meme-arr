//! Shared application state and the in-memory status types for the background
//! indexer and export jobs.

use crate::compactindex::CompactIndex;
use crate::config::{Config, PathRewrite};
use crate::db::{self, Pool};
use crate::export::Format;
use crate::jellyfin::JellyfinClient;
use crate::mediaserver::{MediaServer, ServerKind};
use crate::plex::PlexClient;
use crate::ratelimit::LoginLimiter;
use crate::sync::{MutexExt, RwLockExt};
use crate::tasks::Task;
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

/// Live media-server configuration, sourced from the Settings page (persisted
/// in the DB). Both servers' credentials are kept so switching back and forth
/// doesn't lose them; `server` says which one is in use.
#[derive(Clone, Default)]
pub struct AppSettings {
    pub server: ServerKind,
    pub jellyfin_url: Option<String>,
    pub jellyfin_api_key: Option<String>,
    pub plex_url: Option<String>,
    pub plex_token: Option<String>,
    pub mappings: Vec<PathRewrite>,
    /// Also write a subtitle read from the server as a `.srt` beside the
    /// video. Off by default: it needs a writable media folder.
    pub save_fetched_subtitles: bool,
}

impl AppSettings {
    /// Whether the *selected* media server has everything it needs.
    pub fn configured(&self) -> bool {
        let set = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.is_empty());
        match self.server {
            ServerKind::Plex => set(&self.plex_url) && set(&self.plex_token),
            ServerKind::Jellyfin => set(&self.jellyfin_url) && set(&self.jellyfin_api_key),
        }
    }

    /// Load settings from the database.
    pub fn load(conn: &rusqlite::Connection) -> Result<Self> {
        let plex_url = db::get_setting(conn, "plex_url")?;
        let plex_token = db::get_setting(conn, "plex_token")?;
        let server = ServerKind::parse(db::get_setting(conn, "media_server")?.as_deref());
        let jellyfin_url = db::get_setting(conn, "jellyfin_url")?;
        let jellyfin_api_key = db::get_setting(conn, "jellyfin_api_key")?;
        let save_fetched_subtitles =
            db::get_setting(conn, "save_fetched_subtitles")?.as_deref() == Some("1");
        let mappings = db::list_mappings(conn)?
            .into_iter()
            .map(|m| PathRewrite {
                from: m.plex_prefix,
                to: m.container_path,
            })
            .collect();
        Ok(Self {
            server,
            jellyfin_url,
            jellyfin_api_key,
            plex_url,
            plex_token,
            mappings,
            save_fetched_subtitles,
        })
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Pool,
    pub settings: Arc<RwLock<AppSettings>>,
    pub index: Arc<Mutex<IndexStatus>>,
    pub jobs: Arc<Mutex<HashMap<String, Job>>>,
    /// The research compact dialogue index (see `compactindex`), loaded or
    /// rebuilt in the background — `None` until the first build/load
    /// completes. Used only for a shadow-mode correctness check in the
    /// search handler today; never affects what a user actually sees.
    pub compact: Arc<RwLock<Option<CompactIndex>>>,
    /// When this process actually started (captured once in `lib::run`,
    /// not lazily on first use) — backs `/status`'s `uptime_seconds`.
    pub started_at: Instant,
    /// Brute-force protection for `POST /api/auth/login`.
    pub login_limiter: Arc<LoginLimiter>,
    /// Scheduled tasks running right now (see `tasks`); their schedules and
    /// last outcomes live in the database.
    pub running_tasks: Arc<Mutex<HashSet<Task>>>,
    /// The latest health-check report (System -> Health, and the nav badge).
    /// In-memory: the Check health task reruns on every boot.
    pub health: Arc<RwLock<crate::health::Report>>,
    /// Recent log output and the runtime log level (System -> Logs).
    pub logs: Arc<crate::logs::LogControl>,
}

impl AppState {
    /// Seconds since this process started.
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Snapshot the current settings.
    pub fn settings(&self) -> AppSettings {
        self.settings.read_locked().clone()
    }

    /// Current path-mapping rules.
    pub fn mappings(&self) -> Vec<PathRewrite> {
        self.settings.read_locked().mappings.clone()
    }

    /// The configured media server (Plex or Jellyfin), or an explanation of
    /// what's missing.
    pub fn media_server(&self) -> Result<MediaServer> {
        let s = self.settings.read_locked();
        let val = |v: &Option<String>| v.clone().filter(|x| !x.is_empty());
        let missing = || {
            anyhow!(
                "{} is not configured — set it up in Settings",
                s.server.label()
            )
        };
        match s.server {
            ServerKind::Plex => match (val(&s.plex_url), val(&s.plex_token)) {
                (Some(u), Some(t)) => Ok(MediaServer::Plex(PlexClient::new(u, t))),
                _ => Err(missing()),
            },
            ServerKind::Jellyfin => match (val(&s.jellyfin_url), val(&s.jellyfin_api_key)) {
                (Some(u), Some(k)) => Ok(MediaServer::Jellyfin(JellyfinClient::new(u, k))),
                _ => Err(missing()),
            },
        }
    }

    /// Reload settings from the DB into the in-memory snapshot.
    pub fn reload_settings(&self) -> Result<()> {
        let conn = self.pool.get()?;
        let fresh = AppSettings::load(&conn)?;
        *self.settings.write_locked() = fresh;
        Ok(())
    }

    pub fn set_job(&self, job: Job) {
        self.jobs.locked().insert(job.id.clone(), job);
    }

    pub fn update_job(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(j) = self.jobs.locked().get_mut(id) {
            f(j);
        }
    }

    pub fn get_job(&self, id: &str) -> Option<Job> {
        self.jobs.locked().get(id).cloned()
    }

    /// Record an activity-log entry. Persisted to SQLite (`activity_log`
    /// table) on a blocking-pool thread so a caller in an async context
    /// never stalls on the write — this is fire-and-forget, matching the
    /// old in-memory version's "best effort" semantics, but now the data
    /// actually survives a restart instead of resetting every redeploy.
    pub fn log_event(&self, level: LogLevel, message: impl Into<String>) {
        let message = message.into();
        let ts = now_secs();
        let pool = self.pool.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn_blocking(move || {
                    if let Ok(conn) = pool.get() {
                        if let Err(e) = db::insert_log_entry(&conn, ts, level.as_str(), &message) {
                            warn!("failed to persist activity-log entry: {e:#}");
                        }
                    }
                });
            }
            Err(_) => warn!("log_event called with no tokio runtime available, dropping entry"),
        }
    }

    /// Recent activity log entries, newest first. Blocking (real SQLite
    /// I/O) — callers in an async context should wrap this in
    /// `spawn_blocking`, same as any other DB read on `AppState`.
    pub fn log_entries(&self, limit: usize) -> Vec<LogEntry> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        db::recent_log_entries(&conn, limit)
            .unwrap_or_default()
            .into_iter()
            .map(|(ts, level, message)| LogEntry {
                ts,
                level: LogLevel::parse(&level),
                message,
            })
            .collect()
    }

    /// Wipe the activity log (the Activity tab's "Clear" action). Blocking,
    /// same caller contract as `log_entries`.
    pub fn clear_log(&self) -> Result<()> {
        let conn = self.pool.get()?;
        db::clear_log(&conn)
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    /// Defensive, not fallible: an unrecognized stored value (there
    /// shouldn't be one — `as_str` is the only writer) falls back to
    /// `Info` rather than dropping the row.
    fn parse(s: &str) -> Self {
        match s {
            "warn" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts: i64,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct IndexStatus {
    pub running: bool,
    pub total: usize,
    pub done: usize,
    pub skipped: usize,
    pub errors: usize,
    pub current: Option<String>,
    /// Unix seconds of the last completed run.
    pub finished_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub format: Format,
    pub filename: String,
    /// Whether this export included its source audio -- the frontend uses
    /// this to decide whether the result preview should play unmuted.
    pub audio: bool,
    /// Relative download path served by the API when done.
    pub download_path: Option<String>,
    pub error: Option<String>,
}
