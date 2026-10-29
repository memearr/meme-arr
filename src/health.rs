//! Health checks (System -> Health), modeled on Sonarr/Radarr's: a scheduled
//! pass that looks for known misconfigurations and failures, caches the
//! result for the UI and the nav badge, and links each issue to its fix in
//! `docs/HEALTH.md`.
//!
//! Every check is cheap except two network calls (Plex and the GitHub update
//! check), which get hard timeouts so an unreachable host can't stall a pass.

use crate::config::PathRewrite;
use crate::diskstats::fmt_bytes;
use crate::state::AppState;
use crate::sync::MutexExt;
use crate::version;
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NETWORK_TIMEOUT: Duration = Duration::from_secs(10);
const TOOL_TIMEOUT: Duration = Duration::from_secs(10);
/// Warn below this much free space, or below `LOW_SPACE_FRACTION` of the
/// volume -- whichever is larger.
const LOW_SPACE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const LOW_SPACE_FRACTION: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Issue {
    /// Which check fired; also the anchor of its section in docs/HEALTH.md.
    pub kind: &'static str,
    pub level: Level,
    pub message: String,
    pub fix: String,
    pub docs_url: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    /// Unix seconds; `None` until the first pass after startup finishes.
    pub checked_at: Option<i64>,
    /// Most severe first.
    pub issues: Vec<Issue>,
}

/// Builds issues with the right docs link for this deployment's repo.
struct Issues {
    docs_base: String,
    list: Vec<Issue>,
}

impl Issues {
    fn new(github_repo: &str) -> Self {
        Issues {
            docs_base: format!("https://github.com/{github_repo}/blob/main/docs/HEALTH.md"),
            list: Vec::new(),
        }
    }

    fn push(&mut self, kind: &'static str, level: Level, message: String, fix: impl Into<String>) {
        self.list.push(Issue {
            kind,
            level,
            message,
            fix: fix.into(),
            docs_url: format!("{}#{kind}", self.docs_base),
        });
    }
}

/// Run every check and return the issues, most severe first.
pub async fn run(st: &AppState) -> Vec<Issue> {
    let cfg = &st.config;
    let mut out = Issues::new(&cfg.github_repo);

    tool_checks(
        &mut out,
        &cfg.ffmpeg_bin,
        &cfg.ffprobe_bin,
        &cfg.gifsicle_bin,
    )
    .await;

    let settings = st.settings();
    let label = settings.server.label();
    if !settings.configured() {
        out.push(
            "server_not_configured",
            Level::Warning,
            format!("{label} isn't configured, so nothing can be indexed or exported."),
            "Open Settings and enter your media server's URL and credentials.",
        );
    } else if let Ok(server) = st.media_server() {
        match tokio::time::timeout(NETWORK_TIMEOUT, server.sections()).await {
            Err(_) => out.push(
                "server_unreachable",
                Level::Error,
                format!(
                    "{label} didn't answer within {}s.",
                    NETWORK_TIMEOUT.as_secs()
                ),
                format!("Check the {label} URL in Settings and that the server is up and reachable from this container."),
            ),
            Ok(Err(e)) => out.push(
                "server_unreachable",
                Level::Error,
                format!("Can't reach {label}: {e:#}"),
                format!("Check the {label} URL and credentials in Settings (Settings → Test), and that the server is up."),
            ),
            // Reachable. A library with no path mapping is deliberately NOT
            // reported: leaving a library unmapped is how you opt it out of
            // indexing, so it isn't a fault.
            Ok(Ok(_)) => {
                if server.kind() == crate::mediaserver::ServerKind::Plex {
                    if let Ok(Ok(v)) =
                        tokio::time::timeout(NETWORK_TIMEOUT, server.server_version()).await
                    {
                        subtitle_search_issue(&mut out, &v);
                    }
                }
            }
        }
    }

    let mappings = settings.mappings.clone();
    let output_dir = cfg.output_dir.clone();
    let tmp_dir = cfg.tmp_dir();
    let data_dir = cfg.data_dir.clone();
    let db_path = cfg.db_path();
    let repo = cfg.github_repo.clone();
    let fs_issues = tokio::task::spawn_blocking(move || {
        let mut out = Issues::new(&repo);
        unreadable_mappings(&mut out, &mappings);
        for (label, dir) in [("Output", &output_dir), ("Temp", &tmp_dir)] {
            if let Err(e) = probe_writable(dir) {
                out.push(
                    "dir_not_writable",
                    Level::Error,
                    format!("{label} directory {} isn't writable: {e}", dir.display()),
                    "Fix the volume's permissions/owner (the app runs as PUID:PGID), or set PUID=0.",
                );
            }
        }
        let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        for (label, dir) in [("Data", &data_dir), ("Output", &output_dir)] {
            if let (Ok(free), Ok(total)) = (fs4::available_space(dir), fs4::total_space(dir)) {
                low_space(&mut out, label, free, total);
                if label == "Data" {
                    vacuum_headroom(&mut out, free, db_bytes);
                }
            }
        }
        out.list
    })
    .await
    .unwrap_or_default();
    out.list.extend(fs_issues);

    let update = tokio::time::timeout(
        NETWORK_TIMEOUT,
        version::latest_tag(&cfg.github_repo, cfg.github_token.as_deref()),
    )
    .await
    .unwrap_or_else(|_| {
        Err(anyhow::anyhow!(
            "GitHub didn't answer within {}s",
            NETWORK_TIMEOUT.as_secs()
        ))
    });
    update_check(&mut out, &cfg.app_version, update);

    let index = st.index.locked().clone();
    if !index.running && index.finished_at.is_some() {
        if let Some(e) = &index.last_error {
            out.push(
                "last_scan_failed",
                Level::Error,
                format!("The last library scan failed: {e}"),
                "See the Activity log for details; the most common cause is Plex being unreachable.",
            );
        } else if index.errors > 0 {
            out.push(
                "scan_item_errors",
                Level::Warning,
                format!(
                    "{} item(s) failed to index in the last library scan.",
                    index.errors
                ),
                "The Activity log has each failure's reason (usually an unreadable or corrupt file).",
            );
        }
    }

    let mut issues = out.list;
    issues.sort_by_key(|i| std::cmp::Reverse(i.level));
    issues
}

