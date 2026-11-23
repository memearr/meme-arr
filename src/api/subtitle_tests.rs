//! HTTP-level tests for the Bazarr and online-subtitle handlers, against one stand-in server
//! that speaks enough of Bazarr's and Plex's APIs.

use crate::api::test_support::Harness;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, patch, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const KEY: &str = "bazarr-test-key";

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    plex_version: Mutex<String>,
}

type Shared = Arc<Fake>;

impl Fake {
    fn log(&self, what: impl Into<String>) {
        self.calls.lock().unwrap().push(what.into());
    }

    fn saw(&self, prefix: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with(prefix))
    }
}

fn authed(h: &HeaderMap) -> bool {
    h.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(KEY)
}

async fn start() -> (String, Shared) {
    let fake: Shared = Arc::new(Fake::default());
    *fake.plex_version.lock().unwrap() = "1.40.0.7998".into();
    let app = Router::new()
        .route(
            "/api/system/status",
            get(|h: HeaderMap| async move {
                if !authed(&h) {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                Ok(Json(json!({"data": {"bazarr_version": "1.5.0"}})))
            }),
        )
        .route(
            "/api/system/languages/profiles",
            get(|| async { Json(json!({"data": [{"items": [{"language": "en"}]}]})) }),
        )
        .route(
            "/api/movies",
            get(|| async {
                Json(json!({"data": [{
                    "radarrId": 7, "title": "Paper Moon Heist", "path": "/data/m/101.mkv",
                    "profileId": 1,
                    "missing_subtitles": [{"code2": "en", "forced": false, "hi": false}]
                }]}))
            })
            .patch(
                |State(f): State<Shared>, Query(q): Query<HashMap<String, String>>| async move {
                    f.log(format!("PATCH movies {}", q["action"]));
                    StatusCode::NO_CONTENT
                },
            ),
        )
        .route("/api/series", get(|| async { Json(json!({"data": []})) }))
        .route(
            "/api/subtitles",
            patch(
                |State(f): State<Shared>, Query(q): Query<HashMap<String, String>>| async move {
                    f.log(format!("SUBTITLES {}", q["action"]));
                    StatusCode::NO_CONTENT
                },
            ),
        )
        .route(
            "/identity",
            get(|State(f): State<Shared>| async move {
                let v = f.plex_version.lock().unwrap().clone();
                Json(json!({"MediaContainer": {"version": v}}))
            }),
        )
        .route(
            "/library/metadata/{id}",
            get(|| async {
                Json(
                    json!({"MediaContainer": {"Metadata": [{"Media": [{"Part": [{"Stream": [
                        {"streamType": 3, "key": "/library/streams/9", "language": "English",
                         "format": "srt", "displayTitle": "English (SRT)"}
                    ]}]}]}]}}),
                )
            }),
        )
        .route(
            "/library/metadata/{id}/subtitles",
            get(|| async {
                Json(json!({"MediaContainer": {"Stream": [
                    {"key": "/library/streams/55", "title": "Release.en.srt", "language": "English",
                     "format": "srt", "providerTitle": "OpenSubtitles"}
                ]}}))
            })
            .merge(put(
                |State(f): State<Shared>, Query(q): Query<HashMap<String, String>>| async move {
                    let key = q.get("key").cloned().unwrap_or_default();
                    f.log(format!("PLEX DOWNLOAD {key}"));
                    if key.ends_with("/bad") {
                        return (StatusCode::NOT_FOUND, "no such subtitle");
                    }
                    (StatusCode::OK, "")
                },
            )),
        )
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), fake)
}

