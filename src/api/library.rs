//! Library browsing, the Wanted list, stats, and reindexing.

use super::*;

/// Per-library counts (total / indexed / missing subtitles / unreachable) from
/// what has been indexed so far.
pub(super) async fn library_stats(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let stats = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::library_stats(&conn)
    })
    .await??;
    Ok(Json(json!({ "libraries": stats })))
}

#[derive(Debug, Deserialize)]
pub(super) struct MissingParams {
    library: String,
    #[serde(default = "missing_limit")]
    limit: i64,
}

pub(super) fn missing_limit() -> i64 {
    500
}

/// Items in a library that have no indexed subtitles, with the reason
/// (`no_subtitles` = reachable but no text track; `unreachable` = not on a mount).
pub(super) async fn library_missing(
    State(st): State<AppState>,
    Query(p): Query<MissingParams>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let items = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::missing_items(&conn, &p.library, p.limit.clamp(1, 2000))
    })
    .await??;
    Ok(Json(json!({ "items": items })))
}

#[derive(Debug, Deserialize)]
pub(super) struct WantedParams {
    #[serde(default)]
    q: String,
    #[serde(default = "wanted_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

pub(super) fn wanted_limit() -> i64 {
    50
}

/// Items with no indexed subtitles whose file is reachable -- the ones a
/// subtitle download would actually fix. `unreachable` counts the ones left
/// out because their *path mapping* needs fixing first.
pub(super) async fn wanted(
    State(st): State<AppState>,
    Query(p): Query<WantedParams>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let libraries = libraries_from_query(&raw);
    let (items, total, unreachable) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::wanted_items(
            &conn,
            &p.q,
            &libraries,
            p.limit.clamp(1, 200),
            p.offset.max(0),
        )
    })
    .await??;
    Ok(Json(
        json!({ "items": items, "total": total, "unreachable": unreachable }),
    ))
}

#[derive(Debug, Deserialize)]
pub(super) struct BrowseParams {
    #[serde(default)]
    q: String,
    #[serde(default = "browse_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    /// `all` (default), `missing` (some subtitles missing) or `indexed` (none missing).
    #[serde(default)]
    status: String,
}

pub(super) fn browse_limit() -> i64 {
    60
}

/// Library listing grouped by show (movies individually), paginated.
pub(super) async fn library_browse(
    State(st): State<AppState>,
    Query(p): Query<BrowseParams>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let libraries = libraries_from_query(&raw);
    let (entries, total) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::browse_library(
            &conn,
            &p.q,
            &libraries,
            db::BrowseStatus::parse(&p.status),
            p.limit.clamp(1, 200),
            p.offset.max(0),
        )
    })
    .await??;
    Ok(Json(json!({ "entries": entries, "total": total })))
}

#[derive(Debug, Deserialize)]
pub(super) struct EpisodesParams {
    show: String,
}

/// Episodes of one show (for the Library drill-down).
pub(super) async fn library_episodes(
    State(st): State<AppState>,
    Query(p): Query<EpisodesParams>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let episodes = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::episodes_in_show(&conn, &p.show, 2000)
    })
    .await??;
    Ok(Json(json!({ "episodes": episodes })))
}

pub(super) async fn reindex(State(st): State<AppState>) -> impl IntoResponse {
    match tasks::start(&st, Task::LibraryScan) {
        Ok(()) => (StatusCode::ACCEPTED, Json(json!({ "started": true }))),
        Err(why) => (StatusCode::CONFLICT, Json(json!({ "error": why }))),
    }
}

// ---- settings ------------------------------------------------------------
