//! Pure ffmpeg filtergraph and argument construction (caption burn-in, scaling, codecs).

use super::*;

/// Resources an export needs on disk while ffmpeg runs (cleaned up after).
pub struct CaptionFiles {
    pub srt: Option<PathBuf>,
    pub text_files: Vec<PathBuf>,
}

/// Escape a path for embedding inside an ffmpeg filter argument. Colons,
/// backslashes and single quotes are the ones that break the filter parser.
pub(super) fn escape_filter_path(p: &str) -> String {
    p.replace('\\', "/")
        .replace(':', "\\:")
        .replace('\'', "\\'")
}

/// Build the caption portion of a filter chain (`subtitles=`/`drawtext=`),
/// shared by every chain-builder below.
#[allow(clippy::too_many_arguments)]
pub(super) fn caption_filters(
    caption: Option<&Caption>,
    font_path: &str,
    srt_path: Option<&str>,
    top_text_file: Option<&str>,
    bottom_text_file: Option<&str>,
    frame_height_hint: u32,
) -> Vec<String> {
    let mut parts = Vec::new();
    match caption {
        Some(Caption::Source { .. }) => {
            if let Some(srt) = srt_path {
                let fonts_dir = Path::new(font_path)
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                parts.push(format!(
                    "subtitles='{}':fontsdir='{}':force_style='Outline=2,Shadow=0,MarginV=10'",
                    escape_filter_path(srt),
                    escape_filter_path(&fonts_dir),
                ));
            }
        }
        Some(Caption::Meme { font_size, .. }) => {
            let fs = *font_size;
            let margin = (frame_height_hint as f32 * 0.05).max(8.0) as u32;
            if let Some(tf) = top_text_file {
                parts.push(drawtext(font_path, tf, fs, &format!("{margin}")));
            }
            if let Some(bf) = bottom_text_file {
                parts.push(drawtext(font_path, bf, fs, &format!("h-th-{margin}")));
            }
        }
        _ => {}
    }
    parts
}

/// Build the core (pre-palette) filter chain segments for a single, standalone
/// clip: scale to a target width, height following automatically.
#[allow(clippy::too_many_arguments)]
pub fn core_chain(
    width: u32,
    fps: u32,
    caption: Option<&Caption>,
    font_path: &str,
    srt_path: Option<&str>,
    top_text_file: Option<&str>,
    bottom_text_file: Option<&str>,
    frame_height_hint: u32,
) -> String {
    let mut parts = vec![
        format!("fps={fps}"),
        format!("scale={width}:-2:flags=lanczos"),
    ];
    parts.extend(caption_filters(
        caption,
        font_path,
        srt_path,
        top_text_file,
        bottom_text_file,
        frame_height_hint,
    ));
    parts.join(",")
}

/// Even (width, height) 16:9 canvas for a given target width — used to render
/// every clip in a sequence to identical dimensions so they can be
/// concatenated, regardless of each source's own aspect ratio.
pub fn canvas_dims(width: u32) -> (u32, u32) {
    let w = (width - (width % 2)).max(2);
    let h = (((w * 9 / 16) / 2) * 2).max(2);
    (w, h)
}

/// Build the filter chain for one clip inside a multi-clip sequence: scale
/// down to fit a fixed canvas (preserving the source's own aspect ratio) and
/// pad with black bars to fill it exactly, so every clip in the sequence has
/// identical output dimensions and can be concatenated losslessly.
#[allow(clippy::too_many_arguments)]
pub fn clip_core_chain(
    width: u32,
    height: u32,
    fps: u32,
    caption: Option<&Caption>,
    font_path: &str,
    srt_path: Option<&str>,
    top_text_file: Option<&str>,
    bottom_text_file: Option<&str>,
    frame_height_hint: u32,
) -> String {
    let mut parts = vec![
        format!("fps={fps}"),
        format!("scale={width}:{height}:force_original_aspect_ratio=decrease:flags=lanczos"),
        format!("pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black"),
    ];
    parts.extend(caption_filters(
        caption,
        font_path,
        srt_path,
        top_text_file,
        bottom_text_file,
        frame_height_hint,
    ));
    parts.join(",")
}

pub(super) fn drawtext(font_path: &str, text_file: &str, font_size: u32, y_expr: &str) -> String {
    format!(
        "drawtext=fontfile='{}':textfile='{}':fontcolor=white:fontsize={font_size}:\
         borderw=3:bordercolor=black:x=(w-text_w)/2:y={y_expr}",
        escape_filter_path(font_path),
        escape_filter_path(text_file),
    )
}

/// Full `-vf` string for a given format (GIF appends a palette pass).
pub fn video_filter(format: Format, core: &str) -> String {
    match format {
        Format::Gif => format!(
            "{core},split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3"
        ),
        _ => core.to_string(),
    }
}

/// Fixed Opus encode settings shared by every audio-bearing export (and the
/// sequence pipeline's intermediates, so concat can stream-copy audio
/// instead of re-encoding it twice). 48kHz is Opus's native rate -- encoding
/// at anything else means an extra internal resample ffmpeg does for free
/// anyway, so there's no quality reason not to standardize on it.
pub(super) fn opus_args() -> Vec<String> {
    vec![
        "-c:a".into(),
        "libopus".into(),
        "-ar".into(),
        "48000".into(),
        "-ac".into(),
        "2".into(),
        "-b:a".into(),
        "128k".into(),
    ]
}

