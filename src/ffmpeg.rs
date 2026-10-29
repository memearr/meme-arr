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

pub fn convert_srt_args(file: &str) -> Vec<String> {
    vec![
        "-v".into(),
        "error".into(),
        "-i".into(),
        file.into(),
        "-f".into(),
        "srt".into(),
        "-".into(), // write to stdout
    ]
}

/// Convert a sidecar subtitle file (.srt/.vtt/.ass/.ssa) to SRT text. ffmpeg
/// handles the format (and most encodings) uniformly.
pub async fn convert_to_srt(ffmpeg_bin: &str, file: &Path) -> Result<String> {
    let out = Command::new(ffmpeg_bin)
        .args(convert_srt_args(&file.to_string_lossy()))
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

#[cfg(test)]
mod tests {
    use super::*;

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
