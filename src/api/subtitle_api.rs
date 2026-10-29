//! Online subtitle search/download (Plex and Jellyfin), the Requests list, diagnosis, and re-indexing one item.

use super::*;

/// Why online subtitle search isn't available, or the server + version if it is.
pub(super) enum SubtitleGate {
    /// The server, the version it reported, and whether that version was
    /// understood and checked against a known minimum (Plex only: Jellyfin has
    /// no documented minimum, so its version is reported but never gated on).
    Ok(MediaServer, String, bool),
    No(StatusCode, String),
}

/// Check the selected server is configured and reachable, and -- for Plex,
/// which only has subtitle search from 1.13.8.5395 -- new enough.
pub(super) async fn subtitle_gate(st: &AppState) -> SubtitleGate {
    use crate::plex::{supports_subtitle_search, MIN_SUBTITLE_SEARCH_VERSION as min};
    let server = match st.media_server() {
        Ok(s) => s,
        Err(e) => return SubtitleGate::No(StatusCode::CONFLICT, format!("{e:#}")),
    };
    let label = server.kind().label();
    let version = match server.server_version().await {
        Ok(v) => v,
        Err(e) => {
            return SubtitleGate::No(
                StatusCode::BAD_GATEWAY,
                format!("Couldn't read the {label} server version: {e:#}"),
            )
        }
    };
    if server.kind() == ServerKind::Jellyfin {
        return SubtitleGate::Ok(server, version, false);
    }
    match supports_subtitle_search(&version) {
        Some(true) => SubtitleGate::Ok(server, version, true),
        Some(false) => SubtitleGate::No(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Plex {version} is too old for subtitle search; it needs {min} or newer"),
        ),
        // A version we can't read tells us nothing about the feature, so
        // don't turn away a server that may well support it: try the call and
        // let Plex's own answer decide.
        None => SubtitleGate::Ok(server, version, false),
    }
}

/// Whether online subtitle search can be offered right now (never an error:
/// the answer is in the body).
pub(super) async fn subtitle_capability(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(match subtitle_gate(&st).await {
        SubtitleGate::Ok(server, version, checked) => {
            let mut body = json!({
                "supported": true,
                "server": server.kind().as_str(),
                "version": version,
                "version_checked": checked,
            });
            match server.kind() {
                ServerKind::Plex => {
                    body["minimum_version"] = json!(crate::plex::MIN_SUBTITLE_SEARCH_VERSION);
                }
                // Jellyfin can't tell us up front whether any provider is
                // installed (its API has no endpoint for that), so say what
                // search depends on instead of promising results.
                ServerKind::Jellyfin => {
                    body["needs_provider_plugin"] = json!(true);
                }
            }
            body
        }
        SubtitleGate::No(_, reason) => json!({ "supported": false, "reason": reason }),
    })
}