/// Info-level: Plex is reachable but too old for online subtitle search, so
/// the "Find subtitles online" feature won't work. Not a fault -- plenty of
/// installs never use it -- hence not a warning.
fn subtitle_search_issue(out: &mut Issues, version: &str) {
    if crate::plex::supports_subtitle_search(version) == Some(false) {
        out.push(
            "subtitle_search_unavailable",
            Level::Info,
            format!(
                "Plex {version} is too old for online subtitle search (it needs {} or newer).",
                crate::plex::MIN_SUBTITLE_SEARCH_VERSION
            ),
            "Update Plex Media Server if you want to find missing subtitles from this app.",
        );
    }
}

async fn tool_checks(out: &mut Issues, ffmpeg: &str, ffprobe: &str, gifsicle: &str) {
    for (bin, kind, level, why) in [
        (
            ffmpeg,
            "ffmpeg_missing",
            Level::Error,
            "previews and exports can't run",
        ),
        (
            ffprobe,
            "ffprobe_missing",
            Level::Error,
            "indexing can't read media files",
        ),
        (
            gifsicle,
            "gifsicle_missing",
            Level::Warning,
            "GIF exports still work, just without the extra compression pass",
        ),
    ] {
        let flag = if kind == "gifsicle_missing" {
            "--version"
        } else {
            "-version"
        };
        if let Err(e) = probe_tool(bin, flag).await {
            out.push(
                kind,
                level,
                format!("`{bin}` isn't usable ({e}), so {why}."),
                "The Docker image includes it; on a custom install, put it on PATH or point the matching *_BIN env var at it.",
            );
        }
    }
}

/// Run `bin flag` and check it exits successfully.
async fn probe_tool(bin: &str, flag: &str) -> Result<(), String> {
    let run = tokio::process::Command::new(bin).arg(flag).output();
    match tokio::time::timeout(TOOL_TIMEOUT, run).await {
        Err(_) => Err("timed out".into()),
        Ok(Err(e)) => Err(e.to_string()),
        Ok(Ok(o)) if o.status.success() => Ok(()),
        Ok(Ok(o)) => Err(format!("exited with {}", o.status)),
    }
}

/// A mapping whose container side doesn't exist or can't be listed means
/// every item in it silently resolves as unreachable.
fn unreadable_mappings(out: &mut Issues, rules: &[PathRewrite]) {
    for r in rules {
        if let Err(e) = std::fs::read_dir(&r.to) {
            out.push(
                "mapping_unreadable",
                Level::Error,
                format!(
                    "Path mapping {} → {} points at a folder this container can't read: {e}",
                    r.from, r.to
                ),
                "Check the volume is mounted at that path and is readable by the app's user.",
            );
        }
    }
}

/// Create and remove a probe file.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".memearr-write-probe");
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)
}

fn low_space(out: &mut Issues, label: &str, free: u64, total: u64) {
    let threshold = LOW_SPACE_BYTES.max((total as f64 * LOW_SPACE_FRACTION) as u64);
    if total > 0 && free < threshold {
        out.push(
            "low_disk_space",
            Level::Warning,
            format!(
                "{label} volume is low on space: {} free of {}.",
                fmt_bytes(free as i64),
                fmt_bytes(total as i64)
            ),
            "Free some space: clear old exports or the preview cache (System), or grow the volume.",
        );
    }
}

/// VACUUM writes a full copy of the database before swapping it in, so it
/// needs roughly the database's size free on the data volume.
fn vacuum_headroom(out: &mut Issues, free: u64, db_bytes: u64) {
    if db_bytes > 0 && free < db_bytes {
        out.push(
            "vacuum_headroom",
            Level::Warning,
            format!(
                "Not enough free space to optimize the database: it's {} but only {} is free.",
                fmt_bytes(db_bytes as i64),
                fmt_bytes(free as i64)
            ),
            "Free up space on the data volume before the Optimize database task runs, or it will fail.",
        );
    }
}

