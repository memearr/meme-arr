//! HTTP API: search, media metadata, preview, export jobs, and index control.

use crate::diskstats::fmt_bytes;
use crate::export::{self, ExportRequest};
use crate::mediaserver::{valid_item_id, MediaServer, ServerKind};
use crate::state::{AppState, Job, JobStatus, LogLevel};
use crate::tasks::{self, Task};
use crate::{
    auth, db, diskstats, ffmpeg, oidc, paths, plex_auth, preview_cache, querysyntax, subtitles,
};
use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, RawQuery, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use serde_json::json;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use tracing::{info, warn};

mod authentik;
mod clips;
mod library;
mod plex_login;
mod search;
mod session;
mod settings;
mod subtitle_api;
mod system;
use self::authentik::*;
use self::clips::*;
use self::library::*;
use self::plex_login::*;
use self::search::*;
use self::session::*;
use self::settings::*;
use self::subtitle_api::*;
use self::system::*;

/// The libraries a request is limited to: every `library=<name>` in the query
/// string (the parameter may repeat). Empty means all libraries. Blank values
/// are ignored, so an unset dropdown (`library=`) still means "all".
pub(super) fn libraries_from_query(raw: &Option<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pair in raw.as_deref().unwrap_or("").split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key != "library" {
            continue;
        }
        let value = value.replace('+', " ");
        let Ok(name) = urlencoding::decode(&value) else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/status", get(status))
        .route("/metrics", get(metrics))
        .route("/api/version", get(version))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/setup", post(auth_setup))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/auth/change-password", post(auth_change_password))
        .route("/api/auth/oidc/login", get(oidc_login))
        .route("/api/auth/oidc/callback", get(oidc_callback))
        .route("/api/auth/plex/login", get(plex_login))
        .route("/api/auth/plex/callback", get(plex_callback))
        .route(
            "/api/settings/authentik",
            get(get_authentik_settings)
                .put(put_authentik_settings)
                .delete(delete_authentik_settings_handler),
        )
        .route(
            "/api/settings/plex-login",
            get(get_plex_login_settings).delete(delete_plex_login_settings),
        )
        .route("/api/settings/plex-login/link", get(plex_login_link_start))
        .route("/api/libraries", get(libraries))
        .route("/api/search", get(search))
        .route("/api/search/title", get(search_title))
        .route("/api/media/{rating_key}", get(media))
        .route(
            "/api/media/{rating_key}/subtitles",
            get(subtitles_in_window),
        )
        .route("/api/subtitles/capability", get(subtitle_capability))
        .route(
            "/api/media/{rating_key}/subtitles/diagnose",
            get(subtitle_diagnose),
        )
        .route(
            "/api/subtitles/requests",
            get(subtitle_requests).delete(subtitle_requests_clear),
        )
        .route(
            "/api/subtitles/requests/{id}",
            axum::routing::delete(subtitle_request_delete),
        )
        .route(
            "/api/media/{rating_key}/subtitles/search",
            get(subtitle_search),
        )
        .route(
            "/api/media/{rating_key}/subtitles/download",
            post(subtitle_download),
        )
        .route("/api/preview/{rating_key}", get(preview))
        .route("/api/export", post(export_start))
        .route("/api/sequence/export", post(sequence_export_start))
        .route("/api/jobs/{id}", get(job_status))
        .route("/api/download/{filename}", get(download))
        .route("/api/index/status", get(index_status))
        .route("/api/index/reindex", post(reindex))
        .route("/api/tasks", get(list_tasks))
        .route("/api/backups", get(list_backups).post(create_backup))
        .route("/api/backups/restore", post(restore_backup))
        .route("/api/backups/{name}", get(download_backup))
        .route(
            "/api/settings/api-key",
            get(api_key_status)
                .post(api_key_generate)
                .delete(api_key_revoke),
        )
        .route(
            "/api/settings/trusted-proxies",
            get(get_trusted_proxies).put(put_trusted_proxies),
        )
        .route("/api/logs", get(get_logs))
        .route(
            "/api/settings/log-level",
            get(get_log_level).put(put_log_level),
        )
        .route("/api/health", get(health_report))
        .route("/api/health/check", post(health_check_now))
        .route("/api/tasks/{id}/run", post(run_task))
        .route("/api/tasks/{id}", axum::routing::put(update_task))
        .route(
            "/api/activity/log",
            get(activity_log).delete(clear_activity_log),
        )
        .route("/api/system/storage", get(system_storage))
        .route("/api/exports", get(list_exports))
        .route("/api/system/vacuum", post(system_vacuum))
        .route("/api/system/clear-preview-cache", post(clear_preview_cache))
        .route("/api/system/clear-exports", post(clear_exports))
        .route("/api/libraries/stats", get(library_stats))
        .route("/api/libraries/missing", get(library_missing))
        .route("/api/wanted", get(wanted))
        .route("/api/media/{rating_key}/reindex", post(media_reindex))
        .route("/api/library/browse", get(library_browse))
        .route("/api/library/episodes", get(library_episodes))
        .route(
            "/api/settings",
            get(get_settings).put(put_settings).delete(delete_settings),
        )
        .route("/api/settings/test", post(test_settings))
        .route("/api/system/browse", get(browse_dirs))
        // Anything else under /api is an unknown API path: answer in JSON like
        // every other API error (it used to fall through to the web page, with
        // a 200). More specific routes above always win over these. The auth
        // layer below still applies, so an unauthenticated caller gets a 401
        // rather than learning which paths exist.
        .route("/api", axum::routing::any(api_not_found))
        .route("/api/", axum::routing::any(api_not_found))
        .route("/api/{*rest}", axum::routing::any(api_not_found))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state, require_auth))
}

