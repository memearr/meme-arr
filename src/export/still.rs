//! One frame as a picture: PNG, JPEG or a one-frame GIF, with the frame's
//! subtitle or custom meme text drawn on it the same way a clip draws it.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StillFormat {
    Png,
    Jpg,
    Gif,
}

impl StillFormat {
    pub fn ext(self) -> &'static str {
        match self {
            StillFormat::Png => "png",
            StillFormat::Jpg => "jpg",
            StillFormat::Gif => "gif",
        }
    }
}

/// Narrowest and widest picture, in pixels.
pub const MIN_STILL_WIDTH: u32 = 160;
pub const MAX_STILL_WIDTH: u32 = 3840;

/// The width the clip export's meme text size (`font_size`) is relative to.
const MEME_BASE_WIDTH: u32 = 480;

#[derive(Debug, Clone, Deserialize)]
pub struct StillRequest {
    pub rating_key: String,
    /// The moment to take, milliseconds into the media.
    pub t_ms: i64,
    pub format: StillFormat,
    #[serde(default = "still_width")]
    pub width: u32,
    /// 1..=100; only JPEG uses it.
    #[serde(default = "still_quality")]
    pub quality: u32,
    #[serde(default)]
    pub caption: Option<Caption>,
    /// Same meaning as on [`ExportRequest`]: moves the burned subtitles in time.
    #[serde(default)]
    pub subtitle_offset_ms: i64,
    #[serde(default)]
    pub filename: Option<String>,
}

fn still_width() -> u32 {
    960
}

fn still_quality() -> u32 {
    90
}

impl StillRequest {
    pub fn clamped_width(&self) -> u32 {
        self.width.clamp(MIN_STILL_WIDTH, MAX_STILL_WIDTH)
    }
}

/// The caption as it should be drawn on a picture `width` pixels wide: meme text
/// is sized in pixels, so a wider picture needs bigger text to look the same as
/// in a 480-pixel clip. Subtitles scale with the picture by themselves.
pub fn caption_for_width(caption: Option<&Caption>, width: u32) -> Option<Caption> {
    let mut c = caption.cloned();
    if let Some(Caption::Meme { font_size, .. }) = c.as_mut() {
        *font_size =
            ((*font_size as f32 * width as f32 / MEME_BASE_WIDTH as f32).round() as u32).max(6);
    }
    c
}

/// The `-vf` chain: size, caption, then whatever the format needs.
pub fn still_filter(
    format: StillFormat,
    width: u32,
    caption: Option<&Caption>,
    font_path: &str,
    srt_path: Option<&str>,
    top_text_file: Option<&str>,
    bottom_text_file: Option<&str>,
) -> String {
    let mut parts = vec![format!("scale={width}:-2:flags=lanczos")];
    parts.extend(caption_filters(
        caption,
        font_path,
        srt_path,
        top_text_file,
        bottom_text_file,
        (width as f32 * 0.5625) as u32,
    ));
    match format {
        StillFormat::Png => {}
        // the full-range format a JPEG expects, whatever the source uses
        StillFormat::Jpg => parts.push("format=yuvj420p".into()),
        StillFormat::Gif => parts.push(
            "split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3".into(),
        ),
    }
    parts.join(",")
}

/// JPEG quality 1..=100 as ffmpeg's `-q:v` (2 best .. 31 worst).
pub(super) fn jpeg_qscale(quality: u32) -> u32 {
    let q = quality.clamp(1, 100);
    31 - ((q - 1) * 29 + 49) / 99
}

/// The full ffmpeg argv: seek, decode one frame, filter, write one file with
/// none of the source's tags.
pub fn still_args(
    file: &str,
    t_ms: i64,
    vf: &str,
    format: StillFormat,
    quality: u32,
    out: &str,
) -> Vec<String> {
    let mut args: Vec<String> = ["-y", "-v", "error", "-ss"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.push(ms_to_secs(t_ms.max(0)));
    args.extend(["-i", file, "-frames:v", "1", "-an", "-vf", vf].map(String::from));
    match format {
        StillFormat::Png => {
            args.extend(["-c:v", "png", "-f", "image2", "-update", "1"].map(String::from))
        }
        StillFormat::Jpg => {
            args.extend(["-c:v", "mjpeg", "-q:v"].map(String::from));
            args.push(jpeg_qscale(quality).to_string());
            args.extend(["-f", "image2", "-update", "1"].map(String::from));
        }
        StillFormat::Gif => args.extend(["-c:v", "gif", "-f", "gif"].map(String::from)),
    }
    args.extend(privacy_args(None));
    args.push(out.into());
    args
}

/// Render the frame to `out_path`.
#[allow(clippy::too_many_arguments)]
pub async fn run_still(
    ffmpeg_bin: &str,
    tmp_dir: &Path,
    job_id: &str,
    file: &Path,
    req: &StillRequest,
    cues: &[Cue],
    font_path: &str,
    out_path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(tmp_dir)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let width = req.clamped_width();
    let caption = caption_for_width(req.caption.as_ref(), width);
    // A window one millisecond long: the subtitle lines on screen at that
    // moment come out starting at 0, which is where the single frame is.
    let (srt, top, bottom, files) = super::run::write_caption_files(
        tmp_dir,
        job_id,
        caption.as_ref(),
        cues,
        req.t_ms,
        req.t_ms + 1,
        req.subtitle_offset_ms,
    )?;
    let vf = still_filter(
        req.format,
        width,
        caption.as_ref(),
        font_path,
        srt.as_deref(),
        top.as_deref(),
        bottom.as_deref(),
    );
    let args = still_args(
        &file.to_string_lossy(),
        req.t_ms,
        &vf,
        req.format,
        req.quality,
        &out_path.to_string_lossy(),
    );
    let result = ffmpeg::run(ffmpeg_bin, &args).await;
    files.cleanup();
    result
}
