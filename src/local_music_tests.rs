use super::*;

/// Bytes that are a FLAC file as far as the app can tell, made different by `n`.
fn flac(n: u32) -> Vec<u8> {
    let mut b = b"fLaC".to_vec();
    b.extend_from_slice(&n.to_le_bytes());
    b
}

fn uploads() -> (tempfile::TempDir, Uploads) {
    let dir = tempfile::tempdir().unwrap();
    let up = Uploads::new(dir.path().join("music-uploads"));
    (dir, up)
}

#[test]
fn ids_say_where_a_song_is_from() {
    assert_eq!(Source::of("up-abc"), Source::Upload);
    assert_eq!(Source::of("lib-abc"), Source::Library);
    assert_eq!(Source::of("jf-0123abcd"), Source::Jellyfin);
    assert_eq!(Source::of("a1b2c3"), Source::Navidrome);
    assert_eq!(
        Source::of("upload"),
        Source::Navidrome,
        "only the prefix `up-` counts"
    );
    assert_eq!(Source::Library.name(), "library");
    assert_eq!(Source::Jellyfin.name(), "jellyfin");
}

#[test]
fn a_file_name_becomes_a_tidy_title() {
    assert_eq!(display_name("My Song.mp3"), "My Song");
    assert_eq!(display_name("C:\\music\\a\\Track 01.flac"), "Track 01");
    assert_eq!(display_name("/x/y/z.final.mix.wav"), "z.final.mix");
    assert_eq!(display_name("no extension"), "no extension");
    assert_eq!(display_name(".hidden"), ".hidden");
    assert_eq!(display_name("tab\there\u{7}.mp3"), "tabhere");
    assert_eq!(
        display_name(&format!("{}.mp3", "x".repeat(300)))
            .chars()
            .count(),
        120
    );
}

#[test]
fn every_word_of_the_search_has_to_be_there_in_any_case() {
    assert!(matches("Artist/Album/Great Song.mp3", "great artist"));
    assert!(!matches("Artist/Album/Great Song.mp3", "great nothing"));
    assert!(!matches("anything", "   "), "no words matches nothing");
}

#[test]
fn an_uploaded_song_is_kept_listed_found_and_removed() {
    let (_d, up) = uploads();
    assert!(up.list().is_empty());
    let t = up.store("Dance Break.flac", &flac(1)).unwrap();
    assert!(
        t.id.starts_with("up-") && crate::music::valid_id(&t.id),
        "{}",
        t.id
    );
    assert_eq!(t.title, "Dance Break");
    let path = up.path_of(&t.id).unwrap();
    assert_eq!(path.extension().unwrap(), "flac");
    assert_eq!(std::fs::read(&path).unwrap(), flac(1));

    assert_eq!(up.list(), vec![t.clone()]);
    assert_eq!(up.search("dance"), vec![t.clone()]);
    assert!(up.search("polka").is_empty());

    up.remove(&t.id).unwrap();
    assert!(up.list().is_empty() && up.path_of(&t.id).is_none());
    assert_eq!(up.remove(&t.id), Err(UploadError::NotFound));
    // nothing of it is left in the folder
    assert_eq!(std::fs::read_dir(up.dir()).unwrap().count(), 0);
}

#[test]
fn the_same_song_twice_is_one_song_and_the_newest_is_listed_first() {
    let (_d, up) = uploads();
    let a = up.store("One.flac", &flac(1)).unwrap();
    let again = up.store("One (copy).flac", &flac(1)).unwrap();
    assert_eq!(a.id, again.id);
    assert_eq!(up.list().len(), 1);
    assert_eq!(up.list()[0].title, "One (copy)", "the latest name wins");
    std::thread::sleep(std::time::Duration::from_millis(30));
    let b = up.store("Two.flac", &flac(2)).unwrap();
    assert_eq!(
        up.list().iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        vec![b.id, a.id]
    );
}

#[test]
fn what_is_not_audio_or_is_empty_is_refused_and_leaves_nothing() {
    let (_d, up) = uploads();
    for bytes in [
        &b"<html>not a song</html>"[..],
        b"",
        b"\x89PNG\r\n\x1a\n....",
    ] {
        let err = up.store("x.mp3", bytes).unwrap_err();
        assert!(matches!(err, UploadError::Invalid(_)), "{err:?}");
    }
    assert!(up.list().is_empty());
    assert!(!up.dir().exists() || std::fs::read_dir(up.dir()).unwrap().count() == 0);
}

