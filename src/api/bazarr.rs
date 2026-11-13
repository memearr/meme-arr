//! Bazarr: its settings, search/sync for one item, and the webhook it calls
//! when it has written a new subtitle. See `crate::bazarr` for the client.

use super::*;
use crate::bazarr::Target;
use std::sync::atomic::{AtomicBool, Ordering};

/// 502: Bazarr (or the network to it) said no. Not 401/403, which would tell
/// the browser *it* is logged out.
fn upstream(msg: impl Into<String>) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({ "error": msg.into() })),
    )
        .into_response()
}

/// What the settings screen shows. The API key is never sent back, only
/// whether one is set; the hook token is, because the admin has to paste it
/// into Bazarr.
fn settings_json(s: &crate::state::AppSettings) -> serde_json::Value {
    let set = |v: &Option<String>| v.as_deref().is_some_and(|x| !x.is_empty());
    json!({
        "configured": s.bazarr().is_some(),
        "url": s.bazarr_url,
        "key_set": set(&s.bazarr_api_key),
        "hook_token": s.bazarr_hook_token,
        "languages": s.bazarr_languages,
    })
}

pub(super) async fn get_bazarr_settings(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(settings_json(&st.settings()))
}

#[derive(Debug, Deserialize)]
pub(super) struct BazarrPayload {
    #[serde(default)]
    url: Option<String>,
    /// Only applied when present and non-empty, like the other API keys.
    #[serde(default)]
    api_key: Option<String>,
}

/// Ask the saved Bazarr who it is, and which languages it keeps; store the
/// languages so the indexer can prefer them. Never fails: the answer says.
async fn check_saved(st: &AppState) -> serde_json::Value {
    let Some(client) = st.settings().bazarr() else {
        return json!({ "ok": false, "error": "Bazarr isn't set up yet" });
    };
    let version = match client.version().await {
        Ok(v) => v,
        Err(e) => return json!({ "ok": false, "error": format!("{e:#}") }),
    };
    let languages = client.languages().await.unwrap_or_default();
    let pool = st.pool.clone();
    let joined = languages.join(",");
    let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::set_setting(&conn, "bazarr_languages", &joined)
    })
    .await;
    if !matches!(saved, Ok(Ok(()))) || st.reload_settings().is_err() {
        warn!("couldn't store Bazarr's languages");
    }
    json!({ "ok": true, "version": version, "languages": languages })
}

/// Save Bazarr's address and key (and make the webhook secret on first use),
/// then test the saved connection.
pub(super) async fn put_bazarr_settings(
    State(st): State<AppState>,
    Json(p): Json<BazarrPayload>,
) -> Result<Response, AppError> {
    if let Some(url) = &p.url {
        let u = url.trim();
        if !u.is_empty() && !(u.starts_with("http://") || u.starts_with("https://")) {
            return Ok(bad_request(
                "The Bazarr address should start with http:// or https://",
            ));
        }
    }
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        if let Some(url) = p.url {
            let url = url.trim().trim_end_matches('/').to_string();
            db::set_setting(&conn, "bazarr_url", &url)?;
        }
        if let Some(key) = p.api_key {
            if !key.trim().is_empty() {
                db::set_setting(&conn, "bazarr_api_key", key.trim())?;
            }
        }
        if db::get_setting(&conn, "bazarr_hook_token")?.is_none() {
            db::set_setting(&conn, "bazarr_hook_token", &auth::new_session_token())?;
        }
        Ok(())
    })
    .await??;
    st.reload_settings()?;

    let check = check_saved(&st).await;
    let mut body = settings_json(&st.settings());
    body["check"] = check;
    Ok(Json(body).into_response())
}

/// Test the saved connection again.
pub(super) async fn test_bazarr_settings(State(st): State<AppState>) -> Json<serde_json::Value> {
    Json(check_saved(&st).await)
}

/// Forget Bazarr entirely.
pub(super) async fn delete_bazarr_settings(
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        for key in [
            "bazarr_url",
            "bazarr_api_key",
            "bazarr_hook_token",
            "bazarr_languages",
        ] {
            db::delete_setting(&conn, key)?;
        }
        Ok(())
    })
    .await??;
    st.reload_settings()?;
    Ok(Json(settings_json(&st.settings())))
}

/// A new webhook secret (the old URL stops working).
pub(super) async fn rotate_bazarr_hook(
    State(st): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::set_setting(&conn, "bazarr_hook_token", &auth::new_session_token())
    })
    .await??;
    st.reload_settings()?;
    Ok(Json(settings_json(&st.settings())))
}

// ---- one item ------------------------------------------------------------------

