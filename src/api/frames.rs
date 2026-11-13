//! What the frame-accurate editor asks of a file: its frame rate, a single
//! frame at a time, and a strip of frames for the timeline.

use super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tokio::sync::Semaphore;

/// Frame and filmstrip renders each decode from the source file; a slider
/// drag asks for many. At most this many run at once, the rest wait.
const MAX_CONCURRENT_STILLS: usize = 4;

/// The widest span a filmstrip may cover: it decodes every frame in it.
const MAX_FILMSTRIP_SPAN_MS: i64 = 120_000;

fn still_slots() -> &'static Semaphore {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| Semaphore::new(MAX_CONCURRENT_STILLS))
}

/// A file's frame rate doesn't change, so probe each one once per run.
fn video_info_cache() -> &'static Mutex<HashMap<PathBuf, ffmpeg::VideoInfo>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, ffmpeg::VideoInfo>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The source file of an indexed item, or the response to send instead.
async fn source_file(
    st: &AppState,
    rating_key: String,
) -> Result<std::result::Result<(db::Media, PathBuf), Response>, AppError> {
    let pool = st.pool.clone();
    let m = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_media(&conn, &rating_key)
    })
    .await??;
    let Some(m) = m else {
        return Ok(Err(not_found("media not indexed")));
    };
    match paths::resolve(&m.file_path, &st.mappings()) {
        paths::AccessMode::Direct(p) => Ok(Ok((m, p))),
        paths::AccessMode::NotMounted => Ok(Err(bad_request(
            "source file is not on a mounted volume; check your path mappings in Settings",
        ))),
    }
}

fn jpeg(bytes: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            // A frame at a time never changes, so let the browser keep it.
            (header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        Body::from(bytes),
    )
        .into_response()
}

/// `GET /api/media/{rating_key}/video-info`: frame rate (as a fraction), size
/// and runtime, so the editor can count in frames.
pub(super) async fn video_info(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
) -> Result<Response, AppError> {
    let (m, file) = match source_file(&st, rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let cached = video_info_cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&file)
        .cloned();
    let info = match cached {
        Some(i) => i,
        None => {
            let i = ffmpeg::probe_video(&st.config.ffprobe_bin, &file).await?;
            video_info_cache()
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(file, i.clone());
            i
        }
    };
    Ok(Json(json!({
        "fps_num": info.fps_num,
        "fps_den": info.fps_den,
        "fps": info.fps,
        "width": info.width,
        "height": info.height,
        "variable": info.variable,
        "duration_ms": m.duration_ms,
    }))
    .into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct FrameParams {
    /// Position in the media, milliseconds.
    t: i64,
    #[serde(default = "frame_width")]
    width: u32,
}

fn frame_width() -> u32 {
    320
}

/// `GET /api/frame/{rating_key}?t=<ms>&width=<px>`: the picture at that moment.
pub(super) async fn frame(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Query(p): Query<FrameParams>,
) -> Result<Response, AppError> {
    if p.t < 0 {
        return Ok(bad_request("t must not be negative"));
    }
    let width = p.width.clamp(80, 1280);
    let (m, file) = match source_file(&st, rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    if m.duration_ms > 0 && p.t >= m.duration_ms {
        return Ok(bad_request("t is past the end of the media"));
    }
    let _slot = still_slots().acquire().await?;
    let args = ffmpeg::frame_args(&file.to_string_lossy(), p.t, width);
    let bytes = ffmpeg::run_capture(&st.config.ffmpeg_bin, &args).await?;
    Ok(jpeg(bytes))
}

#[derive(Debug, Deserialize)]
pub(super) struct FilmstripParams {
    start: i64,
    end: i64,
    #[serde(default = "filmstrip_count")]
    count: u32,
    #[serde(default = "filmstrip_height")]
    height: u32,
}

fn filmstrip_count() -> u32 {
    12
}

fn filmstrip_height() -> u32 {
    60
}

/// `GET /api/filmstrip/{rating_key}?start=&end=&count=&height=`: `count`
/// frames evenly spread over the span, side by side in one JPEG.
pub(super) async fn filmstrip(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Query(p): Query<FilmstripParams>,
) -> Result<Response, AppError> {
    if p.start < 0 || p.end <= p.start {
        return Ok(bad_request("end must be greater than start"));
    }
    if p.end - p.start > MAX_FILMSTRIP_SPAN_MS {
        return Ok(bad_request(format!(
            "a filmstrip covers at most {} seconds; zoom in",
            MAX_FILMSTRIP_SPAN_MS / 1000
        )));
    }
    let count = p.count.clamp(2, 40);
    let height = p.height.clamp(24, 160);
    let (_, file) = match source_file(&st, rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let _slot = still_slots().acquire().await?;
    let args = ffmpeg::filmstrip_args(&file.to_string_lossy(), p.start, p.end, count, height);
    let bytes = ffmpeg::run_capture(&st.config.ffmpeg_bin, &args).await?;
    Ok(jpeg(bytes))
}

/// `POST /api/still`: one frame, with its subtitle or meme text drawn on it,
/// as a PNG, JPEG or GIF. One frame takes a moment, so the answer comes back
/// with the finished file's download address rather than as a job to poll.
pub(super) async fn still_export(
    State(st): State<AppState>,
    Json(req): Json<export::StillRequest>,
) -> Result<Response, AppError> {
    if req.t_ms < 0 {
        return Ok(bad_request("t_ms must not be negative"));
    }
    let (m, file) = match source_file(&st, req.rating_key.clone()).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    if m.duration_ms > 0 && req.t_ms >= m.duration_ms {
        return Ok(bad_request("t_ms is past the end of the media"));
    }
    let font_path = match caption_font_path(&st, req.caption.as_ref()) {
        Ok(p) => p,
        Err(msg) => return Ok(bad_request(msg)),
    };
    // The indexed subtitle lines around that moment, unless the editor sent its own.
    let cues = if matches!(
        req.caption,
        Some(export::Caption::Source { cues: None, .. })
    ) {
        let pool = st.pool.clone();
        let media_id = m.id;
        let pad = req.subtitle_offset_ms.abs();
        let t = req.t_ms;
        tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
            let conn = pool.get()?;
            db::cues_in_window(&conn, media_id, t - pad, t + 1 + pad)
        })
        .await??
    } else {
        Vec::new()
    };

    let id = uuid::Uuid::new_v4().to_string();
    let filename = export::safe_filename_ext(req.filename.as_deref(), req.format.ext(), &id[..8]);
    let out_path = st.config.output_dir.join(&filename);
    let _slot = still_slots().acquire().await?;
    if let Err(e) = export::run_still(
        &st.config.ffmpeg_bin,
        &st.config.tmp_dir(),
        &id,
        &file,
        &req,
        &cues,
        &font_path,
        &out_path,
    )
    .await
    {
        report_export_failure(&st, "Frame export", &out_path, &e);
        return Err(e.into());
    }
    Ok(Json(json!({
        "filename": filename,
        "format": req.format.ext(),
        "download_path": format!("{}/api/download/{filename}", st.config.url_base),
    }))
    .into_response())
}
