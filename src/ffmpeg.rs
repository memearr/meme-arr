//! Thin orchestration layer over `ffmpeg` / `ffprobe`, plus `gifsicle` for a
//! post-process GIF recompression pass. We shell out rather than bind the C
//! API: ffmpeg does the real work in every language, and a subprocess keeps
//! the binary small and the failure modes simple.
//!
//! The argument-building functions are pure and unit-tested; the `run_*`
//! functions execute them.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

/// A subtitle track discovered by ffprobe.
#[derive(Debug, Clone)]
pub struct SubtitleTrack {
    /// Stream index among subtitle streams (the N in `0:s:N`).
    pub rel_index: usize,
    pub codec: String,
    pub language: Option<String>,
    /// Track title tag (e.g. "SDH", "Forced"); surfaced for future UI track pick.
    #[allow(dead_code)]
    pub title: Option<String>,
}

impl SubtitleTrack {
    /// Whether this track is text-based and therefore indexable without OCR.
    pub fn is_text(&self) -> bool {
        matches!(
            self.codec.as_str(),
            "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "webvtt" | "text" | "eia_608"
        )
    }
}

/// Renders a process exit status for an error message ("1", or "signal" if
/// killed rather than exited).
fn exit_code_display(status: &std::process::ExitStatus) -> String {
    status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "terminated by signal".to_string())
}

/// ffmpeg/ffprobe can exit non-zero with empty stderr (e.g. killed before
/// writing anything) — make that case say so instead of showing a blank
/// reason that looks like a logging bug.
fn stderr_or_fallback(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "(no stderr output)".to_string()
    } else {
        trimmed.to_string()
    }
}

// ---- ffprobe JSON shapes -------------------------------------------------

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Probed facts about a media file.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    pub duration_ms: i64,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub has_audio: bool,
}

pub fn probe_args(file: &str) -> Vec<String> {
    vec![
        "-v".into(),
        // "quiet" suppresses ffprobe's own error output along with everything
        // else, which left failures with no usable reason attached. "error"
        // still keeps stdout clean (JSON only) but lets the real cause (bad
        // file, unsupported codec, permission denied, ...) reach stderr.
        "error".into(),
        "-print_format".into(),
        "json".into(),
        "-show_format".into(),
        "-show_streams".into(),
        file.into(),
    ]
}

pub async fn probe(ffprobe_bin: &str, file: &Path) -> Result<Probe> {
    let out = Command::new(ffprobe_bin)
        .args(probe_args(&file.to_string_lossy()))
        .output()
        .await
        .with_context(|| format!("failed to run {ffprobe_bin}"))?;
    if !out.status.success() {
        bail!(
            "ffprobe failed for {} (exit {}): {}",
            file.display(),
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        );
    }
    let parsed: ProbeOutput = serde_json::from_slice(&out.stdout)
        .with_context(|| format!("could not parse ffprobe output for {}", file.display()))?;
    Ok(parse_probe(parsed))
}

fn parse_probe(parsed: ProbeOutput) -> Probe {
    let duration_ms = parsed
        .format
        .and_then(|f| f.duration)
        .and_then(|d| d.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0) as i64)
        .unwrap_or(0);

    let mut rel_index = 0;
    let mut subtitle_tracks = Vec::new();
    let mut has_audio = false;
    for s in parsed.streams {
        match s.codec_type.as_deref() {
            Some("subtitle") => {
                subtitle_tracks.push(SubtitleTrack {
                    rel_index,
                    codec: s.codec_name.unwrap_or_default(),
                    language: s.tags.get("language").cloned(),
                    title: s.tags.get("title").cloned(),
                });
                rel_index += 1;
            }
            Some("audio") => has_audio = true,
            _ => {}
        }
    }
    Probe {
        duration_ms,
        subtitle_tracks,
        has_audio,
    }
}

