use super::*;

const SONG: &[u8] = b"fLaC-an-uploaded-song";

/// A state whose data folder is a fresh temp one, and whose library (if any) is `library`.
fn state(library: Option<&std::path::Path>) -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let mut st = crate::state::test_state();
    let mut cfg = (*st.config).clone();
    cfg.data_dir = dir.path().to_path_buf();
    cfg.music_library_dir = library.map(|p| p.to_path_buf());
    st.config = std::sync::Arc::new(cfg);
    (dir, st)
}

async fn body(resp: Response) -> (StatusCode, Vec<u8>) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .ok()
        .unwrap();
    (status, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or(serde_json::Value::Null)
}

async fn upload(st: &AppState, name: &str, bytes: &[u8]) -> Response {
    upload_song(
        State(st.clone()),
        Query(SongUpload { name: name.into() }),
        Bytes::copy_from_slice(bytes),
    )
    .await
    .ok()
    .unwrap()
}

async fn file_of(st: &AppState, id: &str, headers: HeaderMap) -> Response {
    music_file(State(st.clone()), Path(id.into()), headers)
        .await
        .ok()
        .unwrap()
}

#[tokio::test]
async fn an_uploaded_song_is_listed_served_with_ranges_and_removed() {
    let (dir, st) = state(None);
    let (status, b) = body(upload(&st, "Dance Break.flac", SONG).await).await;
    assert_eq!(status, StatusCode::CREATED);
    let track = json_of(&b)["track"].clone();
    let id = track["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("up-"), "{id}");
    assert_eq!(track["title"], "Dance Break");
    assert_eq!(track["source"], "upload");
    assert!(dir.path().join("music-uploads").is_dir());

    let (_, b) = body(
        list_song_uploads(State(st.clone()))
            .await
            .ok()
            .unwrap()
            .into_response(),
    )
    .await;
    assert_eq!(json_of(&b)["tracks"][0]["id"], id.as_str());

    // it is played from where it was kept (not fetched from anywhere)
    let resp = file_of(&st, &id, HeaderMap::new()).await;
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "audio/flac");
    assert_eq!(body(resp).await.1, SONG);
    let mut h = HeaderMap::new();
    h.insert(header::RANGE, "bytes=0-3".parse().unwrap());
    let (status, b) = body(file_of(&st, &id, h).await).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(b, b"fLaC");

    let gone = delete_song_upload(State(st.clone()), Path(id.clone()))
        .await
        .ok()
        .unwrap();
    assert_eq!(gone.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        file_of(&st, &id, HeaderMap::new()).await.status(),
        StatusCode::NOT_FOUND
    );
    let again = delete_song_upload(State(st), Path(id)).await.ok().unwrap();
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn something_that_is_not_audio_is_refused_with_the_reason() {
    let (_d, st) = state(None);
    let (status, b) = body(upload(&st, "page.mp3", b"<html>nope</html>").await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json_of(&b)["error"].as_str().unwrap().contains("audio"));
    assert_eq!(
        upload(&st, "empty.mp3", b"").await.status(),
        StatusCode::BAD_REQUEST
    );
    let (_, b) = body(
        list_song_uploads(State(st))
            .await
            .ok()
            .unwrap()
            .into_response(),
    )
    .await;
    assert_eq!(json_of(&b)["tracks"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn the_sources_say_what_there_is_to_search() {
    let lib = tempfile::tempdir().unwrap();
    let (_d, none) = state(None);
    let (_, b) = body(
        music_sources(State(none.clone()))
            .await
            .ok()
            .unwrap()
            .into_response(),
    )
    .await;
    let v = json_of(&b);
    assert_eq!(
        (
            v["navidrome"].clone(),
            v["library"].clone(),
            v["uploads"].clone()
        ),
        (false.into(), false.into(), 0.into())
    );
    assert!(v["max_upload_bytes"].as_u64().unwrap() > 0);

    let (_d2, st) = state(Some(lib.path()));
    upload(&st, "a.flac", SONG).await;
    let (_, b) = body(music_sources(State(st)).await.ok().unwrap().into_response()).await;
    let v = json_of(&b);
    assert_eq!(
        (v["library"].clone(), v["uploads"].clone()),
        (true.into(), 1.into())
    );
}

#[tokio::test]
async fn a_library_song_is_served_from_the_library_and_nothing_else_is() {
    let lib = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(lib.path().join("Band/LP")).unwrap();
    std::fs::write(lib.path().join("Band/LP/Theme.mp3"), b"ID3-theme").unwrap();
    std::fs::write(lib.path().join("secret.txt"), b"not music").unwrap();
    let (_d, st) = state(Some(lib.path()));
    let id = crate::local_music::library_id("Band/LP/Theme.mp3");
    let resp = file_of(&st, &id, HeaderMap::new()).await;
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(body(resp).await.1, b"ID3-theme");
    // not a file that is there but isn't a song, nor one made up, nor a way out
    for bad in [
        crate::local_music::library_id("secret.txt"),
        crate::local_music::library_id("nothing.mp3"),
        "lib-..".to_string(),
    ] {
        assert_eq!(
            file_of(&st, &bad, HeaderMap::new()).await.status(),
            StatusCode::NOT_FOUND,
            "{bad}"
        );
    }
    // with no library set up, a library id is just not a song
    let (_d2, bare) = state(None);
    assert_eq!(
        file_of(&bare, &id, HeaderMap::new()).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn an_uploaded_real_song_can_be_read_by_ffmpeg_for_its_waveform() {
    let tmp = tempfile::tempdir().unwrap();
    let wav = tmp.path().join("tone.wav");
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
        ])
        .arg(&wav)
        .status();
    if !made.map(|s| s.success()).unwrap_or(false) {
        eprintln!("skipping: ffmpeg not found on PATH");
        return;
    }
    let (_d, st) = state(None);
    let (status, b) = body(upload(&st, "Tone.wav", &std::fs::read(&wav).unwrap()).await).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = json_of(&b)["track"]["id"].as_str().unwrap().to_string();
    let resp = music_peaks(State(st), Path(id), Query(PeaksQuery { bins: Some(20) }))
        .await
        .ok()
        .unwrap();
    let (status, b) = body(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    let v = json_of(&b);
    assert_eq!(v["peaks"].as_array().unwrap().len(), 20);
    let ms = v["duration_ms"].as_i64().unwrap();
    assert!((1900..=2100).contains(&ms), "{ms}");
}