#[test]
fn there_is_a_limit_on_how_many_songs_are_kept_but_a_repeat_is_not_a_new_one() {
    let (_d, up) = uploads();
    for n in 0..MAX_UPLOADS as u32 {
        up.store("s.flac", &flac(n)).unwrap();
    }
    assert_eq!(up.list().len(), MAX_UPLOADS);
    assert_eq!(
        up.store("one more.flac", &flac(9999)),
        Err(UploadError::Limit)
    );
    assert!(
        up.store("again.flac", &flac(0)).is_ok(),
        "a song already there is not new"
    );
}

#[test]
fn only_real_upload_ids_find_a_file() {
    let (d, up) = uploads();
    let t = up.store("s.flac", &flac(1)).unwrap();
    // a Navidrome-style id, a traversal, and a library id find nothing here
    for id in ["s1", "../x", "up-../../x", "lib-abc", "", "up-"] {
        assert!(up.path_of(id).is_none(), "{id:?}");
    }
    // a stray file in the folder that is not audio is not a song
    std::fs::write(up.dir().join("up-stray.txt"), "x").unwrap();
    assert!(up.path_of("up-stray").is_none());
    assert_eq!(up.list(), vec![t]);
    drop(d);
}

fn library() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for rel in [
        "Band/First Album/01 Opening.mp3",
        "Band/First Album/02 Closing.flac",
        "Band/Second Album/Opening Again.ogg",
        "Loose Song.wav",
        "Band/First Album/cover.jpg",
        "Band/First Album/notes.txt",
        ".hidden/Secret Song.mp3",
        "Band/.Trash/Gone.mp3",
    ] {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }
    let lib = Library::new(root.to_path_buf());
    (dir, lib)
}

#[test]
fn the_library_is_searched_by_the_words_in_a_files_path() {
    let (_d, lib) = library();
    let all = lib.search("band", 50);
    let titles: Vec<_> = all.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(
        titles,
        ["01 Opening", "02 Closing", "Opening Again"],
        "audio only, hidden skipped"
    );
    let one = lib.search("opening first", 50);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].title, "01 Opening");
    assert_eq!(one[0].album, "First Album");
    assert_eq!(one[0].artist, "Band");
    assert_eq!(
        one[0].duration_s, 0,
        "the length isn't known until it is played"
    );
    // a file in the top folder has no album or artist
    let loose = lib.search("loose", 50);
    assert_eq!(
        (loose[0].album.as_str(), loose[0].artist.as_str()),
        ("", "")
    );
    assert!(lib.search("nothing like it", 50).is_empty());
    assert_eq!(lib.search("band", 2).len(), 2, "kept to the limit");
}

#[test]
fn a_library_song_has_a_stable_id_that_finds_its_file_again() {
    let (_d, lib) = library();
    let t = &lib.search("closing", 5)[0];
    assert!(
        t.id.starts_with("lib-") && crate::music::valid_id(&t.id),
        "{}",
        t.id
    );
    assert_eq!(t.id, library_id("Band/First Album/02 Closing.flac"));
    assert_eq!(t.id, lib.search("closing", 5)[0].id, "the same every time");
    let path = lib.path_of(&t.id).unwrap();
    assert!(path.ends_with("02 Closing.flac") && path.starts_with(lib.root()));
    // nothing else, nothing made up, nothing outside
    for id in [
        "lib-0000000000000000",
        "lib-../../etc/passwd",
        "s1",
        "up-abc",
        "",
    ] {
        assert!(lib.path_of(id).is_none(), "{id:?}");
    }
}

#[test]
fn a_library_that_is_not_there_is_just_empty() {
    let lib = Library::new(PathBuf::from("/definitely/not/a/folder"));
    assert!(lib.search("anything", 10).is_empty());
    assert!(lib.path_of("lib-0000000000000000").is_none());
}

#[test]
fn the_walk_stops_going_deeper_than_it_should() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    for i in 0..(MAX_DEPTH + 3) {
        deep = deep.join(format!("d{i}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("too deep.mp3"), b"x").unwrap();
    let shallow = dir.path().join("d0");
    std::fs::write(shallow.join("fine.mp3"), b"x").unwrap();
    let lib = Library::new(dir.path().to_path_buf());
    let found = lib.search("mp3", 10);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].title, "fine");
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_out_of_the_library() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("elsewhere.mp3"), b"x").unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link-dir")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("elsewhere.mp3"),
        dir.path().join("link-file.mp3"),
    )
    .unwrap();
    std::fs::write(dir.path().join("real.mp3"), b"x").unwrap();
    let lib = Library::new(dir.path().to_path_buf());
    let titles: Vec<_> = lib.search("mp3", 10).into_iter().map(|t| t.title).collect();
    assert_eq!(titles, ["real"]);
}
