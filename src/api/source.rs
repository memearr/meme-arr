//! What a request about one item needs first: its source file on disk and the
//! subtitle lines a caption will draw. Previews, exports, sequences and single
//! frames all start here, so they all answer the same way.

use super::*;
use std::path::PathBuf;

/// The answer when an item's file is not reachable from this machine.
pub(super) const NOT_MOUNTED: &str =
    "source file is not on a mounted volume; check your path mappings in Settings";

/// The result of looking an item up: its row and file, or the response to send
/// instead (`404` not indexed, `400` not on a mounted volume).
pub(super) type Source = std::result::Result<(db::Media, PathBuf), Response>;

/// The media row and source file of an indexed item.
pub(super) async fn resolve_source(st: &AppState, rating_key: &str) -> Result<Source, AppError> {
    let pool = st.pool.clone();
    let key = rating_key.to_string();
    let m = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_media(&conn, &key)
    })
    .await??;
    let Some(m) = m else {
        return Ok(Err(not_found("media not indexed")));
    };
    match paths::resolve(&m.file_path, &st.mappings()) {
        paths::AccessMode::Direct(p) => Ok(Ok((m, p))),
        paths::AccessMode::NotMounted => Ok(Err(bad_request(NOT_MOUNTED))),
    }
}

/// The indexed subtitle lines a caption needs for `[start_ms, end_ms)`: only for
/// burned subtitles with no line list of their own (an edited list needs no
/// database round trip), and widened by the sync offset so a line that shifts
/// into view, or out of it, is among the candidates. The offset itself is
/// applied when the subtitle file is written, not here.
pub(super) fn source_cues(
    conn: &rusqlite::Connection,
    media_id: i64,
    caption: Option<&export::Caption>,
    (start_ms, end_ms): (i64, i64),
    offset_ms: i64,
) -> anyhow::Result<Vec<subtitles::Cue>> {
    if !matches!(caption, Some(export::Caption::Source { cues: None, .. })) {
        return Ok(Vec::new());
    }
    let pad = offset_ms.abs();
    db::cues_in_window(conn, media_id, start_ms - pad, end_ms + pad)
}

/// [`source_cues`] for a handler that is not already holding a connection.
pub(super) async fn source_cues_for(
    st: &AppState,
    media_id: i64,
    caption: Option<&export::Caption>,
    window: (i64, i64),
    offset_ms: i64,
) -> Result<Vec<subtitles::Cue>, AppError> {
    // Nothing to fetch: skip the trip to the database pool altogether.
    if !matches!(caption, Some(export::Caption::Source { cues: None, .. })) {
        return Ok(Vec::new());
    }
    let pool = st.pool.clone();
    let caption = caption.cloned();
    Ok(tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        source_cues(&conn, media_id, caption.as_ref(), window, offset_ms)
    })
    .await??)
}
