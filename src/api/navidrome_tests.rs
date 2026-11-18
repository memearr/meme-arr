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
                    json!([{"id": "s1", "title": "A Song", "artist": "Band", "album": "LP", "duration": 200}])
                } else {
                    json!([])
                };
                Json(json!({"subsonic-response": {"status": "ok", "searchResult3": {"song": songs}}}))
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
    assert_eq!(resp.status(), StatusCode::CONFLICT);
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
