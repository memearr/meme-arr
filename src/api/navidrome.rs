//! Navidrome: its settings, searching its songs, and a song as a file (for the editor's
//! preview and for an export). See `crate::navidrome` for the client.

use super::*;
use std::path::PathBuf;

/// 502: Navidrome (or the network to it) said no. Not 401/403, which would tell the
/// browser *it* is logged out.
fn upstream(msg: impl Into<String>) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({ "error": crate::tls::explain(msg.into()) })),
    )
        .into_response()
}

fn not_set_up() -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({ "error": "Navidrome isn't set up yet (Settings → Navidrome)" })),
    )
        .into_response()
}

/// What the settings screen shows. The password is never sent back, only whether one is set.
fn settings_json(s: &crate::state::AppSettings) -> serde_json::Value {
    let set = |v: &Option<String>| v.as_deref().is_some_and(|x| !x.is_empty());
    json!({
        "configured": s.navidrome().is_some(),
        "url": s.navidrome_url,
        "user": s.navidrome_user,
        "password_set": set(&s.navidrome_password),
    })
}

pub(super) async fn get_navidrome_settings(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(settings_json(&st.settings()))
}

#[derive(Debug, Deserialize)]
pub(super) struct NavidromePayload {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    user: Option<String>,
    /// Only applied when present and non-empty, like the other secrets.
    #[serde(default)]
    password: Option<String>,
}

/// Try the saved connection. Never fails: the answer says.
async fn check_saved(st: &AppState) -> serde_json::Value {
    let Some(client) = st.settings().navidrome() else {
        return json!({ "ok": false, "error": "Navidrome isn't set up yet" });
    };
    match client.ping().await {
        Ok(version) => json!({ "ok": true, "version": version }),
        Err(e) => json!({ "ok": false, "error": crate::tls::explain(format!("{e:#}")) }),
    }
}

pub(super) async fn put_navidrome_settings(
    State(st): State<AppState>,
    Json(p): Json<NavidromePayload>,
) -> Result<Response, AppError> {
    if let Some(url) = &p.url {
        let u = url.trim();
        if !u.is_empty() && !(u.starts_with("http://") || u.starts_with("https://")) {
            return Ok(bad_request(
                "The Navidrome address should start with http:// or https://",
            ));
        }
    }
    st.db(move |conn| {
        if let Some(url) = p.url {
            db::set_setting(conn, "navidrome_url", url.trim().trim_end_matches('/'))?;
        }
        if let Some(user) = p.user {
            db::set_setting(conn, "navidrome_user", user.trim())?;
        }
        if let Some(pw) = p.password {
            if !pw.is_empty() {
                db::set_setting(conn, "navidrome_password", &pw)?;
            }
        }
        Ok(())
    })
    .await?;
    st.reload_settings()?;
    let check = check_saved(&st).await;
    let mut body = settings_json(&st.settings());
    body["check"] = check;
    Ok(Json(body).into_response())
}

pub(super) async fn test_navidrome_settings(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(check_saved(&st).await)
}

pub(super) async fn delete_navidrome_settings(
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    st.db(move |conn| {
        for key in ["navidrome_url", "navidrome_user", "navidrome_password"] {
            db::delete_setting(conn, key)?;
        }
        Ok(())
    })
    .await?;
    st.reload_settings()?;
    Ok(Json(settings_json(&st.settings())))
}