/// A harness with item "101" (`/m/101.mkv`, mapped into a real folder) and "102" (also mapped,
/// but unknown to Bazarr). `with_sidecar` writes a subtitle file beside the first one.
async fn with_items(with_sidecar: bool) -> Harness {
    let h = Harness::new().await;
    let media = h.dir.path().join("m");
    tokio::fs::create_dir_all(&media).await.unwrap();
    tokio::fs::write(media.join("101.mkv"), b"video")
        .await
        .unwrap();
    tokio::fs::write(media.join("102.mkv"), b"video")
        .await
        .unwrap();
    if with_sidecar {
        tokio::fs::write(
            media.join("101.en.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nhello there\n\n",
        )
        .await
        .unwrap();
    }
    let r = h
        .put(
            "/api/settings",
            json!({"mappings": [{"plex_prefix": "/m", "container_path": media.to_string_lossy()}]}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    h.state
        .db(|c| {
            crate::db::add_media(
                c,
                "101",
                "movie",
                "Paper Moon Heist",
                None,
                "direct",
                "Movies",
            );
            crate::db::add_media(c, "102", "movie", "Other Film", None, "direct", "Movies");
            Ok(())
        })
        .await
        .unwrap();
    h
}

async fn save_bazarr(h: &Harness, url: &str, key: &str) -> Value {
    let r = h
        .put("/api/settings/bazarr", json!({"url": url, "api_key": key}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    r.json()
}

async fn save_plex(h: &Harness, url: &str) {
    let r = h
        .put(
            "/api/settings",
            json!({"plex_url": url, "plex_token": "plex-test-token"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
}

/// Replace item 101's placeholder with a real one-second video, for the handlers that probe it.
/// `false` (and the test should return) when there's no ffmpeg here.
async fn real_video(h: &Harness) -> bool {
    let path = h.dir.path().join("m").join("101.mkv");
    let made = tokio::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=64x48:rate=5:duration=1",
        ])
        .arg(&path)
        .status()
        .await;
    let ok = made.map(|s| s.success()).unwrap_or(false);
    if !ok {
        eprintln!("skipping: ffmpeg not found on PATH");
    }
    ok
}

/// Wait for something a handler does in the background.
async fn eventually(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..50 {
        if done() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    false
}

// ---- Bazarr settings -------------------------------------------------------------

#[tokio::test]
async fn bazarr_settings_are_checked_saved_rotated_and_forgotten() {
    let (url, _fake) = start().await;
    let h = Harness::new().await;

    let before = h.get("/api/settings/bazarr").await.json();
    assert_eq!(before["configured"], false);
    let unset = h.post("/api/settings/bazarr/test", json!({})).await.json();
    assert_eq!(unset["ok"], false);
    assert!(unset["error"].as_str().unwrap().contains("isn't set up"));

    let bad = h
        .put("/api/settings/bazarr", json!({"url": "bazarr:6767"}))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert!(bad.error().contains("http://"));

    let saved = save_bazarr(&h, &format!("{url}/"), KEY).await;
    assert_eq!(saved["configured"], true);
    assert_eq!(saved["url"], url);
    assert_eq!(saved["key_set"], true);
    assert_eq!(saved["check"]["ok"], true);
    assert_eq!(saved["check"]["version"], "1.5.0");
    assert_eq!(saved["languages"], json!(["en"]));
    let token = saved["hook_token"].as_str().unwrap().to_string();
    assert!(!token.is_empty());
    assert!(
        !saved.to_string().contains(KEY),
        "the key must not be echoed"
    );

    // A blank key keeps the saved one, and the hook token survives a re-save.
    let again = save_bazarr(&h, &url, "  ").await;
    assert_eq!(again["check"]["ok"], true);
    assert_eq!(again["hook_token"], token);

    let tested = h.post("/api/settings/bazarr/test", json!({})).await.json();
    assert_eq!(tested["ok"], true);

    let rotated = h
        .post("/api/settings/bazarr/rotate-hook", json!({}))
        .await
        .json();
    assert_ne!(rotated["hook_token"], token);

    let gone = h.delete("/api/settings/bazarr").await.json();
    assert_eq!(gone["configured"], false);
    assert_eq!(gone["key_set"], false);
}

#[tokio::test]
async fn a_wrong_key_or_an_unreachable_bazarr_is_reported_in_the_check() {
    let (url, _fake) = start().await;
    let h = Harness::new().await;
    let wrong = save_bazarr(&h, &url, "not-the-key").await;
    assert_eq!(wrong["check"]["ok"], false);
    assert!(wrong["check"]["error"]
        .as_str()
        .unwrap()
        .contains("API key"));

    let down = save_bazarr(&h, "http://127.0.0.1:1", "k").await;
    assert_eq!(down["check"]["ok"], false);
}

#[tokio::test]
async fn the_bazarr_webhook_needs_the_secret_in_its_url() {
    let h = Harness::new().await;
    let token = save_bazarr(&h, "http://127.0.0.1:1", "k").await["hook_token"]
        .as_str()
        .unwrap()
        .to_string();
    let wrong = h.post("/api/hooks/bazarr/not-the-token", json!({})).await;
    assert_eq!(wrong.status, StatusCode::NOT_FOUND);
    let ok = h
        .post(&format!("/api/hooks/bazarr/{token}"), json!({}))
        .await;
    assert_eq!(ok.status, StatusCode::ACCEPTED);
    assert_eq!(ok.json()["ok"], true);
}

// ---- Bazarr and one item -----------------------------------------------------------

#[tokio::test]
async fn an_item_reports_what_bazarr_knows_about_it() {
    let (url, _fake) = start().await;
    let h = with_items(false).await;

    let off = h.get("/api/media/101/bazarr").await.json();
    assert_eq!(off["configured"], false);

    save_bazarr(&h, &url, KEY).await;
    let missing = h.get("/api/media/nope/bazarr").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let known = h.get("/api/media/101/bazarr").await.json();
    assert_eq!(known["found"], true);
    assert_eq!(known["target"]["title"], "Paper Moon Heist");
    assert_eq!(known["target"]["missing"].as_array().unwrap().len(), 1);
    assert_eq!(known["can_sync"], false);

    let stranger = h.get("/api/media/102/bazarr").await.json();
    assert_eq!(stranger["found"], false);
}

#[tokio::test]
async fn a_subtitle_file_beside_the_video_can_be_synced_and_shifted() {
    let (url, fake) = start().await;
    let h = with_items(true).await;
    if !real_video(&h).await {
        return;
    }
    save_bazarr(&h, &url, KEY).await;

    let known = h.get("/api/media/101/bazarr").await.json();
    assert_eq!(known["can_sync"], true);
    assert_eq!(known["sidecar"], "101.en.srt");

    let sync = h.post("/api/media/101/bazarr/sync", json!({})).await;
    assert_eq!(sync.status, StatusCode::OK, "{}", sync.text());
    assert_eq!(sync.json()["file"], "101.en.srt");
    assert!(fake.saw("SUBTITLES sync"));

    let shifted = h
        .post("/api/media/101/bazarr/shift", json!({"offset_ms": 1500}))
        .await;
    assert_eq!(shifted.status, StatusCode::OK, "{}", shifted.text());
    assert_eq!(shifted.json()["ok"], true);
    assert!(fake.saw("SUBTITLES shift_offset(h=0,m=0,s=1,ms=500)"));

    // Bazarr is asked, but this client refuses a shift it can't make.
    let none = h
        .post("/api/media/101/bazarr/shift", json!({"offset_ms": 0}))
        .await;
    assert_eq!(none.status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn sync_and_shift_refuse_what_cannot_work() {
    let (url, _fake) = start().await;
    let h = with_items(false).await;

    let off = h.post("/api/media/101/bazarr/sync", json!({})).await;
    assert_eq!(off.status, StatusCode::CONFLICT);
    let off = h
        .post("/api/media/101/bazarr/shift", json!({"offset_ms": 10}))
        .await;
    assert_eq!(off.status, StatusCode::CONFLICT);

    save_bazarr(&h, &url, KEY).await;
    for path in ["sync", "shift"] {
        let nothing = h
            .post(
                &format!("/api/media/101/bazarr/{path}"),
                json!({"offset_ms": 10}),
            )
            .await;
        assert_eq!(nothing.status, StatusCode::BAD_REQUEST, "{path}");
        assert!(nothing.error().contains("no subtitle file"), "{path}");
        let unknown = h
            .post(
                &format!("/api/media/nope/bazarr/{path}"),
                json!({"offset_ms": 10}),
            )
            .await;
        assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn asking_bazarr_to_search_runs_in_the_background() {
    let (url, fake) = start().await;
    let h = with_items(false).await;

    let off = h.post("/api/media/101/bazarr/search", json!({})).await;
    assert_eq!(off.status, StatusCode::CONFLICT);

    save_bazarr(&h, &url, KEY).await;
    let unknown = h.post("/api/media/nope/bazarr/search", json!({})).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    let stranger = h.post("/api/media/102/bazarr/search", json!({})).await;
    assert_eq!(stranger.status, StatusCode::NOT_FOUND);

    let ok = h.post("/api/media/101/bazarr/search", json!({})).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());
    assert_eq!(ok.json()["asked"], 1);
    assert!(eventually(|| fake.saw("PATCH movies search-missing")).await);
}

#[tokio::test]
async fn bazarr_being_down_is_a_bad_gateway() {
    let h = with_items(true).await;
    save_bazarr(&h, "http://127.0.0.1:1", "k").await;
    for (method, path) in [
        ("GET", "/api/media/101/bazarr"),
        ("POST", "/api/media/101/bazarr/search"),
        ("POST", "/api/media/101/bazarr/sync"),
        ("POST", "/api/media/101/bazarr/shift"),
    ] {
        let r = if method == "GET" {
            h.get(path).await
        } else {
            h.post(path, json!({"offset_ms": 10})).await
        };
        assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{path}: {}", r.text());
    }
}

// ---- online subtitle search ----------------------------------------------------------

#[tokio::test]
async fn capability_says_why_search_is_not_available() {
    let (url, fake) = start().await;
    let h = Harness::new().await;

    let none = h.get("/api/subtitles/capability").await.json();
    assert_eq!(none["supported"], false);
    assert!(none["reason"].as_str().unwrap().contains("not configured"));

    save_plex(&h, &url).await;
    let ok = h.get("/api/subtitles/capability").await.json();
    assert_eq!(ok["supported"], true);
    assert_eq!(ok["server"], "plex");
    assert_eq!(ok["version_checked"], true);
    assert!(ok["minimum_version"].is_string());

    *fake.plex_version.lock().unwrap() = "1.10.0.1".into();
    let old = h.get("/api/subtitles/capability").await.json();
    assert_eq!(old["supported"], false);
    assert!(old["reason"].as_str().unwrap().contains("too old"));

    *fake.plex_version.lock().unwrap() = "unreleased".into();
    let odd = h.get("/api/subtitles/capability").await.json();
    assert_eq!(odd["supported"], true);
    assert_eq!(odd["version_checked"], false);
}

#[tokio::test]
async fn a_jellyfin_that_cannot_be_reached_is_reported() {
    let h = Harness::new().await;
    let r = h
        .put(
            "/api/settings",
            json!({"media_server": "jellyfin", "jellyfin_url": "http://127.0.0.1:1", "jellyfin_api_key": "k"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let cap = h.get("/api/subtitles/capability").await.json();
    assert_eq!(cap["supported"], false);
    assert!(cap["reason"].as_str().unwrap().contains("Jellyfin"));

    let id = "0123456789abcdef0123456789abcdef";
    let search = h
        .get(&format!("/api/media/{id}/subtitles/search?language=eng"))
        .await;
    assert_eq!(search.status, StatusCode::BAD_GATEWAY);
    let bad_id = h.get("/api/media/12345/subtitles/search").await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn searching_returns_candidates_and_rejects_bad_input() {
    let (url, _fake) = start().await;
    let h = with_items(false).await;
    save_plex(&h, &url).await;

    let bad_id = h.get("/api/media/abc/subtitles/search").await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST);
    assert!(bad_id.error().contains("item id"));
    let bad_lang = h.get("/api/media/101/subtitles/search?language=x!").await;
    assert_eq!(bad_lang.status, StatusCode::BAD_REQUEST);
    let empty_lang = h.get("/api/media/101/subtitles/search?language=").await;
    assert_eq!(empty_lang.status, StatusCode::BAD_REQUEST);

    let ok = h.get("/api/media/101/subtitles/search?language=en").await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());
    let j = ok.json();
    assert_eq!(j["results"].as_array().unwrap().len(), 1);
    assert_eq!(j["results"][0]["key"], "/library/streams/55");
    let default = h.get("/api/media/101/subtitles/search").await;
    assert_eq!(default.status, StatusCode::OK);
}

#[tokio::test]
async fn downloading_a_subtitle_is_recorded_in_the_requests_list() {
    let (url, fake) = start().await;
    let h = with_items(false).await;
    save_plex(&h, &url).await;

    let bad_id = h
        .post("/api/media/abc/subtitles/download", json!({"key": "/x"}))
        .await;
    assert_eq!(bad_id.status, StatusCode::BAD_REQUEST);
    let bad_key = h
        .post(
            "/api/media/101/subtitles/download",
            json!({"key": "/library/../etc"}),
        )
        .await;
    assert_eq!(bad_key.status, StatusCode::BAD_REQUEST);
    assert!(bad_key.error().contains("subtitle key"));

    let ok = h
        .post(
            "/api/media/101/subtitles/download",
            json!({"key": "/library/streams/55", "title": "  Release.en.srt ", "provider": "OpenSubtitles", "language": "en"}),
        )
        .await;
    assert_eq!(ok.status, StatusCode::ACCEPTED, "{}", ok.text());
    assert!(fake.saw("PLEX DOWNLOAD /library/streams/55"));

    let list = h.get("/api/subtitles/requests").await.json();
    let reqs = list["requests"].as_array().unwrap();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0]["subtitle"], "Release.en.srt");
    let id = reqs[0]["id"].as_i64().unwrap();

    let refused = h
        .post(
            "/api/media/101/subtitles/download",
            json!({"key": "/library/streams/bad"}),
        )
        .await;
    assert!(!refused.status.is_success(), "{}", refused.text());
    assert_eq!(
        h.get("/api/subtitles/requests").await.json()["requests"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "a refused download is not recorded"
    );

    assert_eq!(
        h.delete(&format!("/api/subtitles/requests/{id}"))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        h.delete(&format!("/api/subtitles/requests/{id}"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn clearing_requests_needs_the_finished_flag() {
    let h = Harness::new().await;
    let bare = h.delete("/api/subtitles/requests").await;
    assert_eq!(bare.status, StatusCode::BAD_REQUEST);
    let ok = h.delete("/api/subtitles/requests?finished=true").await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.json()["removed"], 0);
}

#[tokio::test]
async fn the_diagnosis_combines_the_disk_with_what_the_server_lists() {
    let (url, _fake) = start().await;
    let h = with_items(true).await;

    // Even with no server set up, the answer says what could be seen locally.
    let blind = h.get("/api/media/101/subtitles/diagnose").await;
    assert_eq!(blind.status, StatusCode::OK, "{}", blind.text());
    assert!(blind.json()["server"]["error"].is_string());

    save_plex(&h, &url).await;
    let seen = h.get("/api/media/101/subtitles/diagnose").await.json();
    assert_eq!(seen["server"]["kind"], "plex");
    assert_eq!(seen["server"]["streams"].as_array().unwrap().len(), 1);
    assert!(seen["findings"].is_array());

    let bad = h.get("/api/media/abc/subtitles/diagnose").await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn subtitle_lines_can_be_read_by_time_window() {
    let h = Harness::new().await;
    h.seed(1).await;
    let ok = h.get("/api/media/TV-0/subtitles?start=0&end=5000").await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.json()["cues"].as_array().unwrap().len(), 1);
    let empty = h
        .get("/api/media/TV-0/subtitles?start=5000&end=9000")
        .await
        .json();
    assert!(empty["cues"].as_array().unwrap().is_empty());
    let missing = h.get("/api/media/nope/subtitles?start=0&end=1").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reindexing_one_item_reads_the_subtitle_beside_it() {
    let h = with_items(true).await;
    if !real_video(&h).await {
        return;
    }
    let ok = h.post("/api/media/101/reindex", json!({})).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());
    let found = h.get("/api/media/101/subtitles?start=0&end=5000").await;
    assert_eq!(found.json()["cues"].as_array().unwrap().len(), 1);

    let unknown = h.post("/api/media/nope/reindex", json!({})).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}