/// Turn a media-server refusal into our own status: unknown item -> 404;
/// auth/permission problems -> 502, not 401/403 (which would tell the browser
/// *it* is logged out); anything else -> 500 as usual.
pub(super) fn upstream_error(e: anyhow::Error) -> Response {
    let code = match e.downcast_ref::<crate::mediaserver::UpstreamStatus>() {
        Some(u) if u.status == 404 => StatusCode::NOT_FOUND,
        Some(u) if u.status == 401 || u.status == 403 => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (code, Json(json!({ "error": format!("{e:#}") }))).into_response()
}

#[derive(Debug, Deserialize)]
pub(super) struct SubtitleSearchParams {
    #[serde(default)]
    language: Option<String>,
}

pub(super) async fn subtitle_search(
    State(st): State<AppState>,
    Path(item_id): Path<String>,
    Query(p): Query<SubtitleSearchParams>,
) -> Result<Response, AppError> {
    if !valid_item_id(st.settings().server, &item_id) {
        return Ok(bad_request("invalid item id"));
    }
    let language = p.language.unwrap_or_else(|| "en".into());
    if language.is_empty()
        || language.len() > 8
        || !language
            .bytes()
            .all(|b| b.is_ascii_alphabetic() || b == b'-')
    {
        return Ok(bad_request("language must be a code like en, pt-BR or eng"));
    }
    match subtitle_gate(&st).await {
        SubtitleGate::No(code, why) => Ok((code, Json(json!({ "error": why }))).into_response()),
        SubtitleGate::Ok(server, version, _) => {
            if server.kind() == ServerKind::Jellyfin
                && crate::jellyfin::language_code(&language).is_none()
            {
                return Ok(bad_request(
                    "unknown language: use a three-letter code like eng for Jellyfin",
                ));
            }
            let results = match server.subtitle_search(&item_id, &language).await {
                Ok(r) => r,
                Err(e) => return Ok(upstream_error(e)),
            };
            let mut body = json!({ "version": version, "results": results });
            if results.is_empty() && server.kind() == ServerKind::Jellyfin {
                body["note"] = json!(
                    "No results. Jellyfin only searches subtitle providers you have installed \
                     and configured (for example the OpenSubtitles plugin)."
                );
            }
            Ok(Json(body).into_response())
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SubtitleDownloadPayload {
    key: String,
    /// What the search result was called, who it came from and in what
    /// language: only used to describe the request in the Requests list.
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

/// Trim, drop empties, and cap the length of free text that came from the
/// client before it's stored.
pub(super) fn clip(s: Option<String>, max: usize) -> Option<String> {
    s.map(|s| s.trim().chars().take(max).collect::<String>())
        .filter(|s| !s.is_empty())
}

/// A subtitle key from a search result is opaque, but it ends up in a request
/// to the media server, so bound it. (Plex keys are additionally required to
/// be plain server paths; Jellyfin keys are percent-encoded into a path
/// segment, so any printable text is safe.)
pub(super) fn valid_subtitle_key_for(kind: ServerKind, key: &str) -> bool {
    match kind {
        ServerKind::Plex => crate::plex::valid_subtitle_key(key),
        ServerKind::Jellyfin => {
            !key.is_empty() && key.len() <= 1024 && !key.chars().any(char::is_control)
        }
    }
}

pub(super) async fn subtitle_download(
    State(st): State<AppState>,
    Path(item_id): Path<String>,
    Json(p): Json<SubtitleDownloadPayload>,
) -> Result<Response, AppError> {
    let kind = st.settings().server;
    if !valid_item_id(kind, &item_id) {
        return Ok(bad_request("invalid item id"));
    }
    if !valid_subtitle_key_for(kind, &p.key) {
        return Ok(bad_request("invalid subtitle key"));
    }
    match subtitle_gate(&st).await {
        SubtitleGate::No(code, why) => Ok((code, Json(json!({ "error": why }))).into_response()),
        SubtitleGate::Ok(server, _, _) => {
            let label = server.kind().label();
            let ack = match server.subtitle_download(&item_id, &p.key).await {
                Ok(ack) => ack,
                Err(e) => {
                    // The UI gets the error too, but a failed request should
                    // also be findable in the logs afterwards.
                    tracing::warn!(item = %item_id, "{label} subtitle download failed: {e:#}");
                    st.log_event(
                        LogLevel::Warn,
                        format!("{label} subtitle download for item {item_id} failed: {e:#}"),
                    );
                    return Ok(upstream_error(e));
                }
            };
            // Remember the request so the Requests list can follow it until the
            // subtitle is searchable. Failing to record it must not turn a
            // download the server already accepted into an error.
            let (rk, server_kind) = (item_id.clone(), server.kind().as_str());
            let (title, provider, language) = (
                clip(p.title, 200).unwrap_or_else(|| "Subtitle".into()),
                clip(p.provider, 100),
                clip(p.language, 16),
            );
            let (pool, title_for_log) = (st.pool.clone(), title.clone());
            let recorded =
                tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
                    let conn = pool.get()?;
                    db::add_subtitle_request(
                        &conn,
                        &rk,
                        server_kind,
                        language.as_deref(),
                        &title,
                        provider.as_deref(),
                    )?;
                    Ok(db::media_item(&conn, &rk)?.map(|i| i.title))
                })
                .await;
            let item_title = match recorded {
                Ok(Ok(t)) => t,
                _ => {
                    tracing::warn!("couldn't record the subtitle request for item {item_id}");
                    None
                }
            };
            // What the server actually answered. "Accepted" is all the server
            // promises: it fetches the file in the background, and where it
            // saves it decides whether this app can ever see it (see
            // /subtitles/diagnose).
            let body = if ack.body.trim().is_empty() {
                String::new()
            } else {
                format!(", body: {}", ack.body.trim())
            };
            tracing::info!(
                item = %item_id, status = ack.status, elapsed_ms = ack.elapsed_ms,
                "{label} accepted the subtitle download \"{title_for_log}\"{body}"
            );
            st.log_event(
                LogLevel::Info,
                format!(
                    "Asked {label} to download \"{title_for_log}\" for '{}': it answered {} in {} ms{body}. It fetches the file in the background; re-index the item once it's there.",
                    item_title.as_deref().unwrap_or(&item_id),
                    ack.status,
                    ack.elapsed_ms
                ),
            );
            Ok((StatusCode::ACCEPTED, Json(json!({ "accepted": true }))).into_response())
        }
    }
}

/// Why hasn't a requested subtitle become searchable? Combines what this app
/// can see (the video, the files beside it, what the indexer would pick) with
/// what the media server says it has for the item, and states the likely
/// cause. Read-only.
pub(super) async fn subtitle_diagnose(
    State(st): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Response, AppError> {
    use crate::diagnose::{self, ServerView};
    let kind = st.settings().server;
    if !valid_item_id(kind, &item_id) {
        return Ok(bad_request("invalid item id"));
    }

    let (pool, mappings, rk) = (st.pool.clone(), st.mappings(), item_id.clone());
    let local = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        diagnose::local_report(&conn, &mappings, &rk)
    })
    .await??;

    let server = match st.media_server() {
        Err(e) => ServerView {
            kind: kind.as_str(),
            streams: None,
            error: Some(format!("{e:#}")),
        },
        Ok(srv) => {
            let asked = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                srv.item_subtitle_streams(&item_id),
            )
            .await;
            match asked {
                Ok(Ok(streams)) => ServerView {
                    kind: kind.as_str(),
                    streams: Some(streams),
                    error: None,
                },
                Ok(Err(e)) => ServerView {
                    kind: kind.as_str(),
                    streams: None,
                    error: Some(format!("{e:#}")),
                },
                Err(_) => ServerView {
                    kind: kind.as_str(),
                    streams: None,
                    error: Some("timed out after 10s".into()),
                },
            }
        }
    };
    let mut findings = diagnose::findings(&local, &server);
    findings.extend(diagnose::save_plan(
        &local,
        &server,
        st.settings().save_fetched_subtitles,
    ));
    tracing::debug!(item = %item_id, "subtitle diagnosis: {}", diagnose::summary_line(&local));
    Ok(Json(json!({ "local": local, "server": server, "findings": findings })).into_response())
}

/// The subtitle downloads this app has asked for, newest first, each with
/// whether it has become searchable subtitles yet.
pub(super) async fn subtitle_requests(State(st): State<AppState>) -> Result<Response, AppError> {
    let pool = st.pool.clone();
    let list = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::list_subtitle_requests(&conn, 500)
    })
    .await??;
    Ok(Json(json!({ "requests": list })).into_response())
}

