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
    let (m, file) = match resolve_source(&st, &rating_key).await? {
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
    let (m, file) = match resolve_source(&st, &rating_key).await? {
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
    let (_, file) = match resolve_source(&st, &rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let _slot = still_slots().acquire().await?;
    let args = ffmpeg::filmstrip_args(&file.to_string_lossy(), p.start, p.end, count, height);
    let bytes = ffmpeg::run_capture(&st.config.ffmpeg_bin, &args).await?;
    Ok(jpeg(bytes))
}

#[derive(Debug, Deserialize)]
pub(super) struct WaveformParams {
    start: i64,
    end: i64,
    /// How many bars (default 400).
    bins: Option<usize>,
}

/// The longest stretch whose sound is drawn.
const MAX_WAVEFORM_SPAN_MS: i64 = 600_000;

/// `GET /api/waveform/{rating_key}?start=&end=&bins=`: the loudness of the file's own sound
/// over `[start, end)` as bars, for the editor's mix timeline. `{"has_audio": true, "peaks": [..]}`;
/// a file with no sound answers `has_audio: false` and no bars.
pub(super) async fn clip_waveform(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Query(p): Query<WaveformParams>,
) -> Result<Response, AppError> {
    if p.start < 0 || p.end <= p.start {
        return Ok(bad_request("end must be greater than start"));
    }
    if p.end - p.start > MAX_WAVEFORM_SPAN_MS {
        return Ok(bad_request(
            "that is too long a stretch to draw the sound of",
        ));
    }
    let (_, file) = match resolve_source(&st, &rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let silent = !ffmpeg::probe(&st.config.ffprobe_bin, &file)
        .await
        .map(|i| i.has_audio)
        .unwrap_or(false);
    if silent {
        return Ok(Json(json!({ "has_audio": false, "peaks": [] })).into_response());
    }
    let _slot = still_slots().acquire().await?;
    match crate::waveform::analyze_span(
        &st.config.ffmpeg_bin,
        &file,
        p.start,
        p.end - p.start,
        p.bins.unwrap_or(400),
    )
    .await
    {
        Ok(w) => Ok(Json(json!({ "has_audio": true, "peaks": w.peaks })).into_response()),
        // a stretch with nothing in it (past the end of the sound) is just flat
        Err(_) => Ok(Json(json!({ "has_audio": true, "peaks": [] })).into_response()),
    }
}

/// `GET /api/loudness/{rating_key}?start=&end=`: how loud the file's own sound is over `[start, end)`
/// (EBU R128). `{"has_audio": true, "lufs": -14.9, "true_peak_db": -0.4, "range_lu": 5.3}`; `lufs` is
/// `null` when that stretch is silent, and a file with no sound answers `has_audio: false`.
pub(super) async fn clip_loudness(
    State(st): State<AppState>,
    Path(rating_key): Path<String>,
    Query(p): Query<WaveformParams>,
) -> Result<Response, AppError> {
    if p.start < 0 || p.end <= p.start {
        return Ok(bad_request("end must be greater than start"));
    }
    if p.end - p.start > MAX_WAVEFORM_SPAN_MS {
        return Ok(bad_request("that is too long a stretch to measure"));
    }
    let (_, file) = match resolve_source(&st, &rating_key).await? {
        Ok(found) => found,
        Err(resp) => return Ok(resp),
    };
    let has_audio = ffmpeg::probe(&st.config.ffprobe_bin, &file)
        .await
        .map(|i| i.has_audio)
        .unwrap_or(false);
    if !has_audio {
        return Ok(Json(json!({ "has_audio": false, "lufs": null })).into_response());
    }
    let _slot = still_slots().acquire().await?;
    let l =
        crate::waveform::loudness(&st.config.ffmpeg_bin, &file, p.start, p.end - p.start).await?;
    Ok(Json(match l {
        Some(l) => json!({ "has_audio": true, "lufs": l.lufs, "true_peak_db": l.true_peak_db, "range_lu": l.range_lu }),
        None => json!({ "has_audio": true, "lufs": null }),
    })
    .into_response())
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
    let (m, file) = match resolve_source(&st, &req.rating_key).await? {
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
    let moment = (req.t_ms, req.t_ms + export::FRAME_SLACK_MS);
    let cues = source_cues_for(
        &st,
        m.id,
        req.caption.as_ref(),
        moment,
        req.subtitle_offset_ms,
    )
    .await?;

    let id = uuid::Uuid::new_v4().to_string();
    let files = JobFiles::with_ext(&st, req.filename.as_deref(), req.format.ext(), &id);
    let tmp = st.config.tmp_dir();
    let _slot = still_slots().acquire().await?;
    let job = export::JobSpec {
        id: &id,
        tmp_dir: &tmp,
        out_path: &files.out_path,
        source_title: None,
    };
    if let Err(e) = export::run_still(tools(&st), job, &file, &req, &cues, &font_path).await {
        report_export_failure(&st, "Frame export", &files.out_path, &e);
        return Err(e.into());
    }
    let filename = &files.filename;
    Ok(Json(json!({
        "filename": filename,
        "format": req.format.ext(),
        "download_path": files.download,
    }))
    .into_response())
}