/// The library item, as far as Bazarr is concerned: its path on the media
/// server, and the subtitle file beside it that the indexer would read.
struct Item {
    server_path: String,
    sidecar: Option<String>,
}

async fn load_item(st: &AppState, rating_key: &str) -> Result<Option<Item>, AppError> {
    let (pool, mappings, rk) = (st.pool.clone(), st.mappings(), rating_key.to_string());
    let item = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<Item>> {
        let conn = pool.get()?;
        let Some(m) = db::get_media(&conn, &rk)? else {
            return Ok(None);
        };
        let sidecar = match paths::resolve(&m.file_path, &mappings) {
            paths::AccessMode::Direct(p) => crate::index::find_sidecar(&p)
                .and_then(|s| s.file_name().map(|n| n.to_string_lossy().to_string())),
            paths::AccessMode::NotMounted => None,
        };
        Ok(Some(Item {
            server_path: m.file_path,
            sidecar,
        }))
    })
    .await??;
    Ok(item)
}

fn target_json(t: &Target) -> serde_json::Value {
    json!({
        "kind": t.kind,
        "title": t.title,
        "missing": t.missing,
    })
}

/// `GET /api/media/{rating_key}/bazarr`: is this item in Bazarr, what is it
/// missing, and is there a subtitle file we could ask it to re-time?
pub(super) async fn media_bazarr(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
) -> Result<Response, AppError> {
    let Some(client) = st.settings().bazarr() else {
        return Ok(Json(json!({ "configured": false })).into_response());
    };
    let Some(item) = load_item(&st, &rating_key).await? else {
        return Ok(not_found("unknown item"));
    };
    match client.find(&item.server_path).await {
        Ok(Some(t)) => Ok(Json(json!({
            "configured": true,
            "found": true,
            "target": target_json(&t),
            "sidecar": item.sidecar,
            "can_sync": item.sidecar.is_some(),
        }))
        .into_response()),
        Ok(None) => Ok(Json(json!({
            "configured": true,
            "found": false,
            "sidecar": item.sidecar,
        }))
        .into_response()),
        Err(e) => Ok(upstream(format!("{e:#}"))),
    }
}

/// `POST /api/media/{rating_key}/bazarr/search`: tell Bazarr to look for the
/// subtitles it is missing for this item. It works in the background; when it
/// writes one it calls our webhook (or the next scan finds it).
pub(super) async fn media_bazarr_search(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
) -> Result<Response, AppError> {
    let Some(client) = st.settings().bazarr() else {
        return Ok(conflict("Bazarr isn't set up (Settings → Bazarr)"));
    };
    let Some(item) = load_item(&st, &rating_key).await? else {
        return Ok(not_found("unknown item"));
    };
    let target = match client.find(&item.server_path).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Ok(not_found(
                "Bazarr doesn't have this file (or more than one of its entries matches it). \
                 Check that it is in Sonarr/Radarr and that Bazarr has synced.",
            ))
        }
        Err(e) => return Ok(upstream(format!("{e:#}"))),
    };
    match client.search(&target).await {
        Ok(asked) => {
            if asked > 0 {
                st.log_event(
                    LogLevel::Info,
                    format!("Asked Bazarr to search subtitles for '{}'", target.title),
                );
            }
            Ok(Json(json!({
                "asked": asked,
                "target": target_json(&target),
                "message": if asked == 0 {
                    "Bazarr isn't missing any subtitles for this item. If we still show none, \
                     re-index the item once its subtitle file is beside the video."
                } else {
                    "Bazarr is searching. The subtitle appears here once it has downloaded one."
                },
            }))
            .into_response())
        }
        Err(e) => Ok(upstream(format!("{e:#}"))),
    }
}

