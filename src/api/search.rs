//! Libraries, dialogue search, title search, and media details.

use super::*;
use crate::sync::RwLockExt;

pub(super) async fn libraries(State(st): State<AppState>) -> Result<Response, AppError> {
    // Not configured yet is a state to report (409, like the subtitle
    // endpoints), not a server fault (500).
    let server = match st.media_server() {
        Ok(s) => s,
        Err(e) => return Ok(conflict(format!("{e:#}"))),
    };
    let sections = server.sections().await?;
    let out: Vec<_> = sections
        .into_iter()
        .map(
            |s| json!({ "key": s.key, "type": s.kind, "title": s.title, "locations": s.locations }),
        )
        .collect();
    Ok(Json(json!({ "libraries": out })).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchParams {
    q: String,
    #[serde(default = "default_limit")]
    limit: i64,
    /// How many matches to skip (for "show more").
    #[serde(default)]
    offset: i64,
}

pub(super) fn default_limit() -> i64 {
    50
}

/// How many show-name matches to surface alongside search results -- a
/// handful is plenty, this is a shortcut option, not a full result list.
pub(super) const MATCHING_SHOWS_LIMIT: i64 = 5;

pub(super) async fn search(
    State(st): State<AppState>,
    Query(p): Query<SearchParams>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let parsed = querysyntax::parse(&p.q);
    let described = parsed.describe();
    let query = parsed.text.clone();
    let limit = p.limit.clamp(1, 500);
    let offset = p.offset.max(0);
    let libraries = libraries_from_query(&raw);
    let ((results, has_more, listing), shows) = tokio::task::spawn_blocking({
        let pool = pool.clone();
        move || -> anyhow::Result<_> {
            let conn = pool.get()?;
            let scope = db::Scope {
                libraries: &libraries,
                filters: &parsed.filters,
                excluded: &parsed.excluded,
            };
            let results = if !db::has_search_text(&parsed.text) && parsed.has_operators() {
                // Operators but nothing to look for in the dialogue (`show:friends`):
                // list the matching items instead of returning nothing.
                let (hits, more) = db::search_titles(&conn, "", scope, limit, offset)?;
                (json!(hits), more, true)
            } else {
                let (hits, more) =
                    db::search_dialogue_page(&conn, &parsed.text, scope, limit, offset)?;
                (json!(hits), more, false)
            };
            let shows = if offset == 0 {
                db::matching_shows_in(&conn, &parsed.text, scope, MATCHING_SHOWS_LIMIT)?
            } else {
                Vec::new()
            };
            Ok((results, shows))
        }
    })
    .await??;

    // Shadow-mode correctness check against the research compact dialogue
    // index -- detached, never touches `hits` or this response. See
    // `compact_rebuild`'s doc comment for why this exists; a no-op until an
    // index has actually been built/loaded.
    shadow_check_compact_index(st, pool, query);

    Ok(Json(json!({
        "results": results,
        "shows": shows,
        "has_more": has_more,
        "offset": offset,
        "filters": described,
        // true when there was no dialogue text and the list is items, not lines
        "listing": listing,
    })))
}

/// Compare the compact index's candidate-blocks-then-verify answer against
/// FTS5's own native phrase match for the same query, logging any
/// disagreement (never alters a real response). Spawned detached so it adds
/// zero latency to the request that triggered it.
pub(super) fn shadow_check_compact_index(st: AppState, pool: db::Pool, query: String) {
    tokio::spawn(async move {
        let check = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let compact_guard = st.compact.read_locked();
            let Some(compact) = compact_guard.as_ref() else {
                return Ok(());
            };

            let terms = crate::compactindex::tokenize(&query);
            if terms.is_empty() {
                return Ok(());
            }
            let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();

            let mut candidate_rows: Vec<u32> = Vec::new();
            for block_id in compact.query_candidate_blocks(&term_refs) {
                candidate_rows.extend(compact.block_range(block_id));
            }

            let conn = pool.get()?;

            // SQLite has a default bound-parameter limit (999) -- an
            // all-stopword fallback query on a huge corpus could plausibly
            // exceed it. This is a best-effort diagnostic, not a
            // correctness gate, so just skip rather than risk a query
            // error flooding the logs.
            if candidate_rows.len() > 900 {
                return Ok(());
            }

            let mut compact_hits: Vec<i64> = if candidate_rows.is_empty() {
                Vec::new()
            } else {
                let placeholders = vec!["?"; candidate_rows.len()].join(",");
                let sql = format!("SELECT id, text FROM subtitle_cue WHERE id IN ({placeholders})");
                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<&dyn rusqlite::ToSql> = candidate_rows
                    .iter()
                    .map(|r| r as &dyn rusqlite::ToSql)
                    .collect();
                let rows: Vec<(i64, String)> = stmt
                    .query_map(params.as_slice(), |r| {
                        let id: i64 = r.get(0)?;
                        let text: String = r.get(1)?;
                        Ok((id, text))
                    })?
                    .filter_map(Result::ok)
                    .collect();
                rows.into_iter()
                    .filter(|(_, text)| crate::compactindex::phrase_matches(text, &terms))
                    .map(|(id, _)| id)
                    .collect()
            };
            compact_hits.sort_unstable();

            let fts_query = format!("\"{}\"", terms.join(" "));
            let mut stmt =
                conn.prepare("SELECT rowid FROM subtitle_fts WHERE subtitle_fts MATCH ?1")?;
            let mut fts_hits: Vec<i64> = stmt
                .query_map([fts_query], |r| r.get::<_, i64>(0))?
                .filter_map(Result::ok)
                .collect();
            fts_hits.sort_unstable();

            if compact_hits != fts_hits {
                warn!(
                    "compact index shadow check disagreed with FTS5 for a dialogue search: \
                     compact={} rows, fts5={} rows",
                    compact_hits.len(),
                    fts_hits.len()
                );
                st.log_event(
                    LogLevel::Warn,
                    format!(
                        "Shadow check: compact dialogue index disagreed with FTS5 \
                         (compact={} rows, fts5={} rows) -- logged for review, results unaffected",
                        compact_hits.len(),
                        fts_hits.len()
                    ),
                );
            }
            Ok(())
        })
        .await;

        if let Ok(Err(e)) = check {
            warn!("compact index shadow check failed: {e:#}");
        }
    });
}

