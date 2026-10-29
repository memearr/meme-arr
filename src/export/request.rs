//! Export request types, limits and validation, and output file naming.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Gif,
    Webp,
    Mp4,
    Webm,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Gif => "gif",
            Format::Webp => "webp",
            Format::Mp4 => "mp4",
            Format::Webm => "webm",
        }
    }

    /// Whether this container format can carry an audio stream at all. GIF
    /// and WebP have no audio channel in their format spec — no player will
    /// ever decode sound from either, regardless of encoding effort.
    pub fn supports_audio(self) -> bool {
        matches!(self, Format::Mp4 | Format::Webm)
    }
}

/// How captions are rendered onto the clip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Caption {
    /// No text.
    None,
    /// Burn the real subtitle lines overlapping the window. `cues`, when
    /// present, is the user-edited line list (corrected, shortened, or
    /// reworded in the editor) to burn instead of re-fetching the indexed
    /// lines; `None` means "use whatever's indexed," the original behavior.
    /// Either way the lines still pass through the sync offset uniformly.
    Source {
        #[serde(default)]
        cues: Option<Vec<Cue>>,
        /// A font by file name (see `fonts`); `None` is the default font.
        #[serde(default)]
        font: Option<String>,
        /// Text size as a percentage of the normal size.
        #[serde(default = "default_font_scale")]
        font_scale: u32,
    },
    /// Classic meme caption(s).
    Meme {
        #[serde(default)]
        top: Option<String>,
        #[serde(default)]
        bottom: Option<String>,
        #[serde(default = "default_font_size")]
        font_size: u32,
        #[serde(default)]
        font: Option<String>,
        #[serde(default = "default_font_scale")]
        font_scale: u32,
    },
}

pub(super) fn default_font_size() -> u32 {
    36
}

/// Smallest and largest text size, as a percentage of normal.
pub const MIN_FONT_SCALE: u32 = 25;
pub const MAX_FONT_SCALE: u32 = 400;

pub(super) fn default_font_scale() -> u32 {
    100
}

impl Caption {
    /// Burned subtitles in the default font at normal size.
    pub fn source(cues: Option<Vec<Cue>>) -> Self {
        Caption::Source {
            cues,
            font: None,
            font_scale: default_font_scale(),
        }
    }

    /// Meme text in the default font at normal size.
    pub fn meme(top: Option<String>, bottom: Option<String>, font_size: u32) -> Self {
        Caption::Meme {
            top,
            bottom,
            font_size,
            font: None,
            font_scale: default_font_scale(),
        }
    }

    /// The font chosen for this caption, by file name; `None` means the default font.
    pub fn font_id(&self) -> Option<&str> {
        match self {
            Caption::Source { font, .. } | Caption::Meme { font, .. } => {
                font.as_deref().filter(|f| !f.trim().is_empty())
            }
            Caption::None => None,
        }
    }

