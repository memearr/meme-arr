//! Media-server settings, connection test, and the folder browser.

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct SettingsPayload {
    /// `"plex"` | `"jellyfin"`. Absent keeps the saved choice.
    #[serde(default)]
    media_server: Option<String>,
    #[serde(default)]
    jellyfin_url: Option<String>,
    /// Only applied when present and non-empty, like `plex_token`.
    #[serde(default)]
    jellyfin_api_key: Option<String>,
    #[serde(default)]
    plex_url: Option<String>,
    /// Only applied when present and non-empty; otherwise the saved token is kept.
    #[serde(default)]
    plex_token: Option<String>,
    #[serde(default)]
    mappings: Option<Vec<db::Mapping>>,
    /// Write subtitles read from the server as a `.srt` beside the video.
    #[serde(default)]
    save_fetched_subtitles: Option<bool>,
}

/// Current settings. The token itself is never returned — only whether one is set.
pub(super) async fn get_settings(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let s = st.settings();
    let pool = st.pool.clone();
    let mappings = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::list_mappings(&conn)
    })
    .await??;
    Ok(Json(json!({
        "media_server": s.server.as_str(),
        "jellyfin_url": s.jellyfin_url,
        "jellyfin_key_set": s.jellyfin_api_key.as_deref().is_some_and(|t| !t.is_empty()),
        "plex_url": s.plex_url,
        "token_set": s.plex_token.as_deref().is_some_and(|t| !t.is_empty()),
        "configured": s.configured(),
        "save_fetched_subtitles": s.save_fetched_subtitles,
        "mappings": mappings,
    })))
}

pub(super) async fn put_settings(
    State(st): State<AppState>,
    Json(p): Json<SettingsPayload>,
) -> Result<Response, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut conn = pool.get()?;
        if let Some(kind) = p.media_server {
            // Normalise through the enum so only known values are stored.
            let kind = ServerKind::parse(Some(&kind));
            db::set_setting(&conn, "media_server", kind.as_str())?;
        }
        if let Some(url) = p.jellyfin_url {
            let url = url.trim().trim_end_matches('/').to_string();
            db::set_setting(&conn, "jellyfin_url", &url)?;
        }
        if let Some(key) = p.jellyfin_api_key {
            if !key.trim().is_empty() {
                db::set_setting(&conn, "jellyfin_api_key", key.trim())?;
            }
        }
        if let Some(url) = p.plex_url {
            let url = url.trim().trim_end_matches('/').to_string();
            db::set_setting(&conn, "plex_url", &url)?;
        }
        // Update the token only if a non-empty one was supplied.
        if let Some(token) = p.plex_token {
            if !token.trim().is_empty() {
                db::set_setting(&conn, "plex_token", token.trim())?;
            }
        }
        if let Some(on) = p.save_fetched_subtitles {
            db::set_setting(&conn, "save_fetched_subtitles", if on { "1" } else { "0" })?;
        }
        if let Some(mappings) = p.mappings {
            db::replace_mappings(&mut conn, &mappings)?;
        }
        Ok(())
    })
    .await??;

    st.reload_settings()?;
    // Re-check health against the new settings, so a fixed mapping or Plex
    // URL clears its warning now instead of at the next scheduled pass. A
    // check already in flight is fine to skip.
    let _ = tasks::start(&st, Task::CheckHealth);
    get_settings(State(st))
        .await
        .map(IntoResponse::into_response)
}

pub(super) async fn delete_settings(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut conn = pool.get()?;
        for key in [
            "media_server",
            "jellyfin_url",
            "jellyfin_api_key",
            "plex_url",
            "plex_token",
            "save_fetched_subtitles",
        ] {
            db::delete_setting(&conn, key)?;
        }
        db::replace_mappings(&mut conn, &[])?;
        Ok(())
    })
    .await??;
    st.reload_settings()?;
    let _ = tasks::start(&st, Task::CheckHealth);
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub(super) struct BrowseDirsParams {
    #[serde(default)]
    path: Option<String>,
}

/// Subfolders of a container path, for the setup wizard. With no `path`, the
/// first of `/media`, `/mnt`, `/` that exists.
pub(super) async fn browse_dirs(Query(p): Query<BrowseDirsParams>) -> Result<Response, AppError> {
    let path = match p.path.filter(|s| !s.trim().is_empty()) {
        Some(p) => std::path::PathBuf::from(p),
        None => ["/media", "/mnt", "/"]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.is_dir())
            .unwrap_or_else(|| std::path::PathBuf::from("/")),
    };
    if !path.is_absolute() {
        return Ok(bad_request("path must be absolute"));
    }
    let shown = path.to_string_lossy().replace('\\', "/");
    match tokio::task::spawn_blocking(move || diskstats::list_dirs(&path)).await? {
        Ok(dirs) => Ok(Json(json!({ "path": shown, "dirs": dirs })).into_response()),
        Err(e) => Ok(bad_request(format!("can't read {shown}: {e}"))),
    }
}

/// Try connecting to the chosen media server using the supplied overrides (or
/// saved settings) and report the libraries found. Does not persist anything.
pub(super) async fn test_settings(
    State(st): State<AppState>,
    Json(p): Json<SettingsPayload>,
) -> Result<Response, AppError> {
    let saved = st.settings();
    // Unsaved form values win; blanks fall back to what's saved.
    let pick = |new: Option<String>, old: Option<String>, strip_slash: bool| {
        new.map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(|v| {
                if strip_slash {
                    v.trim_end_matches('/').to_string()
                } else {
                    v
                }
            })
            .or(old)
            .filter(|v| !v.is_empty())
    };
    let kind = p
        .media_server
        .as_deref()
        .map(|k| ServerKind::parse(Some(k)))
        .unwrap_or(saved.server);

    let (server, extra) = match kind {
        ServerKind::Plex => {
            let url = pick(p.plex_url, saved.plex_url, true);
            let token = pick(p.plex_token, saved.plex_token, false);
            match (url, token) {
                (Some(u), Some(t)) => (MediaServer::Plex(crate::plex::PlexClient::new(u, t)), None),
                _ => return Ok(bad_request("provide a Plex URL and token to test")),
            }
        }
        ServerKind::Jellyfin => {
            let url = pick(p.jellyfin_url, saved.jellyfin_url, true);
            let key = pick(p.jellyfin_api_key, saved.jellyfin_api_key, false);
            let (Some(u), Some(k)) = (url, key) else {
                return Ok(bad_request("provide a Jellyfin URL and API key to test"));
            };
            // /System/Info needs authorization, so this proves the key too.
            let name = match crate::jellyfin::test_connection(&u, &k).await {
                Ok(n) => n,
                Err(e) => {
                    return Ok(
                        Json(json!({ "ok": false, "error": format!("{e:#}") })).into_response()
                    )
                }
            };
            (
                MediaServer::Jellyfin(crate::jellyfin::JellyfinClient::new(u, k)),
                Some(name),
            )
        }
    };

    match server.sections().await {
        Ok(sections) => {
            let libs: Vec<_> = sections
                .into_iter()
                .map(|s| json!({ "title": s.title, "type": s.kind, "locations": s.locations }))
                .collect();
            Ok(
                Json(json!({ "ok": true, "server_name": extra, "libraries": libs }))
                    .into_response(),
            )
        }
        Err(e) => Ok(Json(json!({ "ok": false, "error": format!("{e:#}") })).into_response()),
    }
}

// ---- helpers -------------------------------------------------------------
