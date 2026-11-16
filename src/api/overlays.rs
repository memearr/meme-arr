//! Overlays: the pictures and clips an export can lay over a clip, and the built-in
//! shapes. List, upload, delete, fetch the file (for the editor's preview), and draw a
//! shape.

use super::*;
use crate::overlays::{content_type, OverlayError, MAX_OVERLAYS, MAX_OVERLAY_BYTES};
use axum::body::Bytes;

fn overlay_error(e: OverlayError) -> Response {
    let status = match e {
        OverlayError::Invalid(_) => StatusCode::BAD_REQUEST,
        OverlayError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        OverlayError::Limit => StatusCode::CONFLICT,
        OverlayError::NotFound => StatusCode::NOT_FOUND,
    };
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}

/// `GET /api/overlays`: the uploaded overlays and the built-in shapes.
pub(super) async fn list_overlays(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let overlays = st.config.overlays();
    let list = tokio::task::spawn_blocking(move || overlays.list()).await?;
    let shapes: Vec<_> = crate::sprite::SHAPES
        .iter()
        .map(|(id, label)| json!({ "id": id, "label": label }))
        .collect();
    Ok(Json(json!({
        "overlays": list,
        "shapes": shapes,
        "max_bytes": MAX_OVERLAY_BYTES,
        "max_overlays": MAX_OVERLAYS,
    })))
}

#[derive(Debug, Deserialize)]
pub(super) struct OverlayUpload {
    /// The file's name (the body is just the file's bytes).
    name: String,
}

/// `POST /api/overlays?name=<file name>`: keep an uploaded picture or clip.
pub(super) async fn upload_overlay(
    State(st): State<AppState>,
    Query(p): Query<OverlayUpload>,
    body: Bytes,
) -> Result<Response, AppError> {
    let overlays = st.config.overlays();
    let name = p.name.clone();
    let stored = tokio::task::spawn_blocking(move || overlays.store(&name, &body)).await?;
    match stored {
        Ok(info) => {
            st.log_event(LogLevel::Info, format!("Overlay added: {}", info.id));
            Ok((StatusCode::CREATED, Json(json!({ "overlay": info }))).into_response())
        }
        Err(e) => Ok(overlay_error(e)),
    }
}

/// `DELETE /api/overlays/{id}`.
pub(super) async fn delete_overlay(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let overlays = st.config.overlays();
    let id2 = id.clone();
    let removed = tokio::task::spawn_blocking(move || overlays.remove(&id2)).await?;
    match removed {
        Ok(()) => {
            st.log_event(LogLevel::Info, format!("Overlay removed: {id}"));
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Err(e) => Ok(overlay_error(e)),
    }
}

/// `GET /api/overlays/{id}/file`: the stored file itself (for the editor's preview).
pub(super) async fn overlay_file(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    match st.config.overlays().path_of(&id) {
        Some(path) => serve_file(&path, content_type(&id), &headers).await,
        None => Ok(overlay_error(OverlayError::NotFound)),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct SpriteParams {
    /// `#RRGGBB` (or `RRGGBB`); the default is orange.
    #[serde(default)]
    color: Option<String>,
}

/// `GET /api/sprites/{shape}?color=`: a built-in shape as the PNG an export overlays,
/// so the editor previews exactly what is exported.
pub(super) async fn shape_sprite(
    Path(shape): Path<String>,
    Query(p): Query<SpriteParams>,
) -> Result<Response, AppError> {
    let shape = shape.trim_end_matches(".png");
    let Some(kind) = crate::sprite::Shape::parse(shape) else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no such shape" })),
        )
            .into_response());
    };
    let rgb = match p.color.as_deref() {
        Some(c) => match export::parse_hex_color(c) {
            Some(rgb) => rgb,
            None => return Ok(bad_request("color must be #RRGGBB")),
        },
        None => crate::sprite::DEFAULT_COLOR,
    };
    let png = tokio::task::spawn_blocking(move || crate::sprite::shape_png(kind, rgb)).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        png,
    )
        .into_response())
}

#[cfg(test)]
#[path = "overlays_tests.rs"]
mod tests;