    /// The text size as a percentage of normal, kept within sensible limits.
    pub fn font_scale_pct(&self) -> u32 {
        match self {
            Caption::Source { font_scale, .. } | Caption::Meme { font_scale, .. } => {
                (*font_scale).clamp(MIN_FONT_SCALE, MAX_FONT_SCALE)
            }
            Caption::None => 100,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportRequest {
    pub rating_key: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub format: Format,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_quality")]
    pub quality: u32, // 1..=100, higher = better
    #[serde(default)]
    pub caption: Option<Caption>,
    /// Shift burned subtitle timing to correct sync drift: positive delays
    /// (later), negative advances (earlier), milliseconds. Only meaningful
    /// with `caption: Source`.
    #[serde(default)]
    pub subtitle_offset_ms: i64,
    /// Keep the source audio track, encoded as Opus. Only valid when
    /// `format.supports_audio()` -- callers validate this before export
    /// starts (see `validate_audio_format`).
    #[serde(default)]
    pub audio: bool,
    #[serde(default)]
    pub filename: Option<String>,
    /// Write the movie or show/episode the clip came from into the file's
    /// `title` tag (MP4 and WebM). It is the only metadata an export keeps;
    /// everything from the source file is stripped either way.
    #[serde(default = "default_true")]
    pub credit_source: bool,
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_width() -> u32 {
    480
}

pub(super) fn default_fps() -> u32 {
    15
}

pub(super) fn default_quality() -> u32 {
    80
}

/// Per-clip cap, same as a single export.
pub const MAX_CLIP_MS: i64 = 60_000;

/// Total sequence length cap — a "basic" compilation tool, not a feature film.
pub const MAX_SEQUENCE_MS: i64 = 180_000;

/// Upper bound on how many clips one sequence export can contain.
pub const MAX_SEQUENCE_CLIPS: usize = 20;

/// One clip within a multi-clip sequence export.
#[derive(Debug, Clone, Deserialize)]
pub struct SequenceClipSpec {
    pub rating_key: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(default)]
    pub caption: Option<Caption>,
    #[serde(default)]
    pub subtitle_offset_ms: i64,
}

/// Export several clips — possibly from different source files — back to
/// back into one output. Every clip shares the sequence's format/size/quality;
/// each keeps its own in/out window and caption choice.
#[derive(Debug, Clone, Deserialize)]
pub struct SequenceExportRequest {
    pub clips: Vec<SequenceClipSpec>,
    pub format: Format,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_quality")]
    pub quality: u32,
    /// Keep each clip's source audio, encoded as Opus. Sequence-wide, like
    /// format/size/quality -- same constraint as `ExportRequest::audio`.
    #[serde(default)]
    pub audio: bool,
    #[serde(default)]
    pub filename: Option<String>,
    /// As on [`ExportRequest`]: name the sources in the `title` tag.
    #[serde(default = "default_true")]
    pub credit_source: bool,
}

/// What the clip came from, for the file's `title` tag: `Paper Moon Heist
/// (2019)` for a movie, `The Lantern Cove - S02E03 - Tideline` for an episode.
/// Built from the library's own metadata, never from the source file's tags or
/// its name.
pub fn source_label(
    kind: &str,
    title: &str,
    show: Option<&str>,
    season: Option<i64>,
    episode: Option<i64>,
    year: Option<i64>,
) -> String {
    match (kind, show) {
        ("episode", Some(show)) => match (season, episode) {
            (Some(s), Some(e)) => format!("{show} - S{s:02}E{e:02} - {title}"),
            _ => format!("{show} - {title}"),
        },
        _ => match year {
            Some(y) if y > 0 => format!("{title} ({y})"),
            _ => title.to_string(),
        },
    }
}

/// The `title` for a sequence: each distinct source once, in order, joined with
/// " / " (cut short rather than growing without limit).
pub fn combined_source_title(labels: &[String]) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for l in labels {
        if !l.is_empty() && !seen.contains(&l.as_str()) {
            seen.push(l);
        }
    }
    if seen.is_empty() {
        return None;
    }
    let joined = seen.join(" / ");
    Some(if joined.chars().count() > 200 {
        let cut: String = joined.chars().take(197).collect();
        format!("{cut}...")
    } else {
        joined
    })
}

/// Human-readable reason `format`/`audio` can't be exported together, or
/// `None` if they're compatible. GIF and WebP have no audio channel at all.
pub fn validate_audio_format(format: Format, audio: bool) -> Option<String> {
    if audio && !format.supports_audio() {
        Some(format!(
            "{} doesn't support audio -- choose MP4 or WebM instead",
            format.ext().to_uppercase()
        ))
    } else {
        None
    }
}

/// Validate a sequence request's shape before touching Plex/ffmpeg at all.
/// Returns a human-readable reason on failure.
pub fn validate_sequence(clips: &[SequenceClipSpec]) -> std::result::Result<(), String> {
    if clips.is_empty() {
        return Err("a sequence needs at least one clip".into());
    }
    if clips.len() > MAX_SEQUENCE_CLIPS {
        return Err(format!(
            "sequences are limited to {MAX_SEQUENCE_CLIPS} clips"
        ));
    }
    let mut total = 0i64;
    for (i, c) in clips.iter().enumerate() {
        if c.end_ms <= c.start_ms {
            return Err(format!(
                "clip {} (#{i}): end must be after start",
                c.rating_key
            ));
        }
        let len = c.end_ms - c.start_ms;
        if len > MAX_CLIP_MS {
            return Err(format!(
                "clip {} (#{i}) is longer than the {}s per-clip limit",
                c.rating_key,
                MAX_CLIP_MS / 1000
            ));
        }
        total += len;
    }
    if total > MAX_SEQUENCE_MS {
        return Err(format!(
            "the sequence totals {:.0}s, over the {}s limit",
            total as f64 / 1000.0,
            MAX_SEQUENCE_MS / 1000
        ));
    }
    Ok(())
}

/// Sanitize a user-supplied filename to a safe base name (no directories, no
/// traversal), then force the format's extension.
pub fn safe_filename(requested: Option<&str>, format: Format, fallback: &str) -> String {
    let base = requested
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback);
    // Strip directory components first (handle both separators, cross-platform),
    // so "../../etc/passwd" becomes "passwd" before we sanitize.
    let basename = base.rsplit(['/', '\\']).next().unwrap_or(base);
    // Drop any extension the user typed; we append our own.
    let stem = basename
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(basename);
    let cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = cleaned.trim().trim_matches('.');
    let name = if name.is_empty() { fallback } else { name };
    let name = if name.is_empty() { "clip" } else { name };
    format!("{name}.{}", format.ext())
}