#[derive(Debug, Deserialize)]
pub(super) struct MusicSearch {
    #[serde(default)]
    q: String,
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/music/search?q=&limit=`: songs in Navidrome that match.
pub(super) async fn music_search(
    State(st): State<AppState>,
    Query(p): Query<MusicSearch>,
) -> Result<Response, AppError> {
    let Some(client) = st.settings().navidrome() else {
        return Ok(not_set_up());
    };
    let q = p.q.trim();
    if q.is_empty() {
        return Ok(Json(json!({ "tracks": [] })).into_response());
    }
    let limit = p.limit.unwrap_or(20).clamp(1, 50);
    match client.search(q, limit).await {
        Ok(tracks) => Ok(Json(json!({ "tracks": tracks })).into_response()),
        Err(e) => Ok(upstream(format!("{e:#}"))),
    }
}

/// Why a song could not be made available.
pub(super) enum TrackError {
    NotSetUp,
    BadId,
    Upstream(String),
}

/// Song `id` as a file in the music cache, fetching it from Navidrome the first time.
/// One fetch at a time: they share a download file name, and a song is fetched once
/// however many people ask.
pub(super) async fn ensure_track(st: &AppState, id: &str) -> Result<PathBuf, TrackError> {
    static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let music = st.config.music();
    if !crate::music::valid_id(id) {
        return Err(TrackError::BadId);
    }
    if let Some(p) = music.path_of(id) {
        return Ok(p);
    }
    let Some(client) = st.settings().navidrome() else {
        return Err(TrackError::NotSetUp);
    };
    let _one = FETCHING.lock().await;
    if let Some(p) = music.path_of(id) {
        return Ok(p);
    }
    let up = |e: String| TrackError::Upstream(e);
    tokio::fs::create_dir_all(music.dir())
        .await
        .map_err(|e| up(e.to_string()))?;
    {
        let m = music.clone();
        let _ = tokio::task::spawn_blocking(move || m.prune(crate::music::MAX_CACHE_BYTES)).await;
    }
    let dl = music.download_path(id).ok_or(TrackError::BadId)?;
    client
        .fetch(id, &dl, crate::navidrome::MAX_TRACK_BYTES)
        .await
        .map_err(|e| up(format!("{e:#}")))?;
    let m = music.clone();
    let id2 = id.to_string();
    tokio::task::spawn_blocking(move || m.finish(&id2, &dl))
        .await
        .map_err(|e| up(e.to_string()))?
        .map_err(up)
}

pub(super) fn track_error(e: TrackError) -> Response {
    match e {
        TrackError::NotSetUp => not_set_up(),
        TrackError::BadId => not_found("no such song"),
        TrackError::Upstream(m) => upstream(m),
    }
}

/// `GET /api/music/{id}/file`: the song itself (fetched from Navidrome the first time),
/// with range support so the player can seek in it.
pub(super) async fn music_file(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    match ensure_track(&st, &id).await {
        Ok(path) => serve_file(&path, crate::music::content_type(&path), &headers).await,
        Err(e) => Ok(track_error(e)),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct CoverQuery {
    /// Pixels (default 96, kept between 32 and 512).
    size: Option<u32>,
}

/// `GET /api/music/cover/{id}?size=`: a song's cover picture from Navidrome (`id` is a
/// search result's `cover_art`), for the picker. Pictures don't change, so the browser keeps them.
pub(super) async fn music_cover(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CoverQuery>,
) -> Result<Response, AppError> {
    if !crate::music::valid_id(&id) {
        return Ok(not_found("no such picture"));
    }
    let Some(client) = st.settings().navidrome() else {
        return Ok(not_set_up());
    };
    let size = q.size.unwrap_or(96).clamp(32, 512);
    match client.cover(&id, size).await {
        Ok((kind, bytes)) => Ok((
            [
                (header::CONTENT_TYPE, kind),
                (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
            ],
            bytes,
        )
            .into_response()),
        Err(e) => Ok(upstream(format!("{e:#}"))),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct PeaksQuery {
    /// How many bars (default 600, kept between 20 and 2000).
    bins: Option<usize>,
}

/// `GET /api/music/{id}/peaks`: the song's loudness over time as bars, for the editor's
/// waveform. `{"duration_ms": 187000, "peaks": [0.2, 0.9, ...]}`.
pub(super) async fn music_peaks(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PeaksQuery>,
) -> Result<Response, AppError> {
    let path = match ensure_track(&st, &id).await {
        Ok(p) => p,
        Err(e) => return Ok(track_error(e)),
    };
    let bins = q.bins.unwrap_or(600);
    match crate::waveform::analyze(&st.config.ffmpeg_bin, &path, bins).await {
        Ok(w) => {
            Ok(Json(json!({ "duration_ms": w.duration_ms, "peaks": w.peaks })).into_response())
        }
        Err(e) => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": format!("couldn't read that song: {e:#}") })),
        )
            .into_response()),
    }
}

#[cfg(test)]
#[path = "navidrome_tests.rs"]
mod tests;