fn update_check(out: &mut Issues, current: &str, latest: anyhow::Result<Option<String>>) {
    match latest {
        Err(e) => out.push(
            "update_check_failed",
            Level::Warning,
            format!("Can't check for updates: {e:#}"),
            "For a private repo, set GITHUB_TOKEN in the container's environment to a token with read access to it (classic `repo` scope, or fine-grained Contents: Read). A `read:packages` token for pulling the image is not enough.",
        ),
        Ok(Some(latest)) if version::is_newer(&latest, current) => out.push(
            "update_available",
            Level::Info,
            format!("A new version is available: {latest} (running {current})."),
            "Pull the new image and recreate the container.",
        ),
        Ok(_) => {}
    }
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issues() -> Issues {
        Issues::new("me/repo")
    }

    fn rule(from: &str, to: &str) -> PathRewrite {
        PathRewrite {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn a_mapping_to_a_missing_folder_is_an_error_but_a_real_one_is_fine() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = issues();
        let real = dir.path().to_string_lossy().to_string();
        let missing = dir.path().join("nope").to_string_lossy().to_string();
        unreadable_mappings(&mut out, &[rule("/a", &real), rule("/b", &missing)]);
        assert_eq!(out.list.len(), 1);
        assert_eq!(out.list[0].level, Level::Error);
        assert!(out.list[0].message.contains("/b"));
    }

    #[test]
    fn writable_probe_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        probe_writable(dir.path()).unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn low_space_uses_the_larger_of_2gb_and_5_percent() {
        const GB: u64 = 1024 * 1024 * 1024;
        let check = |free, total| {
            let mut out = issues();
            low_space(&mut out, "Data", free, total);
            !out.list.is_empty()
        };
        assert!(check(GB, 10 * GB)); // under the 2 GB floor
        assert!(!check(3 * GB, 10 * GB));
        assert!(check(40 * GB, 1000 * GB)); // under 5% of a 1 TB volume
        assert!(!check(60 * GB, 1000 * GB));
    }

    #[test]
    fn vacuum_needs_about_the_database_size_free() {
        let mut out = issues();
        vacuum_headroom(&mut out, 4_000, 5_000);
        vacuum_headroom(&mut out, 6_000, 5_000);
        assert_eq!(out.list.len(), 1);
        assert_eq!(out.list[0].kind, "vacuum_headroom");
    }

    #[test]
    fn update_check_reports_failure_and_new_versions_but_not_up_to_date() {
        let mut out = issues();
        update_check(
            &mut out,
            "v0.13.1",
            Err(anyhow::anyhow!("GitHub returned 404 Not Found")),
        );
        update_check(&mut out, "v0.13.1", Ok(Some("v0.14.0".into())));
        update_check(&mut out, "v0.13.1", Ok(Some("v0.13.1".into())));
        update_check(&mut out, "v0.13.1", Ok(None));
        let kinds: Vec<_> = out.list.iter().map(|i| (i.kind, i.level)).collect();
        assert_eq!(
            kinds,
            vec![
                ("update_check_failed", Level::Warning),
                ("update_available", Level::Info)
            ]
        );
        assert!(out.list[0].fix.contains("read:packages"));
    }

    #[test]
    fn old_plex_gets_an_info_note_about_subtitle_search_and_nothing_else_does() {
        let kinds = |v: &str| {
            let mut out = issues();
            subtitle_search_issue(&mut out, v);
            out.list
                .iter()
                .map(|i| (i.kind, i.level))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds("1.13.8.5394"),
            vec![("subtitle_search_unavailable", Level::Info)]
        );
        assert!(kinds("1.13.8.5395").is_empty());
        assert!(kinds("1.41.3.9314-a0bfb8370").is_empty());
        // A version we can't read says nothing about the feature: no note.
        assert!(kinds("garbled").is_empty());
    }

    #[tokio::test]
    async fn a_missing_tool_is_reported_with_its_level() {
        let mut out = issues();
        tool_checks(
            &mut out,
            "definitely-not-ffmpeg-xyz",
            "definitely-not-ffprobe-xyz",
            "definitely-not-gifsicle-xyz",
        )
        .await;
        let kinds: Vec<_> = out.list.iter().map(|i| (i.kind, i.level)).collect();
        assert_eq!(
            kinds,
            vec![
                ("ffmpeg_missing", Level::Error),
                ("ffprobe_missing", Level::Error),
                ("gifsicle_missing", Level::Warning)
            ]
        );
    }

    #[test]
    fn levels_sort_errors_first() {
        let mut v = [Level::Info, Level::Error, Level::Warning];
        v.sort_by(|a, b| b.cmp(a));
        assert_eq!(v, [Level::Error, Level::Warning, Level::Info]);
    }
}
