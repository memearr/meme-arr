use super::*;
use std::collections::HashMap;

/// A state whose data folder is a fresh temp one.
fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let mut st = crate::state::test_state();
    let mut cfg = (*st.config).clone();
    cfg.data_dir = dir.path().to_path_buf();
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

const SONG: &[u8] = b"fLaC-pretend-this-is-a-song";

/// A password made for one test (never a literal).
fn secret() -> String {
    format!("pw-{}", uuid::Uuid::new_v4().simple())
}

fn failed(code: u32, msg: &str) -> Json<serde_json::Value> {
    Json(
        json!({"subsonic-response": {"status": "failed", "error": {"code": code, "message": msg}}}),
    )
}

fn allowed(q: &HashMap<String, String>, password: &str) -> bool {
    q.get("u").map(String::as_str) == Some("me")
        && q.get("p").is_none()
        && match (q.get("t"), q.get("s")) {
            (Some(t), Some(s)) => *t == crate::navidrome::token(password, s),
            _ => false,
        }
}

type Q = Query<HashMap<String, String>>;

/// A stand-in Navidrome that knows one account (`me` and `password`) and one song.
async fn fake_navidrome(password: &str) -> String {
    use axum::routing::get;
    let app = axum::Router::new()
        .route(
            "/rest/ping",
            get(|State(pw): State<String>, Query(q): Q| async move {
                if !allowed(&q, &pw) {
                    return failed(40, "Wrong username or password");
                }
                Json(json!({"subsonic-response": {"status": "ok", "serverVersion": "0.52.5"}}))
            }),
        )
        .route(
            "/rest/search3",
            get(|State(pw): State<String>, Query(q): Q| async move {
                if !allowed(&q, &pw) {
                    return failed(40, "Wrong username or password");
                }
                let hit = q.get("query").is_some_and(|s| s.contains("song"));
                let songs = if hit {
                    json!([{"id": "s1", "title": "A Song", "artist": "Band", "album": "LP", "duration": 200, "coverArt": "al-1"}])
                } else {
                    json!([])
                };
                Json(json!({"subsonic-response": {"status": "ok", "searchResult3": {"song": songs}}}))
            }),
        )
        .route(
            "/rest/getCoverArt",
            get(|State(pw): State<String>, Query(q): Q| async move {
                if !allowed(&q, &pw) {
                    return failed(40, "Wrong username or password").into_response();
                }
                match q.get("id").map(String::as_str) {
                    Some("al-1") => (
                        [(header::CONTENT_TYPE, "image/png")],
                        format!("PNG-{}", q.get("size").cloned().unwrap_or_default()),
                    )
                        .into_response(),
                    Some("odd") => ([(header::CONTENT_TYPE, "text/html")], "<html>login</html>")
                        .into_response(),
                    _ => failed(70, "Cover art not found").into_response(),
                }
            }),
        )
        .route(
            "/rest/stream",
            get(|State(pw): State<String>, Query(q): Q| async move {
                if !allowed(&q, &pw) {
                    return failed(40, "Wrong username or password").into_response();
                }
                match q.get("id").map(String::as_str) {
                    Some("s1") => ([(header::CONTENT_TYPE, "audio/flac")], SONG).into_response(),
                    Some("html") => ([(header::CONTENT_TYPE, "audio/flac")], "<html>login</html>")
                        .into_response(),
                    _ => failed(70, "Song not found").into_response(),
                }
            }),
        )
        .with_state(password.to_string());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn save(st: &AppState, url: &str, user: &str, password: &str) -> serde_json::Value {
    let resp = put_navidrome_settings(
        State(st.clone()),
        Json(NavidromePayload {
            url: Some(url.into()),
            user: Some(user.into()),
            password: Some(password.into()),
        }),
    )
    .await
    .ok()
    .unwrap();
    json_of(&body(resp).await.1)
}

#[tokio::test]
async fn saving_the_account_tests_it_and_never_sends_the_password_back() {
    let (_d, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    let j = save(&st, &format!("{url}/"), "me", &pw).await;
    assert_eq!(j["check"]["ok"], true, "{j}");
    assert_eq!(j["check"]["version"], "0.52.5");
    assert_eq!(j["configured"], true);
    assert_eq!(j["url"], url.as_str(), "the trailing slash is dropped");
    assert_eq!(j["user"], "me");
    assert_eq!(j["password_set"], true);
    assert!(!j.to_string().contains(&pw));
    // saving again without a password keeps the one that is there
    let resp = put_navidrome_settings(
        State(st.clone()),
        Json(NavidromePayload {
            url: None,
            user: Some("me".into()),
            password: Some(String::new()),
        }),
    )
    .await
    .ok()
    .unwrap();
    assert_eq!(json_of(&body(resp).await.1)["check"]["ok"], true);
}

#[tokio::test]
async fn a_wrong_password_says_so_and_a_bad_address_is_refused() {
    let (_d, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    let j = save(&st, &url, "me", &secret()).await;
    assert_eq!(j["check"]["ok"], false);
    assert!(j["check"]["error"]
        .as_str()
        .unwrap()
        .contains("Wrong username or password"));
    let resp = put_navidrome_settings(
        State(st.clone()),
        Json(NavidromePayload {
            url: Some("navi:4533".into()),
            user: None,
            password: None,
        }),
    )
    .await
    .ok()
    .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn without_an_account_music_is_not_available() {
    let (_d, st) = state();
    let resp = music_search(
        State(st.clone()),
        Query(MusicSearch {
            q: "song".into(),
            limit: None,
        }),
    )
    .await
    .ok()
    .unwrap();
    // other sources (uploads, the library) may still have songs, so this is an empty answer
    let (status, b) = body(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json_of(&b)["tracks"].as_array().unwrap().len(), 0);
    let resp = music_file(State(st), Path("s1".into()), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn songs_are_found_and_fetched_once_then_served_with_ranges() {
    let (dir, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;

    let hit = |q: &str| {
        music_search(
            State(st.clone()),
            Query(MusicSearch {
                q: q.into(),
                limit: Some(5),
            }),
        )
    };
    let (status, b) = body(hit("some song").await.ok().unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let tracks = json_of(&b)["tracks"].clone();
    assert_eq!(tracks[0]["id"], "s1");
    assert_eq!(tracks[0]["duration_s"], 200);
    let (_, b) = body(hit("nothing").await.ok().unwrap()).await;
    assert_eq!(json_of(&b)["tracks"].as_array().unwrap().len(), 0);
    let (_, b) = body(hit("  ").await.ok().unwrap()).await;
    assert_eq!(json_of(&b)["tracks"].as_array().unwrap().len(), 0);

    let resp = music_file(State(st.clone()), Path("s1".into()), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "audio/flac");
    let (status, b) = body(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b, SONG);
    assert!(dir.path().join("music/s1.flac").is_file());
    assert!(!dir.path().join("music/s1.dl").exists());

    let mut h = HeaderMap::new();
    h.insert(header::RANGE, "bytes=0-3".parse().unwrap());
    let resp = music_file(State(st), Path("s1".into()), h)
        .await
        .ok()
        .unwrap();
    let (status, b) = body(resp).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(b, b"fLaC");
}

#[tokio::test]
async fn a_missing_song_or_a_bad_id_or_a_non_audio_answer_is_an_error_and_leaves_nothing() {
    let (dir, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    for (id, want) in [
        ("gone", StatusCode::BAD_GATEWAY),
        ("html", StatusCode::BAD_GATEWAY),
        ("..", StatusCode::NOT_FOUND),
        ("a.b", StatusCode::NOT_FOUND),
    ] {
        let resp = music_file(State(st.clone()), Path(id.into()), HeaderMap::new())
            .await
            .ok()
            .unwrap();
        assert_eq!(resp.status(), want, "{id}");
    }
    let left: Vec<_> = std::fs::read_dir(dir.path().join("music"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

#[tokio::test]
async fn removing_the_account_forgets_it() {
    let (_d, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    let j = delete_navidrome_settings(State(st.clone()))
        .await
        .ok()
        .unwrap()
        .0;
    assert_eq!(j["configured"], false);
    assert_eq!(j["password_set"], false);
    assert!(st.settings().navidrome().is_none());
}

fn peaks_of(st: &AppState, id: &str) -> impl std::future::Future<Output = Response> {
    let (st, id) = (st.clone(), id.to_string());
    async move {
        music_peaks(State(st), Path(id), Query(PeaksQuery { bins: Some(50) }))
            .await
            .ok()
            .unwrap()
    }
}

#[tokio::test]
async fn peaks_need_navidrome_and_a_real_song_id() {
    let (_d, st) = state();
    assert_eq!(peaks_of(&st, "s1").await.status(), StatusCode::CONFLICT);
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    assert_eq!(
        peaks_of(&st, "../etc").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn peaks_of_a_file_that_is_not_audio_are_refused_not_a_crash() {
    let (_d, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    // the stand-in's "song" is not decodable audio
    let (status, b) = body(peaks_of(&st, "s1").await).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(json_of(&b)["error"]
        .as_str()
        .unwrap()
        .contains("couldn't read"));
}

async fn cover_of(st: &AppState, id: &str, size: Option<u32>) -> Response {
    music_cover(
        State(st.clone()),
        Path(id.into()),
        Query(CoverQuery { size }),
    )
    .await
    .ok()
    .unwrap()
}

#[tokio::test]
async fn search_results_carry_their_cover_and_it_is_served_scaled_and_cacheable() {
    let (_d, st) = state();
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    let (_, b) = body(
        music_search(
            State(st.clone()),
            Query(MusicSearch {
                q: "song".into(),
                limit: None,
            }),
        )
        .await
        .ok()
        .unwrap(),
    )
    .await;
    assert_eq!(json_of(&b)["tracks"][0]["cover_art"], "al-1");

    let resp = cover_of(&st, "al-1", None).await;
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "image/png");
    assert!(resp.headers()[header::CACHE_CONTROL]
        .to_str()
        .unwrap()
        .contains("max-age"));
    let (status, b) = body(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(b, b"PNG-96", "the default size is asked for");
    // a silly size is kept to what makes sense
    assert_eq!(
        body(cover_of(&st, "al-1", Some(5)).await).await.1,
        b"PNG-32"
    );
    assert_eq!(
        body(cover_of(&st, "al-1", Some(9000)).await).await.1,
        b"PNG-512"
    );
}

#[tokio::test]
async fn a_cover_that_is_not_a_picture_or_not_there_is_refused() {
    let (_d, st) = state();
    assert_eq!(
        cover_of(&st, "al-1", None).await.status(),
        StatusCode::CONFLICT
    );
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    assert_eq!(
        cover_of(&st, "../x", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        cover_of(&st, "odd", None).await.status(),
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(
        cover_of(&st, "nope", None).await.status(),
        StatusCode::BAD_GATEWAY
    );
}

/// A state with a library folder holding a song called "Library Song" and an upload called "Upload Song".
async fn state_with_local_songs() -> (tempfile::TempDir, tempfile::TempDir, AppState) {
    let lib = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(lib.path().join("Band/LP")).unwrap();
    std::fs::write(lib.path().join("Band/LP/Library Song.mp3"), b"ID3-x").unwrap();
    let (dir, mut st) = state();
    let mut cfg = (*st.config).clone();
    cfg.music_library_dir = Some(lib.path().to_path_buf());
    st.config = std::sync::Arc::new(cfg);
    st.config.uploads().store("Upload Song.flac", SONG).unwrap();
    (dir, lib, st)
}

async fn search(st: &AppState, q: &str) -> Response {
    music_search(
        State(st.clone()),
        Query(MusicSearch {
            q: q.into(),
            limit: Some(10),
        }),
    )
    .await
    .ok()
    .unwrap()
}

#[tokio::test]
async fn search_covers_uploads_the_library_and_navidrome_each_marked_with_its_source() {
    let (_d, _l, st) = state_with_local_songs().await;
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    // "song" is in all three: the stand-in Navidrome's one song is "A Song"
    let (status, b) = body(search(&st, "song").await).await;
    assert_eq!(status, StatusCode::OK);
    let v = json_of(&b);
    let got: Vec<(String, String)> = v["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            (
                t["source"].as_str().unwrap().into(),
                t["title"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("upload".to_string(), "Upload Song".to_string()),
            ("library".into(), "Library Song".into()),
            ("navidrome".into(), "A Song".into()),
        ]
    );
    assert!(v["warning"].is_null());
    // a library song says where it is, an upload has no album
    assert_eq!(v["tracks"][1]["album"], "LP");
    assert!(v["tracks"][1]["id"].as_str().unwrap().starts_with("lib-"));
    // only the uploads match this one
    let (_, b) = body(search(&st, "upload").await).await;
    assert_eq!(json_of(&b)["tracks"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn search_works_without_navidrome_from_what_is_on_this_machine() {
    let (_d, _l, st) = state_with_local_songs().await;
    let (status, b) = body(search(&st, "library").await).await;
    assert_eq!(status, StatusCode::OK);
    let v = json_of(&b);
    assert_eq!(v["tracks"].as_array().unwrap().len(), 1);
    assert_eq!(v["tracks"][0]["source"], "library");
}

#[tokio::test]
async fn when_navidrome_cannot_be_reached_the_other_songs_still_come_with_a_warning() {
    let (_d, _l, st) = state_with_local_songs().await;
    let pw = secret();
    let url = fake_navidrome(&pw).await;
    save(&st, &url, "me", &pw).await;
    // the account is changed behind its back: Navidrome now refuses it
    st.settings.write().unwrap().navidrome_url = Some("http://127.0.0.1:1".into());
    let (status, b) = body(search(&st, "song").await).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    let v = json_of(&b);
    assert_eq!(v["tracks"].as_array().unwrap().len(), 2);
    assert!(
        v["warning"].as_str().unwrap().starts_with("Navidrome: "),
        "{v}"
    );
    // with nothing local to show, the failure is the answer
    let (status, _) = body(search(&st, "nothing-local-matches-this").await).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

// ---- Jellyfin as a source of songs --------------------------------------------

const JF_SONG: &[u8] = b"ID3-a-jellyfin-song";

/// A stand-in Jellyfin that accepts one API key and has one song, `aa11`, with a picture.
async fn fake_jellyfin(key: &str) -> String {
    use axum::routing::get;
    fn allowed(h: &HeaderMap, key: &str) -> bool {
        h.get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains(&format!("Token=\"{key}\"")))
    }
    let app = axum::Router::new()
        .route(
            "/Items",
            get(|State(key): State<String>, h: HeaderMap, Query(q): Q| async move {
                if !allowed(&h, &key) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                assert_eq!(q.get("IncludeItemTypes").map(String::as_str), Some("Audio"));
                let hit = q.get("SearchTerm").is_some_and(|s| s.contains("song"));
                let items = if hit {
                    json!([{"Id": "aa11", "Name": "Jelly Song", "Artists": ["Band"], "Album": "LP",
                            "RunTimeTicks": 1_875_000_000i64, "ImageTags": {"Primary": "t"}}])
                } else {
                    json!([])
                };
                Json(json!({ "Items": items })).into_response()
            }),
        )
        .route(
            "/Items/{id}/Download",
            get(
                |State(key): State<String>, h: HeaderMap, Path(id): Path<String>| async move {
                    if !allowed(&h, &key) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    match id.as_str() {
                        "aa11" => ([(header::CONTENT_TYPE, "audio/mpeg")], JF_SONG).into_response(),
                        "page" => ([(header::CONTENT_TYPE, "text/html")], "<html>login</html>")
                            .into_response(),
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                },
            ),
        )
        .route(
            "/Items/{id}/Images/Primary",
            get(
                |State(key): State<String>,
                 h: HeaderMap,
                 Path(id): Path<String>,
                 Query(q): Q| async move {
                    if !allowed(&h, &key) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    match id.as_str() {
                        "aa11" => (
                            [(header::CONTENT_TYPE, "image/png")],
                            format!("JF-{}", q.get("maxWidth").cloned().unwrap_or_default()),
                        )
                            .into_response(),
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                },
            ),
        )
        .with_state(key.to_string());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// Set Jellyfin up as the (selected, or not) media server.
fn use_jellyfin(st: &AppState, url: &str, key: &str, selected: bool) {
    let mut s = st.settings.write().unwrap();
    s.server = if selected {
        crate::mediaserver::ServerKind::Jellyfin
    } else {
        crate::mediaserver::ServerKind::Plex
    };
    s.jellyfin_url = Some(url.into());
    s.jellyfin_api_key = Some(key.into());
}

#[tokio::test]
async fn jellyfin_songs_are_found_fetched_once_and_their_cover_served() {
    let (dir, st) = state();
    let key = secret();
    use_jellyfin(&st, &fake_jellyfin(&key).await, &key, true);

    let (_, b) = body(search(&st, "song").await).await;
    let v = json_of(&b);
    let t = &v["tracks"][0];
    assert_eq!(t["source"], "jellyfin");
    assert_eq!(
        (t["id"].as_str(), t["cover_art"].as_str()),
        (Some("jf-aa11"), Some("jf-aa11"))
    );
    assert_eq!(
        (
            t["title"].as_str(),
            t["artist"].as_str(),
            t["duration_s"].as_u64()
        ),
        (Some("Jelly Song"), Some("Band"), Some(187))
    );
    assert!(v["warning"].is_null());

    // the first play fetches the original file into the cache, as for Navidrome
    let resp = music_file(State(st.clone()), Path("jf-aa11".into()), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "audio/mpeg");
    assert_eq!(body(resp).await.1, JF_SONG);
    assert!(dir.path().join("music/jf-aa11.mp3").is_file());
    assert!(!dir.path().join("music/jf-aa11.dl").exists());

    // and so does the picture, scaled
    let resp = music_cover(
        State(st.clone()),
        Path("jf-aa11".into()),
        Query(CoverQuery { size: None }),
    )
    .await
    .ok()
    .unwrap();
    assert_eq!(resp.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(body(resp).await.1, b"JF-96");

    let (_, b) = body(
        crate::api::music_sources(State(st))
            .await
            .ok()
            .unwrap()
            .into_response(),
    )
    .await;
    assert_eq!(json_of(&b)["jellyfin"], true);
}

#[tokio::test]
async fn jellyfin_is_a_source_only_when_it_is_the_media_server_in_use() {
    let (_d, st) = state();
    let key = secret();
    use_jellyfin(&st, &fake_jellyfin(&key).await, &key, false); // Plex is the selected one
    let (status, b) = body(search(&st, "song").await).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json_of(&b)["tracks"].as_array().unwrap().len(),
        0,
        "not searched"
    );
    let (_, b) = body(
        crate::api::music_sources(State(st.clone()))
            .await
            .ok()
            .unwrap()
            .into_response(),
    )
    .await;
    assert_eq!(json_of(&b)["jellyfin"], false);
    let resp = music_file(State(st), Path("jf-aa11".into()), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    let (status, b) = body(resp).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(json_of(&b)["error"].as_str().unwrap().contains("Jellyfin"));
}

#[tokio::test]
async fn a_wrong_key_is_said_so_and_other_songs_still_show_with_a_warning() {
    let (_d, _l, st) = state_with_local_songs().await;
    let key = secret();
    use_jellyfin(&st, &fake_jellyfin(&key).await, "not-the-key", true);
    // another source has songs: they are shown, with what went wrong
    let (status, b) = body(search(&st, "song").await).await;
    assert_eq!(status, StatusCode::OK);
    let v = json_of(&b);
    assert_eq!(v["tracks"].as_array().unwrap().len(), 2);
    let warning = v["warning"].as_str().unwrap();
    assert!(
        warning.starts_with("Jellyfin: ") && warning.contains("API key"),
        "{warning}"
    );
    // nothing else matches: the failure is the answer, and it never contains the key
    let (status, b) = body(search(&st, "nothing-local-matches-this").await).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(!String::from_utf8_lossy(&b).contains("not-the-key"));
}

#[tokio::test]
async fn a_jellyfin_download_that_is_a_web_page_is_refused_and_leaves_nothing() {
    let (dir, st) = state();
    let key = secret();
    use_jellyfin(&st, &fake_jellyfin(&key).await, &key, true);
    for id in ["jf-page", "jf-missing"] {
        let resp = music_file(State(st.clone()), Path(id.into()), HeaderMap::new())
            .await
            .ok()
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY, "{id}");
    }
    let left: Vec<_> = std::fs::read_dir(dir.path().join("music"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
    // a path-like id is not a Jellyfin song at all
    let resp = music_file(State(st), Path("jf-..".into()), HeaderMap::new())
        .await
        .ok()
        .unwrap();
    assert_ne!(resp.status(), StatusCode::OK);
}