pub(super) async fn search_title(
    State(st): State<AppState>,
    Query(p): Query<SearchParams>,
    RawQuery(raw): RawQuery,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let parsed = querysyntax::parse(&p.q);
    let described = parsed.describe();
    let limit = p.limit.clamp(1, 500);
    let offset = p.offset.max(0);
    let libraries = libraries_from_query(&raw);
    let ((results, has_more), shows) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        let scope = db::Scope {
            libraries: &libraries,
            filters: &parsed.filters,
            excluded: &parsed.excluded,
        };
        let hits = db::search_titles(&conn, &parsed.text, scope, limit, offset)?;
        let shows = if offset == 0 {
            db::matching_shows_in(&conn, &parsed.text, scope, MATCHING_SHOWS_LIMIT)?
        } else {
            Vec::new()
        };
        Ok((hits, shows))
    })
    .await??;
    Ok(Json(json!({
        "results": results,
        "shows": shows,
        "has_more": has_more,
        "offset": offset,
        "filters": described,
    })))
}

pub(super) async fn media(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
) -> Result<Response, AppError> {
    let pool = st.pool.clone();
    let m = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_media(&conn, &rating_key)
    })
    .await??;
    match m {
        Some(m) => Ok(Json(m).into_response()),
        None => Ok(not_found("media not indexed")),
    }
}

// ---- online subtitle search (Plex and Jellyfin) ----------------------------
