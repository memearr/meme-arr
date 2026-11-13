//! Finding a sidecar subtitle beside a video, and the change-detection fingerprint that includes it.

use super::*;
use encoding_rs::Encoding;

/// Subtitle sidecar extensions we can turn into searchable text.
pub(super) const SIDECAR_EXTS: &[&str] = &["srt", "vtt", "ass", "ssa"];

/// Find a sidecar subtitle file next to the video: a text subtitle in the same
/// folder whose name starts with the video's base name (e.g. `Movie.en.srt`
/// beside `Movie.mkv`). Prefers English-tagged files, then `.srt`.
pub(crate) fn find_sidecar(video: &Path) -> Option<PathBuf> {
    let dir = video.parent()?;
    let stem = video.file_stem()?.to_string_lossy().to_lowercase();

    let mut best: Option<(PathBuf, u8)> = None; // (path, rank) — lower rank wins
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let ext = match path.extension() {
            Some(e) => e.to_string_lossy().to_lowercase(),
            None => continue,
        };
        if !SIDECAR_EXTS.contains(&ext.as_str()) {
            continue;
        }
        let name = match path.file_stem() {
            Some(n) => n.to_string_lossy().to_lowercase(),
            None => continue,
        };
        if !name.starts_with(&stem) {
            continue;
        }
        let is_english = name == stem
            || name.contains(".en")
            || name.contains("eng")
            || name.contains("english");
        // Rank: English .srt best (0) … non-English non-srt worst (3).
        let rank = (if is_english { 0 } else { 2 }) + (if ext == "srt" { 0 } else { 1 });
        if best.as_ref().map(|(_, r)| rank < *r).unwrap_or(true) {
            best = Some((path, rank));
        }
    }
    best.map(|(p, _)| p)
}

/// The change-detection fingerprint of a video *and its sidecar subtitle*:
/// `(mtime, size)` where mtime is the newest of the two and size is their
/// sum. Fingerprinting only the video meant a subtitle added (or replaced, or
/// removed) later left the fingerprint unchanged, so a library scan skipped
/// the item and the new subtitle was never indexed. Any add/change/remove of
/// the sidecar now changes at least one of the two numbers; with no sidecar the
/// value is exactly the video's own, so nothing without one is re-indexed.
pub(crate) fn fingerprint(video: &Path) -> std::io::Result<(i64, i64)> {
    let secs = |m: &std::fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    };
    let v = std::fs::metadata(video)?;
    let (mut mtime, mut size) = (secs(&v), v.len() as i64);
    if let Some(side) = find_sidecar(video) {
        if let Ok(s) = std::fs::metadata(&side) {
            mtime = mtime.max(secs(&s));
            size += s.len() as i64;
        }
    }
    Ok((mtime, size))
}

/// Subtitle file bytes as text, and the encoding they were in. Subtitles that
/// came with older (AVI-era) releases are very often not UTF-8 -- Windows-1252
/// and other legacy code pages, sometimes UTF-16 -- so: a byte-order mark wins,
/// then valid UTF-8, then a guess from the bytes.
pub(crate) fn decode_subtitle_bytes(bytes: &[u8]) -> (String, &'static Encoding) {
    if let Some((enc, bom_len)) = Encoding::for_bom(bytes) {
        let (text, _) = enc.decode_without_bom_handling(&bytes[bom_len..]);
        return (text.into_owned(), enc);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return (text.to_string(), encoding_rs::UTF_8);
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let enc = detector.guess(None, chardetng::Utf8Detection::Deny);
    let (text, _) = enc.decode_without_bom_handling(bytes);
    (text.into_owned(), enc)
}

/// Load a sidecar subtitle file into SRT text. `.srt`/`.vtt` are decoded here
/// (whatever their encoding); other formats go through ffmpeg, told the
/// encoding when it isn't UTF-8.
pub(super) async fn load_sidecar(ffmpeg_bin: &str, path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let bytes = tokio::fs::read(path).await?;
    let (text, enc) = decode_subtitle_bytes(&bytes);
    if (ext == "srt" || ext == "vtt") && !text.trim().is_empty() {
        return Ok(text);
    }
    let charenc = (enc != encoding_rs::UTF_8).then(|| enc.name());
    ffmpeg::convert_to_srt_as(ffmpeg_bin, path, charenc).await
}