// ---- error handling ------------------------------------------------------

pub struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let msg = format!("{:#}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": msg })),
        )
            .into_response()
    }
}

fn bad_request(msg: impl Into<String>) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg.into() })),
    )
        .into_response()
}

fn not_found(msg: impl Into<String>) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": msg.into() }))).into_response()
}

/// 409: the request is fine but the app isn't in a state to do it (for
/// example no media server is configured yet).
fn conflict(msg: impl Into<String>) -> Response {
    (StatusCode::CONFLICT, Json(json!({ "error": msg.into() }))).into_response()
}

async fn api_not_found() -> Response {
    not_found("no such API endpoint")
}

// ---- handlers ------------------------------------------------------------

/// Serve a file, honouring a single `Range: bytes=` request (so a download can
/// resume and a `<video>` can seek) -- see `byterange` for what is supported.
async fn serve_file(
    path: &std::path::Path,
    content_type: &str,
    req_headers: &HeaderMap,
) -> Result<Response, AppError> {
    use crate::byterange::{self, Range};
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let range = req_headers.get(header::RANGE).and_then(|v| v.to_str().ok());

    match byterange::parse(range, len) {
        Range::Full => {
            let mut bytes = Vec::with_capacity(len as usize);
            file.read_to_end(&mut bytes).await?;
            Ok((
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                ],
                Body::from(bytes),
            )
                .into_response())
        }
        Range::Partial { start, end } => {
            file.seek(std::io::SeekFrom::Start(start)).await?;
            let mut bytes = vec![0u8; (end - start + 1) as usize];
            file.read_exact(&mut bytes).await?;
            Ok((
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::ACCEPT_RANGES, "bytes".to_string()),
                    (header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}")),
                ],
                Body::from(bytes),
            )
                .into_response())
        }
        Range::Unsatisfiable => Ok((
            StatusCode::RANGE_NOT_SATISFIABLE,
            [
                (header::CONTENT_RANGE, format!("bytes */{len}")),
                (header::ACCEPT_RANGES, "bytes".to_string()),
            ],
        )
            .into_response()),
    }
}

fn content_type_for(filename: &str) -> &'static str {
    match filename.rsplit('.').next() {
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Pick a bundled font file for meme text / subtitle rendering.
fn default_font(fonts_dir: &std::path::Path) -> String {
    for candidate in ["Impact.ttf", "DejaVuSans-Bold.ttf", "DejaVuSans.ttf"] {
        let p = fonts_dir.join(candidate);
        if p.is_file() {
            return p.to_string_lossy().to_string();
        }
    }
    // Fall back to a common system path; drawtext will error clearly if absent.
    fonts_dir
        .join("DejaVuSans.ttf")
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn libs(q: &str) -> Vec<String> {
        libraries_from_query(&Some(q.to_string()))
    }

    #[test]
    fn libraries_come_from_every_library_parameter() {
        assert_eq!(libs("q=x&library=TV&library=Anime"), ["TV", "Anime"]);
        assert_eq!(libs("library=TV"), ["TV"]);
    }

    #[test]
    fn a_blank_or_missing_library_means_all() {
        assert!(libs("q=x&library=").is_empty());
        assert!(libs("q=x").is_empty());
        assert!(libraries_from_query(&None).is_empty());
    }

    #[test]
    fn names_are_percent_decoded_and_deduplicated() {
        assert_eq!(libs("library=4K%20Movies&library=4K+Movies"), ["4K Movies"]);
        assert_eq!(libs("library=A%26B"), ["A&B"]);
        assert_eq!(libs("x=library%3DTV&library=TV&mylibrary=No"), ["TV"]);
    }
}