/// Pick the best subtitle track to index: first English text track, else first
/// text track of any language.
pub fn pick_track(tracks: &[SubtitleTrack]) -> Option<&SubtitleTrack> {
    tracks
        .iter()
        .find(|t| t.is_text() && matches!(t.language.as_deref(), Some("eng") | Some("en")))
        .or_else(|| tracks.iter().find(|t| t.is_text()))
}

pub fn extract_srt_args(file: &str, rel_index: usize) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-i".into(),
        file.into(),
        "-map".into(),
        format!("0:s:{rel_index}"),
        "-f".into(),
        "srt".into(),
        "-".into(), // write to stdout
    ]
}

/// Extract a subtitle track to SRT text (returned as a string).
pub async fn extract_srt(ffmpeg_bin: &str, file: &Path, rel_index: usize) -> Result<String> {
    let out = Command::new(ffmpeg_bin)
        .args(extract_srt_args(&file.to_string_lossy(), rel_index))
        .output()
        .await
        .with_context(|| format!("failed to run {ffmpeg_bin}"))?;
    if !out.status.success() {
        bail!(
            "subtitle extraction failed (stream {rel_index}) for {} (exit {}): {}",
            file.display(),
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn convert_srt_args(file: &str, charenc: Option<&str>) -> Vec<String> {
    let mut args = vec!["-v".into(), "error".into()];
    // Text that isn't UTF-8 (the usual case for old subtitle files) is
    // dropped line by line, with "Invalid data found", unless ffmpeg is told
    // what it is.
    if let Some(enc) = charenc {
        args.extend(["-sub_charenc".into(), enc.into()]);
    }
    args.extend([
        "-i".into(),
        file.into(),
        "-f".into(),
        "srt".into(),
        "-".into(), // write to stdout
    ]);
    args
}

/// Convert a sidecar subtitle file (.srt/.vtt/.ass/.ssa) to SRT text. ffmpeg
/// handles the format (and most encodings) uniformly.
pub async fn convert_to_srt(ffmpeg_bin: &str, file: &Path) -> Result<String> {
    convert_to_srt_as(ffmpeg_bin, file, None).await
}

/// `convert_to_srt` for a file whose text encoding is known (`charenc`, an
/// iconv name such as `windows-1252`), or `None` for UTF-8.
pub async fn convert_to_srt_as(
    ffmpeg_bin: &str,
    file: &Path,
    charenc: Option<&str>,
) -> Result<String> {
    let out = Command::new(ffmpeg_bin)
        .args(convert_srt_args(&file.to_string_lossy(), charenc))
        .output()
        .await
        .with_context(|| format!("failed to run {ffmpeg_bin}"))?;
    if !out.status.success() {
        bail!(
            "subtitle conversion failed for {} (exit {}): {}",
            file.display(),
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---- preview -------------------------------------------------------------

fn ms_to_secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// Fast-seek + transcode just the requested window to a browser-playable mp4.
/// `-ss` before `-i` makes the seek near-instant; `-t` bounds the duration.
/// `vf` is the full filter chain (scale, plus a caption burn-in when the
/// editor wants the preview to show subtitles) — built by the caller so the
/// preview can match whatever `export::core_chain` would produce for a real
/// export, rather than this function hardcoding a scale-only filter.
pub fn preview_args(file: &str, start_ms: i64, end_ms: i64, vf: &str, out: &str) -> Vec<String> {
    let dur = (end_ms - start_ms).max(0);
    vec![
        "-y".into(),
        "-v".into(),
        "error".into(),
        "-ss".into(),
        ms_to_secs(start_ms),
        "-t".into(),
        ms_to_secs(dur),
        "-i".into(),
        file.into(),
        "-vf".into(),
        vf.into(),
        "-c:v".into(),
        "libx264".into(),
        // Force 8-bit 4:2:0: some sources (10-bit HEVC/HDR rips, 4:4:4, etc.)
        // would otherwise produce a pixel format browsers can't decode, which
        // shows up client-side as a generic "unsupported format" video error.
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-preset".into(),
        "veryfast".into(),
        "-crf".into(),
        "23".into(),
        "-c:a".into(),
        "aac".into(),
        // Stereo: a 5.1/7.1 track (common on remuxes) is a frequent reason a
        // browser can open the preview but never start it, and the preview is
        // only a preview.
        "-ac".into(),
        "2".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-f".into(),
        "mp4".into(),
        out.into(),
    ]
}

pub async fn run(ffmpeg_bin: &str, args: &[String]) -> Result<()> {
    let out = Command::new(ffmpeg_bin)
        .args(args)
        // If whoever is waiting for this gives up (a browser that moved on, a
        // shutdown), stop ffmpeg instead of letting it finish into the void.
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("failed to run {ffmpeg_bin}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "ffmpeg failed (exit {}): {}",
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        ));
    }
    Ok(())
}

/// Map the 1..=100 quality knob to a gifsicle `--lossy` level (0 = least
/// lossy, keeps `-O3`'s free structural optimizations only; ~160 = most
/// aggressive). gifsicle has no documented hard ceiling, but values much
/// past this introduce visible banding on typical dialogue-clip content.
fn gifsicle_lossy_level(quality: u32) -> u32 {
    let q = quality.clamp(1, 100) as f32;
    (160.0 - (q / 100.0) * 160.0).round() as u32
}

/// Re-compress an already-written GIF in place via gifsicle: `-O3` (free
/// structural/palette optimizations, no quality cost) plus a `--lossy` pass
/// sized from `quality`. ffmpeg's palette pipeline alone leaves real size on
/// the table for GIF specifically (no inter-frame prediction, 256-color cap)
/// — gifsicle is the standard second pass for this and typically takes
/// another 30-70% off. Writes to a sibling temp path and renames over the
/// original so a mid-run failure can never leave a truncated file in place.
///
/// Callers should treat failure here as non-fatal: the GIF ffmpeg already
/// wrote is still a valid, playable output, just not as compressed as it
/// could be.
pub async fn gifsicle_optimize(gifsicle_bin: &str, path: &Path, quality: u32) -> Result<()> {
    let tmp = path.with_extension("gifsicle.tmp");
    let out = Command::new(gifsicle_bin)
        .arg("-O3")
        .arg(format!("--lossy={}", gifsicle_lossy_level(quality)))
        .arg("-o")
        .arg(&tmp)
        .arg(path)
        .output()
        .await
        .with_context(|| format!("failed to run {gifsicle_bin}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp);
        bail!(
            "gifsicle optimization failed for {} (exit {}): {}",
            path.display(),
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        );
    }
    std::fs::rename(&tmp, path)
        .with_context(|| format!("could not replace {} with optimized GIF", path.display()))?;
    Ok(())
}

// ---- video info, single frames and filmstrips ----------------------------

/// What the editor needs to know about a file's picture to work in frames.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VideoInfo {
    /// Frame rate as a fraction (24000/1001 for "23.976"), so frame <-> time
    /// conversions don't accumulate a rounding error.
    pub fps_num: u32,
    pub fps_den: u32,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// The stream's average and nominal rates disagree: frames are not evenly
    /// spaced, so a frame number is only approximate.
    pub variable: bool,
}

#[derive(Debug, Deserialize)]
struct VideoProbeOutput {
    #[serde(default)]
    streams: Vec<VideoProbeStream>,
}

#[derive(Debug, Deserialize)]
struct VideoProbeStream {
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

/// `"24000/1001"` or `"25"` -> `(24000, 1001)`; `None` for `"0/0"`, zero or junk.
fn parse_rate(s: &str) -> Option<(u32, u32)> {
    let (n, d) = match s.split_once('/') {
        Some((n, d)) => (n.trim().parse::<u32>().ok()?, d.trim().parse::<u32>().ok()?),
        None => (s.trim().parse::<u32>().ok()?, 1),
    };
    if n == 0 || d == 0 {
        return None;
    }
    Some((n, d))
}

fn rate_value((n, d): (u32, u32)) -> f64 {
    n as f64 / d as f64
}

fn parse_video_info(parsed: VideoProbeOutput) -> Option<VideoInfo> {
    let s = parsed.streams.into_iter().next()?;
    let avg = s.avg_frame_rate.as_deref().and_then(parse_rate);
    let nominal = s.r_frame_rate.as_deref().and_then(parse_rate);
    let (fps_num, fps_den) = avg.or(nominal)?;
    // Containers often report a doubled "nominal" rate (field rate) for
    // evenly spaced video, so only a clearly different pair means VFR.
    let variable = match (avg, nominal) {
        (Some(a), Some(n)) => {
            let (a, n) = (rate_value(a), rate_value(n));
            (a - n).abs() / n > 0.02 && ((n / a) - 2.0).abs() > 0.02
        }
        _ => false,
    };
    Some(VideoInfo {
        fps_num,
        fps_den,
        fps: fps_num as f64 / fps_den as f64,
        width: s.width.unwrap_or(0),
        height: s.height.unwrap_or(0),
        variable,
    })
}

pub fn video_probe_args(file: &str) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-select_streams".into(),
        "v:0".into(),
        "-show_entries".into(),
        "stream=avg_frame_rate,r_frame_rate,width,height".into(),
        "-print_format".into(),
        "json".into(),
        file.into(),
    ]
}

/// The first video stream's frame rate and size.
pub async fn probe_video(ffprobe_bin: &str, file: &Path) -> Result<VideoInfo> {
    let out = Command::new(ffprobe_bin)
        .args(video_probe_args(&file.to_string_lossy()))
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("failed to run {ffprobe_bin}"))?;
    if !out.status.success() {
        bail!(
            "ffprobe failed for {} (exit {}): {}",
            file.display(),
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        );
    }
    let parsed: VideoProbeOutput = serde_json::from_slice(&out.stdout)
        .with_context(|| format!("could not parse ffprobe output for {}", file.display()))?;
    parse_video_info(parsed).with_context(|| format!("{} has no video stream", file.display()))
}

/// One frame at `t_ms`, scaled to `width`, as a JPEG on stdout. `-ss` before
/// `-i` seeks fast and, because the frame is decoded rather than copied, lands
/// on exactly the frame at (or just after) that time.
pub fn frame_args(file: &str, t_ms: i64, width: u32) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-ss".into(),
        ms_to_secs(t_ms.max(0)),
        "-i".into(),
        file.into(),
        "-frames:v".into(),
        "1".into(),
        "-vf".into(),
        // yuvj420p: the full-range format a JPEG expects, whatever the source uses.
        format!("scale={width}:-2:flags=bilinear,format=yuvj420p"),
        "-q:v".into(),
        "5".into(),
        "-f".into(),
        "mjpeg".into(),
        "pipe:1".into(),
    ]
}