/// `POST /api/media/{rating_key}/bazarr/sync`: ask Bazarr to re-time the
/// subtitle file beside the video against the audio.
pub(super) async fn media_bazarr_sync(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
) -> Result<Response, AppError> {
    let Some(client) = st.settings().bazarr() else {
        return Ok(conflict("Bazarr isn't set up (Settings → Bazarr)"));
    };
    let Some(item) = load_item(&st, &rating_key).await? else {
        return Ok(not_found("unknown item"));
    };
    let Some(sidecar) = item.sidecar else {
        return Ok(bad_request(
            "There's no subtitle file beside this video to re-time (subtitles inside the video can't be).",
        ));
    };
    let target = match client.find(&item.server_path).await {
        Ok(Some(t)) => t,
        Ok(None) => return Ok(not_found("Bazarr doesn't have this file")),
        Err(e) => return Ok(upstream(format!("{e:#}"))),
    };
    match client.sync(&target, &sidecar).await {
        Ok(()) => {
            st.log_event(
                LogLevel::Info,
                format!(
                    "Asked Bazarr to re-time the subtitle for '{}'",
                    target.title
                ),
            );
            Ok(Json(json!({
                "ok": true,
                "file": sidecar,
                "message": "Bazarr is re-timing the subtitle. Re-index the item when it's done to use it.",
            }))
            .into_response())
        }
        Err(e) => Ok(upstream(format!("{e:#}"))),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ShiftBody {
    /// Milliseconds to move every subtitle line: positive = later.
    offset_ms: i64,
}

/// `POST /api/media/{rating_key}/bazarr/shift`: make the editor's subtitle sync
/// permanent by asking Bazarr to shift the `.srt` beside the video, then
/// re-index the item so search results and the editor use the new timing.
pub(super) async fn media_bazarr_shift(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Json(body): Json<ShiftBody>,
) -> Result<Response, AppError> {
    let Some(client) = st.settings().bazarr() else {
        return Ok(conflict("Bazarr isn't set up (Settings → Bazarr)"));
    };
    let Some(item) = load_item(&st, &rating_key).await? else {
        return Ok(not_found("unknown item"));
    };
    let Some(sidecar) = item.sidecar else {
        return Ok(bad_request(
            "There's no subtitle file beside this video to change (subtitles inside the video can't be).",
        ));
    };
    let target = match client.find(&item.server_path).await {
        Ok(Some(t)) => t,
        Ok(None) => return Ok(not_found("Bazarr doesn't have this file")),
        Err(e) => return Ok(upstream(format!("{e:#}"))),
    };
    if let Err(e) = client.shift(&target, &sidecar, body.offset_ms).await {
        return Ok(upstream(format!("{e:#}")));
    }
    st.log_event(
        LogLevel::Info,
        format!(
            "Asked Bazarr to shift the subtitle for '{}' by {} ms",
            target.title, body.offset_ms
        ),
    );
    // Bazarr has rewritten the file by now; read it again.
    let reindexed = crate::index::index_item(&st, &rating_key).await?;
    Ok(Json(json!({
        "ok": true,
        "file": sidecar,
        "reindexed": reindexed,
        "message": "The subtitle file was shifted and re-read.",
    }))
    .into_response())
}

// ---- the webhook ---------------------------------------------------------------

/// Equal without leaking, through timing, how much of a guess was right.
fn same_secret(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}

static SCAN_PENDING: AtomicBool = AtomicBool::new(false);

/// `POST /api/hooks/bazarr/{token}`: Bazarr calls this after it writes a
/// subtitle (Bazarr → Settings → Notifications → an Apprise URL such as
/// `json://host:port/api/hooks/bazarr/<token>`). Public, but the secret in the
/// URL is the key; the body is ignored. Calls close together are one scan,
/// run after a short wait so a batch of downloads is picked up together.
pub(super) async fn bazarr_hook(State(st): State<AppState>, Path(token): Path<String>) -> Response {
    let ok = st
        .settings()
        .bazarr_hook_token
        .as_deref()
        .is_some_and(|t| !t.is_empty() && same_secret(t, &token));
    if !ok {
        // Same answer as a path that doesn't exist.
        return not_found("not found");
    }
    if !SCAN_PENDING.swap(true, Ordering::SeqCst) {
        let st = st.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            SCAN_PENDING.store(false, Ordering::SeqCst);
            st.log_event(
                LogLevel::Info,
                "Bazarr reported a new subtitle; scanning the library for it",
            );
            // A scan may already be running (and may have passed this item):
            // keep trying for a while, so the new subtitle is never missed.
            for _ in 0..20 {
                if tasks::start(&st, Task::LibraryScan).is_ok() {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });
    }
    (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_compare_equal_only_when_identical() {
        assert!(same_secret("abc123", "abc123"));
        assert!(!same_secret("abc123", "abc124"));
        assert!(!same_secret("abc123", "abc12"));
        assert!(!same_secret("", "x"));
    }

    #[test]
    fn the_api_key_is_never_in_the_settings_json() {
        let s = crate::state::AppSettings {
            bazarr_url: Some("http://bazarr:6767".into()),
            bazarr_api_key: Some("top-secret-key".into()),
            bazarr_hook_token: Some("hooktoken".into()),
            bazarr_languages: vec!["en".into()],
            ..Default::default()
        };
        let v = settings_json(&s);
        assert!(!v.to_string().contains("top-secret-key"));
        assert_eq!(v["configured"], true);
        assert_eq!(v["key_set"], true);
        assert_eq!(v["hook_token"], "hooktoken");
        assert_eq!(settings_json(&Default::default())["configured"], false);
    }
}