/// How an encode pass should handle audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    /// Strip audio entirely (`-an`) -- every format supports this, and it's
    /// the default (opt-in feature).
    None,
    /// Encode the source audio to Opus from scratch.
    Encode,
    /// Uncompressed 48 kHz stereo, for intermediates. Sequence parts carry PCM
    /// so the one lossy Opus encode happens over the joined audio: encoding
    /// each part to Opus first leaves codec padding at every join (overlapping
    /// audio timestamps in WebM, a few ms of drift in MP4).
    Pcm,
    /// Pass an already-Opus audio stream through untouched. Used by the
    /// sequence pipeline's final pass: phase 1 already did the one real
    /// (lossy) Opus encode per clip and phase 2's concat is a stream copy,
    /// so re-encoding audio again here would just be a second lossy pass
    /// for no reason.
    Copy,
}

/// Output-codec args appended after `-vf`. `audio` is only ever `Encode`/
/// `Copy` for formats where `Format::supports_audio()` is true -- callers
/// validate that pairing before this is reached (see
/// `validate_audio_format`); passing a non-`None` mode for GIF/WebP here is
/// simply ignored rather than asserted, since this function has no request
/// context to error out with.
pub fn codec_args(format: Format, quality: u32, audio: AudioMode) -> Vec<String> {
    let q = quality.clamp(1, 100);
    match format {
        Format::Gif => vec!["-an".into()],
        Format::Webp => vec![
            "-vcodec".into(),
            "libwebp".into(),
            "-lossless".into(),
            "0".into(),
            "-q:v".into(),
            q.to_string(),
            "-loop".into(),
            "0".into(),
            "-an".into(),
        ],
        Format::Mp4 => {
            let mut args = vec![
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                x264_crf(q).to_string(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-movflags".into(),
                "+faststart".into(),
                // No "x264 - core ... options: ..." banner inside the stream.
                "-x264-params".into(),
                "no-info=1".into(),
            ];
            args.extend(audio_args(audio));
            args
        }
        Format::Webm => {
            let mut args = vec![
                "-c:v".into(),
                "libvpx-vp9".into(),
                "-crf".into(),
                vp9_crf(q).to_string(),
                "-b:v".into(),
                "0".into(),
            ];
            args.extend(audio_args(audio));
            args
        }
    }
}

pub(super) fn audio_args(mode: AudioMode) -> Vec<String> {
    match mode {
        AudioMode::None => vec!["-an".into()],
        AudioMode::Encode => opus_args(),
        AudioMode::Copy => vec!["-c:a".into(), "copy".into()],
        AudioMode::Pcm => vec![
            "-c:a".into(),
            "pcm_s16le".into(),
            "-ar".into(),
            "48000".into(),
            "-ac".into(),
            "2".into(),
        ],
    }
}

/// Map a 1..=100 quality knob to an x264 CRF (0 best .. 51 worst).
pub(super) fn x264_crf(quality: u32) -> u32 {
    // quality 100 -> ~18, quality 1 -> ~38
    let q = quality.clamp(1, 100) as f32;
    (38.0 - (q / 100.0) * 20.0).round() as u32
}

/// Map quality to a VP9 CRF (0 best .. 63 worst).
pub(super) fn vp9_crf(quality: u32) -> u32 {
    let q = quality.clamp(1, 100) as f32;
    (50.0 - (q / 100.0) * 24.0).round() as u32
}

pub(super) fn ms_to_secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// What a file's `title` tag may say: one line, no control characters, not
/// absurdly long. Whatever the source is called, it goes into an argv, not a
/// shell, but a title is shown by players and file managers, so keep it tidy.
pub fn clean_title(title: &str) -> String {
    let one_line: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let tidy = one_line.split_whitespace().collect::<Vec<_>>().join(" ");
    tidy.chars().take(200).collect()
}

/// ffmpeg output options that keep the *source file's* tags out of an export.
///
/// By default ffmpeg copies the input's metadata into the output, which for a
/// movie rip means the original release name (`title`), comments such as "Ripped
/// by ...", track and chapter titles, creation dates, languages and the
/// encoder, and it also carries over subtitle and data tracks. A meme should
/// carry none of that. So: no global, stream or chapter metadata is copied,
/// no subtitle or data streams are kept (captions are burned into the picture),
/// and the muxer and encoder don't stamp their version strings (`bitexact`).
///
/// The one thing that may be written is `title`: the movie or show/episode the
/// clip came from, when `title` is given (MP4 and WebM carry it; GIF and WebP
/// have nowhere to put it).
pub fn privacy_args(title: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-map_metadata",
        "-1",
        "-map_chapters",
        "-1",
        "-map_metadata:s:v",
        "-1",
        "-map_metadata:s:a",
        "-1",
        "-sn",
        "-dn",
        "-fflags",
        "+bitexact",
        "-flags:v",
        "+bitexact",
        "-flags:a",
        "+bitexact",
        // an empty value deletes the tag the muxer would otherwise add
        "-metadata:s:v",
        "encoder=",
        "-metadata:s:a",
        "encoder=",
        "-metadata",
        "encoder=",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(t) = title.map(clean_title).filter(|t| !t.is_empty()) {
        args.push("-metadata".into());
        args.push(format!("title={t}"));
    }
    args
}

/// Assemble the full ffmpeg argv for an export (input seek + filters + codec +
/// privacy options + out). `title` is the one tag the file may carry, see
/// [`privacy_args`].
#[allow(clippy::too_many_arguments)]
pub fn export_args(
    file: &str,
    start_ms: i64,
    end_ms: i64,
    vf: &str,
    codec: &[String],
    title: Option<&str>,
    out: &str,
) -> Vec<String> {
    let dur = (end_ms - start_ms).max(0);
    let mut args = vec![
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
    ];
    args.extend_from_slice(codec);
    args.extend(privacy_args(title));
    args.push(out.into());
    args
}

impl CaptionFiles {
    pub fn cleanup(&self) {
        if let Some(p) = &self.srt {
            let _ = std::fs::remove_file(p);
        }
        for p in &self.text_files {
            let _ = std::fs::remove_file(p);
        }
    }
}
