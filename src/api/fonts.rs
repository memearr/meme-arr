//! Caption fonts: list them, upload one, delete one.

use super::*;
use crate::fonts::{FontError, FontInfo, MAX_FONT_BYTES};
use axum::body::Bytes;

/// The font file a caption uses: the one it chose, or the default font.
pub(super) fn caption_font_path(
    st: &AppState,
    caption: Option<&export::Caption>,
) -> Result<String, String> {
    match caption.and_then(|c| c.font_id()) {
        Some(id) => st
            .config
            .fonts()
            .path_of(id)
            .map(|p| p.to_string_lossy().to_string())
            .ok_or_else(|| format!("unknown font {id:?}; see Settings -> Fonts")),
        None => Ok(default_font(&st.config.fonts_dir)),
    }
}

fn font_error(e: FontError) -> Response {
    let status = match e {
        FontError::Invalid(_) => StatusCode::BAD_REQUEST,
        FontError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        FontError::Exists | FontError::Limit => StatusCode::CONFLICT,
        FontError::NotFound => StatusCode::NOT_FOUND,
        FontError::BuiltIn => StatusCode::BAD_REQUEST,
    };
    (status, Json(json!({ "error": e.to_string() }))).into_response()
}

/// `GET /api/fonts`: every font a caption can use, and which one is the default.
pub(super) async fn list_fonts(State(st): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let fonts = st.config.fonts();
    let default_file = std::path::Path::new(&default_font(&st.config.fonts_dir))
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let (list, default) = tokio::task::spawn_blocking(move || {
        let list = fonts.list();
        let default: Option<FontInfo> = list.iter().find(|f| f.id == default_file).cloned();
        (list, default)
    })
    .await?;
    Ok(Json(json!({
        "fonts": list,
        "default": default,
        "max_bytes": MAX_FONT_BYTES,
        "max_fonts": crate::fonts::MAX_USER_FONTS,
    })))
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadParams {
    /// The font's file name (the body is just the file's bytes).
    name: String,
}

/// `POST /api/fonts?name=<file name>`: keep an uploaded `.ttf` or `.otf` font.
pub(super) async fn upload_font(
    State(st): State<AppState>,
    Query(p): Query<UploadParams>,
    body: Bytes,
) -> Result<Response, AppError> {
    let fonts = st.config.fonts();
    let name = p.name.clone();
    let stored = tokio::task::spawn_blocking(move || fonts.store(&name, &body)).await?;
    match stored {
        Ok(font) => {
            st.log_event(
                LogLevel::Info,
                format!("Font added: {} ({})", font.family, font.id),
            );
            Ok((StatusCode::CREATED, Json(json!({ "font": font }))).into_response())
        }
        Err(e) => Ok(font_error(e)),
    }
}

/// `DELETE /api/fonts/{id}`: remove an uploaded font.
pub(super) async fn delete_font(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let fonts = st.config.fonts();
    let id2 = id.clone();
    let removed = tokio::task::spawn_blocking(move || fonts.remove(&id2)).await?;
    match removed {
        Ok(()) => {
            st.log_event(LogLevel::Info, format!("Font removed: {id}"));
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Err(e) => Ok(font_error(e)),
    }
}
