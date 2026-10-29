//! Subtitles the media server keeps for itself.
//!
//! Plex stores a subtitle it downloaded (or that was uploaded) in its own
//! database, not as a file beside the video, so a sidecar scan never sees it.
//! It does list the subtitle among the item's streams and serves the text at
//! `/library/streams/{id}`; this module picks the right stream, fetches it and
//! turns it into the same cues a sidecar file would give.

use crate::mediaserver::ServerSubtitle;
use crate::state::AppState;
use crate::{ffmpeg, subtitles};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Subtitle formats that are pictures, not text: nothing to index.
const IMAGE_FORMATS: &[&str] = &[
    "pgs",
    "hdmv_pgs_subtitle",
    "vobsub",
    "dvd_subtitle",
    "dvdsub",
    "dvb_subtitle",
    "sup",
    "idx",
];

fn is_text_format(format: Option<&str>) -> bool {
    match format {
        Some(f) => !IMAGE_FORMATS.contains(&f.to_lowercase().as_str()),
        None => true,
    }
}

fn is_english(s: &ServerSubtitle) -> bool {
    s.language.as_deref().is_some_and(|l| {
        let l = l.to_lowercase();
        l == "en" || l == "eng" || l.starts_with("english")
    })
}

/// The external, text-based stream worth indexing: English before other
/// languages, and an ordinary one before a forced-only or hearing-impaired one.
pub fn pick_stream(streams: &[ServerSubtitle]) -> Option<&ServerSubtitle> {
    streams
        .iter()
        .filter(|s| s.external && s.key.is_some() && is_text_format(s.format.as_deref()))
        .min_by_key(|s| {
            (if is_english(s) { 0 } else { 2 }) + u8::from(s.forced || s.hearing_impaired)
        })
}

