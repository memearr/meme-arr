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
