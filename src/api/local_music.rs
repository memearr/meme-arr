//! Songs that are not in Navidrome: upload them, list and remove the uploads, and say which
//! sources of music there are. See `crate::local_music`.

use super::*;
use crate::local_music::{Source, UploadError, MAX_UPLOADS, MAX_UPLOAD_BYTES};
use crate::navidrome::Track;
use axum::body::Bytes;

fn upload_error(e: UploadError) -> Response {
    let status = match e {
        UploadError::Invalid(_) => StatusCode::BAD_REQUEST,
        UploadError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        UploadError::Limit => StatusCode::CONFLICT,
        UploadError::NotFound => StatusCode::NOT_FOUND,
    };
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}

/// A song as the API shows it: its fields, and where it is from.
pub(super) fn song_json(t: &Track, source: Source) -> serde_json::Value {
    let mut v = serde_json::to_value(t).unwrap_or_default();
    v["source"] = json!(source.name());
    v
}

/// `GET /api/music/sources`: which sources of music there are, so the editor knows what to offer.
pub(super) async fn music_sources(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let uploads = st.config.uploads();
    let count = tokio::task::spawn_blocking(move || uploads.list().len()).await?;
    Ok(Json(json!({
        "navidrome": st.settings().navidrome().is_some(),
        "library": st.config.library().is_some(),
        "uploads": count,
        "max_uploads": MAX_UPLOADS,
        "max_upload_bytes": MAX_UPLOAD_BYTES,
    })))
}

/// `GET /api/music/uploads`: the uploaded songs, newest first.
pub(super) async fn list_song_uploads(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let uploads = st.config.uploads();
    let list = tokio::task::spawn_blocking(move || uploads.list()).await?;
    let tracks: Vec<_> = list.iter().map(|t| song_json(t, Source::Upload)).collect();
    Ok(Json(json!({ "tracks": tracks })))
}

#[derive(Debug, Deserialize)]
pub(super) struct SongUpload {
    /// The file's name (the body is just the file's bytes).
    name: String,
}

/// `POST /api/music/uploads?name=<file name>`: keep an uploaded song.
pub(super) async fn upload_song(
    State(st): State<AppState>,
    Query(p): Query<SongUpload>,
    body: Bytes,
) -> Result<Response, AppError> {
    let uploads = st.config.uploads();
    let name = p.name.clone();
    let stored = tokio::task::spawn_blocking(move || uploads.store(&name, &body)).await?;
    match stored {
        Ok(t) => {
            st.log_event(LogLevel::Info, format!("Song uploaded: {}", t.id));
            Ok((
                StatusCode::CREATED,
                Json(json!({ "track": song_json(&t, Source::Upload) })),
            )
                .into_response())
        }
        Err(e) => Ok(upload_error(e)),
    }
}

/// `DELETE /api/music/uploads/{id}`.
pub(super) async fn delete_song_upload(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let uploads = st.config.uploads();
    let id2 = id.clone();
    let removed = tokio::task::spawn_blocking(move || uploads.remove(&id2)).await?;
    match removed {
        Ok(()) => {
            st.log_event(LogLevel::Info, format!("Song removed: {id}"));
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Err(e) => Ok(upload_error(e)),
    }
}

#[cfg(test)]
#[path = "local_music_tests.rs"]
mod tests;