pub(super) async fn subtitle_request_delete(
    State(st): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let pool = st.pool.clone();
    let removed = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        let conn = pool.get()?;
        db::delete_subtitle_request(&conn, id)
    })
    .await??;
    if removed {
        Ok(Json(json!({ "removed": true })).into_response())
    } else {
        Ok(not_found("no such request"))
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ClearRequestsParams {
    #[serde(default)]
    finished: bool,
}

/// `DELETE /api/subtitles/requests?finished=true`: forget the requests that
/// have become searchable. (The flag is required so a bare DELETE can't wipe
/// the ones still being waited on.)
pub(super) async fn subtitle_requests_clear(
    State(st): State<AppState>,
    Query(p): Query<ClearRequestsParams>,
) -> Result<Response, AppError> {
    if !p.finished {
        return Ok(bad_request("pass finished=true to clear finished requests"));
    }
    let pool = st.pool.clone();
    let n = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
        let conn = pool.get()?;
        db::clear_finished_subtitle_requests(&conn)
    })
    .await??;
    Ok(Json(json!({ "removed": n })).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct WindowParams {
    start: i64,
    end: i64,
}

pub(super) async fn subtitles_in_window(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Query(w): Query<WindowParams>,
) -> Result<Response, AppError> {
    let pool = st.pool.clone();
    let cues = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        let m = match db::get_media(&conn, &rating_key)? {
            Some(m) => m,
            None => return Ok(None),
        };
        Ok(Some(db::cues_in_window(&conn, m.id, w.start, w.end)?))
    })
    .await??;
    match cues {
        Some(cues) => Ok(Json(json!({ "cues": cues })).into_response()),
        None => Ok(not_found("media not indexed")),
    }
}

/// Re-index one item now (e.g. right after downloading a subtitle for it)
/// instead of scanning the whole library. `status: "unchanged"` means the
/// video and any sidecar subtitle are exactly as last indexed.
pub(super) async fn media_reindex(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
) -> Result<Response, AppError> {
    match crate::index::index_item(&st, &rating_key).await? {
        Some(r) => Ok(Json(r).into_response()),
        None => Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "unknown item" })),
        )
            .into_response()),
    }
}
