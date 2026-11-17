use super::*;

#[test]
fn only_plain_ids_are_used_in_file_names() {
    assert!(valid_id("a1B2-c_3"));
    for bad in ["", "../x", "a/b", "a\\b", "a.b", "a b", &"x".repeat(65)] {
        assert!(!valid_id(bad), "{bad:?}");
    }
}

#[test]
fn audio_is_told_by_its_first_bytes() {
    assert_eq!(sniff(b"ID3\x04rest").unwrap().0, "mp3");
    assert_eq!(sniff(&[0xFF, 0xFB, 0x90, 0]).unwrap().0, "mp3");
    assert_eq!(sniff(b"fLaC\0\0").unwrap().0, "flac");
    assert_eq!(sniff(b"OggS\0").unwrap().0, "ogg");
    assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt ").unwrap().0, "wav");
    assert_eq!(sniff(b"\0\0\0\x20ftypM4A ").unwrap().0, "m4a");
    assert!(sniff(b"<html>").is_none());
    assert!(sniff(b"").is_none());
}

#[test]
fn a_download_is_kept_under_its_real_name_and_found_again() {
    let dir = tempfile::tempdir().unwrap();
    let m = Music::new(dir.path().to_path_buf());
    assert!(m.path_of("abc").is_none());
    let dl = m.download_path("abc").unwrap();
    std::fs::write(&dl, b"fLaC and then some audio").unwrap();
    let kept = m.finish("abc", &dl).unwrap();
    assert_eq!(kept.file_name().unwrap(), "abc.flac");
    assert_eq!(m.path_of("abc"), Some(kept.clone()));
    assert_eq!(content_type(&kept), "audio/flac");
    assert!(!dl.exists());
    // a half-written file is not "there"
    std::fs::write(dir.path().join("zzz.part"), b"x").unwrap();
    assert!(m.path_of("zzz").is_none());
}

#[test]
fn something_that_is_not_audio_is_refused_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let m = Music::new(dir.path().to_path_buf());
    let dl = m.download_path("abc").unwrap();
    std::fs::write(&dl, b"<html>login</html>").unwrap();
    assert!(m.finish("abc", &dl).is_err());
    assert!(!dl.exists());
    assert!(m.path_of("abc").is_none());
    assert!(m.download_path("../x").is_none());
}

#[test]
fn the_oldest_songs_go_first_when_the_cache_is_too_big() {
    let dir = tempfile::tempdir().unwrap();
    let m = Music::new(dir.path().to_path_buf());
    for (i, name) in ["old.mp3", "mid.mp3", "new.mp3"].iter().enumerate() {
        let p = dir.path().join(name);
        std::fs::write(&p, vec![0u8; 100]).unwrap();
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i as u64);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    m.prune(250);
    assert!(!dir.path().join("old.mp3").exists());
    assert!(dir.path().join("mid.mp3").exists());
    assert!(dir.path().join("new.mp3").exists());
    m.prune(0);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
