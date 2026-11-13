use super::*;

fn touch(dir: &Path, name: &str) {
    std::fs::write(dir.join(name), b"x").unwrap();
}

fn set_mtime(path: &Path, secs: u64) {
    let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    f.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn fingerprint_is_the_videos_own_when_there_is_no_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("Movie.mkv");
    std::fs::write(&video, b"12345").unwrap();
    set_mtime(&video, 1_000);
    assert_eq!(fingerprint(&video).unwrap(), (1_000, 5));
}

#[test]
fn a_subtitle_added_replaced_or_removed_later_changes_the_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("Movie.mkv");
    let sub = dir.path().join("Movie.en.srt");
    std::fs::write(&video, b"12345").unwrap();
    set_mtime(&video, 1_000);
    let bare = fingerprint(&video).unwrap();

    // Added (the downloaded-subtitle case this exists for).
    std::fs::write(&sub, b"1\n00:00:01,000 --> 00:00:02,000\nhi\n").unwrap();
    set_mtime(&sub, 2_000);
    let with = fingerprint(&video).unwrap();
    assert_ne!(with, bare);
    assert_eq!(with.0, 2_000);

    // Replaced with different content, even at the same size and an
    // older mtime than before: the sum of sizes or newest mtime moves, or
    // -- when both happen to match -- that's the same file as far as we
    // can tell. A longer replacement must register:
    std::fs::write(&sub, b"a much longer replacement subtitle file").unwrap();
    set_mtime(&sub, 1_500);
    assert_ne!(fingerprint(&video).unwrap(), with);

    // Removed: back to exactly the bare video.
    std::fs::remove_file(&sub).unwrap();
    assert_eq!(fingerprint(&video).unwrap(), bare);
}

#[test]
fn finds_matching_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "Movie (2021).mkv");
    touch(dir.path(), "Movie (2021).srt");
    touch(dir.path(), "unrelated.srt");
    let found = find_sidecar(&dir.path().join("Movie (2021).mkv")).unwrap();
    assert_eq!(found.file_name().unwrap(), "Movie (2021).srt");
}

#[test]
fn prefers_english_srt_over_other_lang_and_format() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "Show.mkv");
    touch(dir.path(), "Show.es.srt"); // Spanish
    touch(dir.path(), "Show.en.ass"); // English but ASS
    touch(dir.path(), "Show.en.srt"); // English SRT — should win
    let found = find_sidecar(&dir.path().join("Show.mkv")).unwrap();
    assert_eq!(found.file_name().unwrap(), "Show.en.srt");
}

#[test]
fn no_sidecar_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "Lonely.mkv");
    touch(dir.path(), "Lonely.nfo");
    touch(dir.path(), "poster.jpg");
    assert!(find_sidecar(&dir.path().join("Lonely.mkv")).is_none());
}

#[test]
fn ignores_image_subtitle_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "Film.mkv");
    touch(dir.path(), "Film.sup"); // PGS image subs — not indexable
    assert!(find_sidecar(&dir.path().join("Film.mkv")).is_none());
}

// ---- subtitle files in legacy encodings (the usual case beside old AVIs) ----

const SRT_LATIN1: &[u8] = b"1\r\n00:00:01,000 --> 00:00:03,000\r\nCaf\xe9 na\xefve, \x93quoted\x94\r\n\r\n2\r\n00:00:04,000 --> 00:00:05,000\r\nSecond line\r\n";

#[test]
fn decodes_a_windows_1252_subtitle_file() {
    let (text, enc) = decode_subtitle_bytes(SRT_LATIN1);
    assert_ne!(enc, encoding_rs::UTF_8);
    assert!(
        text.contains("Café naïve, \u{201c}quoted\u{201d}"),
        "{text}"
    );
    assert!(text.contains("Second line"));
}

#[test]
fn decodes_utf8_with_and_without_a_bom_and_utf16() {
    let plain = "1\n00:00:01,000 --> 00:00:02,000\nCafé\n";
    assert_eq!(decode_subtitle_bytes(plain.as_bytes()).0, plain);

    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(plain.as_bytes());
    assert_eq!(decode_subtitle_bytes(&bom).0, plain);

    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(plain.encode_utf16().flat_map(|u| u.to_le_bytes()));
    assert_eq!(decode_subtitle_bytes(&utf16).0, plain);
}

#[tokio::test]
async fn a_non_utf8_srt_beside_a_video_loads_with_every_line() {
    let dir = tempfile::tempdir().unwrap();
    let srt = dir.path().join("Movie.avi.srt");
    std::fs::write(&srt, SRT_LATIN1).unwrap();
    // No ffmpeg needed: .srt is decoded in-process (this path must not fail
    // with "Invalid data found when processing input").
    let text = load_sidecar("ffmpeg-not-installed", &srt).await.unwrap();
    let cues = subtitles::parse(&text);
    assert_eq!(cues.len(), 2, "{text}");
    assert!(cues[0].text.contains("Café"));
}

// ---- which sidecar wins: language, forced, hard-of-hearing, format ----------

fn rank(name: &str, preferred: &[&str]) -> (u8, usize, u8, u8) {
    let ext = name.rsplit('.').next().unwrap();
    let preferred: Vec<String> = preferred.iter().map(|s| s.to_string()).collect();
    sidecar_rank(name, "Movie", ext, &preferred)
}

#[test]
fn forced_subtitles_lose_to_a_full_one() {
    assert!(rank("Movie.en.srt", &[]) < rank("Movie.en.forced.srt", &[]));
    // even when the forced one is in the preferred language and the full one is not
    assert!(rank("Movie.fr.srt", &["en"]) < rank("Movie.en.forced.srt", &["en"]));
}

#[test]
fn the_preferred_language_wins_in_the_order_given() {
    let prefs = ["fr", "en"];
    assert!(rank("Movie.fr.srt", &prefs) < rank("Movie.en.srt", &prefs));
    assert!(rank("Movie.en.srt", &prefs) < rank("Movie.de.srt", &prefs));
    // with no preference: English first, as it always was; no tag means English
    assert!(rank("Movie.srt", &[]) <= rank("Movie.en.srt", &[]));
    assert!(rank("Movie.en.srt", &[]) < rank("Movie.fr.srt", &[]));
}

#[test]
fn a_plain_subtitle_beats_one_for_the_hard_of_hearing_then_srt_beats_ass() {
    assert!(rank("Movie.en.srt", &[]) < rank("Movie.en.hi.srt", &[]));
    assert!(rank("Movie.en.sdh.srt", &[]) > rank("Movie.en.srt", &[]));
    assert!(rank("Movie.en.srt", &[]) < rank("Movie.en.ass", &[]));
}

#[test]
fn find_sidecar_picks_the_full_english_file_over_forced_and_hi() {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "Movie.mkv",
        "Movie.en.forced.srt",
        "Movie.en.hi.srt",
        "Movie.en.srt",
        "Movie.fr.srt",
    ] {
        touch(dir.path(), name);
    }
    let found = find_sidecar(&dir.path().join("Movie.mkv")).unwrap();
    assert_eq!(found.file_name().unwrap(), "Movie.en.srt");
}