/// Bytes to text: UTF-8 (with or without a BOM), else Latin-1, which at least
/// keeps every byte instead of failing on an old Windows-1252 file.
pub fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Cues from a subtitle's bytes. SRT and WebVTT parse directly; anything else
/// (ASS/SSA) goes through ffmpeg via a short-lived temp file.
pub async fn cues_from_bytes(
    ffmpeg_bin: &str,
    format: Option<&str>,
    bytes: &[u8],
) -> Result<Vec<subtitles::Cue>> {
    let text = decode(bytes);
    let cues = subtitles::parse(&text);
    if !cues.is_empty() {
        return Ok(cues);
    }
    if text.trim().is_empty() {
        return Ok(cues);
    }
    let ext = match format.map(str::to_lowercase).as_deref() {
        Some("ass") | Some("ssa") => "ass",
        _ if text.contains("[Script Info]") => "ass",
        _ => "srt",
    };
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = std::env::temp_dir().join(format!(
        "memearr-sub-{}-{}.{ext}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    tokio::fs::write(&tmp, text.as_bytes()).await?;
    let converted = ffmpeg::convert_to_srt(ffmpeg_bin, &tmp).await;
    let _ = tokio::fs::remove_file(&tmp).await;
    Ok(subtitles::parse(&converted?))
}

/// Language names to the tags used in subtitle file names (`Movie.en.srt`).
const LANGUAGE_TAGS: &[(&str, &str)] = &[
    ("english", "en"),
    ("spanish", "es"),
    ("french", "fr"),
    ("german", "de"),
    ("italian", "it"),
    ("portuguese", "pt"),
    ("dutch", "nl"),
    ("swedish", "sv"),
    ("norwegian", "no"),
    ("danish", "da"),
    ("finnish", "fi"),
    ("polish", "pl"),
    ("russian", "ru"),
    ("japanese", "ja"),
    ("korean", "ko"),
    ("chinese", "zh"),
    ("arabic", "ar"),
    ("turkish", "tr"),
    ("greek", "el"),
    ("hebrew", "he"),
    ("hindi", "hi"),
    ("czech", "cs"),
    ("hungarian", "hu"),
    ("romanian", "ro"),
    ("ukrainian", "uk"),
    ("thai", "th"),
    ("vietnamese", "vi"),
    ("indonesian", "id"),
];

/// The tag to put in a subtitle's file name: a 2- or 3-letter code as given, or
/// the code for a language name. `None` for a language we don't know, in which
/// case the file just carries no language tag.
pub fn language_tag(language: Option<&str>) -> Option<String> {
    let l = language?.trim().to_lowercase();
    if (l.len() == 2 || l.len() == 3) && l.chars().all(|c| c.is_ascii_lowercase()) {
        return Some(l);
    }
    let first = l.split(|c: char| !c.is_alphabetic()).next()?;
    LANGUAGE_TAGS
        .iter()
        .find(|(name, _)| *name == first)
        .map(|(_, tag)| tag.to_string())
}

/// Where the counterpart `.srt` for a server-kept subtitle goes: beside the
/// video, named after it (`Video.en.srt`, `Video.en.forced.srt`).
pub fn counterpart_path(video: &Path, stream: &ServerSubtitle) -> Option<PathBuf> {
    let dir = video.parent()?;
    let stem = video.file_stem()?.to_string_lossy();
    let mut name = stem.to_string();
    if let Some(tag) = language_tag(stream.language.as_deref()) {
        name.push('.');
        name.push_str(&tag);
    }
    if stream.forced {
        name.push_str(".forced");
    }
    name.push_str(".srt");
    Some(dir.join(name))
}

/// Write `srt` to `path` unless something is already there. `Ok(false)` means a
/// file of that name exists and was left alone (it may be a hand-made one).
pub fn write_counterpart(path: &Path, srt: &str) -> std::io::Result<bool> {
    use std::io::Write;
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(e) => return Err(e),
    };
    if let Err(e) = f.write_all(srt.as_bytes()) {
        drop(f);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(true)
}

/// SRT text for a stream: the server's own file when it already is SRT,
/// otherwise rendered from the parsed cues (styling is dropped).
fn srt_for(stream: &ServerSubtitle, original: &str, cues: &[subtitles::Cue]) -> String {
    let is_srt = matches!(
        stream.format.as_deref().map(str::to_lowercase).as_deref(),
        Some("srt") | Some("subrip")
    );
    if is_srt && !cues.is_empty() {
        original.to_string()
    } else {
        subtitles::trimmed_srt(cues, 0, i64::MAX, 0)
    }
}

/// What was read from the server.
pub struct RemoteSubtitle {
    pub cues: Vec<subtitles::Cue>,
    pub description: String,
    pub stream: ServerSubtitle,
    /// The subtitle as SRT text, ready to be saved as a file.
    pub srt: String,
}

/// The subtitle the server keeps for this item, as cues. `None` when the server
/// has nothing external worth reading (or isn't one that keeps them itself).
pub async fn read_for_item(state: &AppState, rating_key: &str) -> Result<Option<RemoteSubtitle>> {
    let server = state.media_server()?;
    if server.kind() != crate::mediaserver::ServerKind::Plex {
        return Ok(None);
    }
    let streams = server.item_subtitle_streams(rating_key).await?;
    let Some(stream) = pick_stream(&streams) else {
        return Ok(None);
    };
    let key = stream.key.clone().unwrap_or_default();
    let bytes = server.subtitle_stream_bytes(&key).await?;
    let cues = cues_from_bytes(&state.config.ffmpeg_bin, stream.format.as_deref(), &bytes).await?;
    let srt = srt_for(stream, &decode(&bytes), &cues);
    Ok(Some(RemoteSubtitle {
        cues,
        description: format!(
            "{} ({})",
            stream.name,
            stream.format.as_deref().unwrap_or("unknown format")
        ),
        stream: stream.clone(),
        srt,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(lang: &str, format: &str, external: bool, forced: bool) -> ServerSubtitle {
        ServerSubtitle {
            name: format!("{lang} {format}"),
            language: Some(lang.into()),
            format: Some(format.into()),
            external,
            path: None,
            key: external.then(|| "/library/streams/1".to_string()),
            forced,
            hearing_impaired: false,
        }
    }

    #[test]
    fn picks_the_external_english_text_stream() {
        let streams = vec![
            stream("French", "srt", true, false),
            stream("English", "srt", false, false),
            stream("English", "pgs", true, false),
            stream("English", "srt", true, false),
        ];
        let picked = pick_stream(&streams).unwrap();
        assert_eq!(picked.language.as_deref(), Some("English"));
        assert_eq!(picked.format.as_deref(), Some("srt"));
        assert!(picked.external);
    }

    #[test]
    fn prefers_an_ordinary_stream_over_a_forced_one() {
        let streams = vec![
            stream("English", "srt", true, true),
            stream("English", "srt", true, false),
        ];
        assert!(!pick_stream(&streams).unwrap().forced);
    }

    #[test]
    fn takes_another_language_rather_than_nothing_and_never_a_picture() {
        assert!(pick_stream(&[stream("French", "srt", true, false)]).is_some());
        assert!(pick_stream(&[stream("English", "pgs", true, false)]).is_none());
        assert!(pick_stream(&[stream("English", "srt", false, false)]).is_none());
        assert!(pick_stream(&[]).is_none());
    }

    #[test]
    fn language_names_and_codes_become_file_name_tags() {
        assert_eq!(language_tag(Some("English")).as_deref(), Some("en"));
        assert_eq!(language_tag(Some("en")).as_deref(), Some("en"));
        assert_eq!(language_tag(Some("eng")).as_deref(), Some("eng"));
        assert_eq!(
            language_tag(Some("Portuguese (Brazil)")).as_deref(),
            Some("pt")
        );
        assert_eq!(language_tag(Some("Klingon")), None);
        assert_eq!(language_tag(None), None);
        // nothing path-like can slip into a file name
        assert_eq!(language_tag(Some("../../x")), None);
    }

    #[test]
    fn the_counterpart_sits_beside_the_video_and_is_named_after_it() {
        let video = Path::new("/media/Show/S01E02.mkv");
        let en = stream("English", "srt", true, false);
        assert_eq!(
            counterpart_path(video, &en).unwrap(),
            Path::new("/media/Show/S01E02.en.srt")
        );
        let forced = stream("English", "srt", true, true);
        assert_eq!(
            counterpart_path(video, &forced).unwrap(),
            Path::new("/media/Show/S01E02.en.forced.srt")
        );
        let unknown = stream("Klingon", "srt", true, false);
        assert_eq!(
            counterpart_path(video, &unknown).unwrap(),
            Path::new("/media/Show/S01E02.srt")
        );
    }

    #[test]
    fn writing_never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("V.en.srt");
        assert!(write_counterpart(&p, "first").unwrap());
        assert!(!write_counterpart(&p, "second").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "first");
        // an unwritable location is an error, not a panic
        assert!(write_counterpart(&dir.path().join("no/such/dir/V.srt"), "x").is_err());
    }

    #[test]
    fn srt_is_kept_as_is_and_other_formats_are_rendered() {
        let orig = "1\n00:00:01,000 --> 00:00:02,000\nHi\n\n";
        let cues = subtitles::parse(orig);
        assert_eq!(
            srt_for(&stream("English", "srt", true, false), orig, &cues),
            orig
        );
        let rendered = srt_for(&stream("English", "ass", true, false), "ignored", &cues);
        assert_eq!(subtitles::parse(&rendered), cues);
        assert!(rendered.starts_with("1\n00:00:01,000 --> 00:00:02,000\nHi"));
    }

    #[test]
    fn decodes_utf8_with_bom_and_falls_back_to_latin1() {
        assert_eq!(decode(b"\xEF\xBB\xBFcaf\xc3\xa9"), "café");
        assert_eq!(decode(b"caf\xe9"), "café");
    }

    #[tokio::test]
    async fn srt_bytes_become_timed_cues() {
        let srt = b"1\n00:00:01,000 --> 00:00:03,500\nI am the one who knocks\n\n2\n00:00:04,000 --> 00:00:05,000\nSay my name\n\n";
        let cues = cues_from_bytes("ffmpeg-not-needed", Some("srt"), srt)
            .await
            .unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!((cues[0].start_ms, cues[0].end_ms), (1000, 3500));
        assert_eq!(cues[1].text, "Say my name");
    }

    #[tokio::test]
    async fn empty_bytes_are_no_cues_not_an_error() {
        assert!(cues_from_bytes("ffmpeg-not-needed", None, b"  \n")
            .await
            .unwrap()
            .is_empty());
    }
}