/// `count` evenly spaced frames from `[start_ms, end_ms)` tiled into one
/// horizontal JPEG strip, each `height` pixels tall, so a whole filmstrip is
/// one request and one ffmpeg run.
pub fn filmstrip_args(
    file: &str,
    start_ms: i64,
    end_ms: i64,
    count: u32,
    height: u32,
) -> Vec<String> {
    let dur_s = ((end_ms - start_ms).max(1)) as f64 / 1000.0;
    vec![
        "-v".into(),
        "error".into(),
        "-ss".into(),
        ms_to_secs(start_ms.max(0)),
        "-t".into(),
        ms_to_secs(end_ms - start_ms),
        "-i".into(),
        file.into(),
        "-vf".into(),
        format!("fps={count}/{dur_s:.3},scale=-2:{height}:flags=bilinear,tile={count}x1,format=yuvj420p"),
        "-frames:v".into(),
        "1".into(),
        "-q:v".into(),
        "6".into(),
        "-f".into(),
        "mjpeg".into(),
        "pipe:1".into(),
    ]
}

/// Run ffmpeg and return what it wrote to stdout (an image).
pub async fn run_capture(ffmpeg_bin: &str, args: &[String]) -> Result<Vec<u8>> {
    let out = Command::new(ffmpeg_bin)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .with_context(|| format!("failed to run {ffmpeg_bin}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "ffmpeg failed (exit {}): {}",
            exit_code_display(&out.status),
            stderr_or_fallback(&out.stderr),
        ));
    }
    if out.stdout.is_empty() {
        bail!("ffmpeg produced no image (is that time past the end of the file?)");
    }
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    #[test]
    fn subtitle_conversion_names_the_text_encoding_when_it_is_known() {
        let a = convert_srt_args("x.ass", None);
        assert!(!a.contains(&"-sub_charenc".to_string()));
        let b = convert_srt_args("x.ass", Some("windows-1252"));
        let at = b.iter().position(|v| v == "-sub_charenc").unwrap();
        assert_eq!(b[at + 1], "windows-1252");
        assert!(
            at < b.iter().position(|v| v == "-i").unwrap(),
            "before the input"
        );
    }

    use super::*;

    fn video_probe(json: &str) -> Option<VideoInfo> {
        parse_video_info(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn parses_a_fractional_frame_rate() {
        let v = video_probe(
            r#"{"streams":[{"avg_frame_rate":"24000/1001","r_frame_rate":"24000/1001","width":1920,"height":1080}]}"#,
        )
        .unwrap();
        assert_eq!((v.fps_num, v.fps_den), (24000, 1001));
        assert!((v.fps - 23.976).abs() < 0.001);
        assert_eq!((v.width, v.height), (1920, 1080));
        assert!(!v.variable);
    }

    #[test]
    fn falls_back_to_the_nominal_rate_when_the_average_is_unknown() {
        let v =
            video_probe(r#"{"streams":[{"avg_frame_rate":"0/0","r_frame_rate":"25/1"}]}"#).unwrap();
        assert_eq!((v.fps_num, v.fps_den), (25, 1));
    }

    #[test]
    fn a_doubled_nominal_rate_is_not_variable_frame_rate() {
        let v = video_probe(r#"{"streams":[{"avg_frame_rate":"25/1","r_frame_rate":"50/1"}]}"#)
            .unwrap();
        assert!(!v.variable);
        let v = video_probe(r#"{"streams":[{"avg_frame_rate":"20/1","r_frame_rate":"30/1"}]}"#)
            .unwrap();
        assert!(v.variable);
    }

    #[test]
    fn no_usable_rate_or_stream_is_none() {
        assert!(video_probe(r#"{"streams":[]}"#).is_none());
        assert!(
            video_probe(r#"{"streams":[{"avg_frame_rate":"0/0","r_frame_rate":"0/0"}]}"#).is_none()
        );
    }

    #[test]
    fn frame_args_seek_before_the_input_and_ask_for_one_jpeg() {
        let a = frame_args("/m/x.mkv", 2419, 320);
        let ss = a.iter().position(|x| x == "-ss").unwrap();
        let i = a.iter().position(|x| x == "-i").unwrap();
        assert!(ss < i);
        assert_eq!(a[ss + 1], "2.419");
        assert!(a.windows(2).any(|w| w == ["-frames:v", "1"]));
        assert!(a.contains(&"scale=320:-2:flags=bilinear,format=yuvj420p".to_string()));
        assert_eq!(a.last().unwrap(), "pipe:1");
    }

    #[test]
    fn filmstrip_args_tile_the_requested_count_into_one_row() {
        let a = filmstrip_args("/m/x.mkv", 1000, 7000, 12, 60);
        let vf = &a[a.iter().position(|x| x == "-vf").unwrap() + 1];
        assert!(vf.starts_with("fps=12/6.000,"));
        assert!(vf.contains("scale=-2:60"));
        assert!(vf.ends_with("tile=12x1,format=yuvj420p"));
    }

    #[test]
    fn gifsicle_lossy_level_scales_opposite_to_quality() {
        assert_eq!(gifsicle_lossy_level(100), 0);
        assert!(gifsicle_lossy_level(1) > gifsicle_lossy_level(100));
        assert!(gifsicle_lossy_level(1) <= 160);
    }

    #[test]
    fn probe_parses_duration_and_subs() {
        let json = r#"{
            "streams": [
                {"codec_type":"video","codec_name":"h264"},
                {"codec_type":"audio","codec_name":"aac"},
                {"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}},
                {"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"spa"}}
            ],
            "format": {"duration":"120.500"}
        }"#;
        let parsed: ProbeOutput = serde_json::from_str(json).unwrap();
        let probe = parse_probe(parsed);
        assert_eq!(probe.duration_ms, 120_500);
        assert!(probe.has_audio);
        assert_eq!(probe.subtitle_tracks.len(), 2);
        assert_eq!(probe.subtitle_tracks[0].rel_index, 0);
        assert!(probe.subtitle_tracks[0].is_text());
        assert_eq!(probe.subtitle_tracks[1].rel_index, 1);
        assert!(!probe.subtitle_tracks[1].is_text()); // PGS is image-based
    }

    #[test]
    fn probe_reports_no_audio_when_source_has_none() {
        let json = r#"{"streams": [{"codec_type":"video","codec_name":"h264"}]}"#;
        let parsed: ProbeOutput = serde_json::from_str(json).unwrap();
        assert!(!parse_probe(parsed).has_audio);
    }

    #[test]
    fn picks_english_text_track() {
        let tracks = vec![
            SubtitleTrack {
                rel_index: 0,
                codec: "hdmv_pgs_subtitle".into(),
                language: Some("eng".into()),
                title: None,
            },
            SubtitleTrack {
                rel_index: 1,
                codec: "subrip".into(),
                language: Some("spa".into()),
                title: None,
            },
            SubtitleTrack {
                rel_index: 2,
                codec: "subrip".into(),
                language: Some("eng".into()),
                title: None,
            },
        ];
        assert_eq!(pick_track(&tracks).unwrap().rel_index, 2);
    }

    #[test]
    fn preview_uses_fast_seek_before_input() {
        let args = preview_args("/media/x.mkv", 5000, 8000, "scale=480:-2", "/tmp/p.mp4");
        let ss = args.iter().position(|a| a == "-ss").unwrap();
        let i = args.iter().position(|a| a == "-i").unwrap();
        assert!(ss < i, "-ss must precede -i for fast seek");
        assert!(args.contains(&"3.000".to_string())); // duration = 3s
    }

    #[test]
    fn preview_passes_through_the_given_filter_chain() {
        let args = preview_args(
            "/media/x.mkv",
            0,
            1000,
            "scale=480:-2,drawtext=text=hi",
            "/tmp/p.mp4",
        );
        let i = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(args[i + 1], "scale=480:-2,drawtext=text=hi");
    }

    #[test]
    fn preview_audio_is_stereo_so_surround_tracks_cant_stop_it_playing() {
        let args = preview_args("/media/x.mkv", 0, 1000, "scale=480:-2", "/tmp/p.mp4");
        let i = args.iter().position(|a| a == "-ac").expect("-ac present");
        assert_eq!(args[i + 1], "2");
    }

    #[test]
    fn preview_forces_browser_compatible_pixel_format() {
        let args = preview_args("/media/x.mkv", 0, 1000, "scale=480:-2", "/tmp/p.mp4");
        let i = args
            .iter()
            .position(|a| a == "-pix_fmt")
            .expect("-pix_fmt present");
        assert_eq!(args[i + 1], "yuv420p");
    }

    #[test]
    fn extract_srt_writes_to_stdout() {
        let args = extract_srt_args("/media/x.mkv", 1);
        assert_eq!(args.last().unwrap(), "-");
        assert!(args.contains(&"0:s:1".to_string()));
    }
}
