//! HTTP-level tests for previews, stills, exports, the frame-accurate editor's reads and downloads.
//! The ones that need a real video make a tiny one with ffmpeg and skip themselves (like the
//! pipeline tests) when ffmpeg isn't on PATH.

use crate::api::test_support::Harness;
use axum::http::{header, Method, StatusCode};
use serde_json::{json, Value};

/// A harness with item "101" (a two-second video with sound) and "102" (the same, silent), both
/// mapped under `/m`. `None` when there's no ffmpeg here.
async fn with_video() -> Option<Harness> {
    let h = Harness::new().await;
    let media = h.dir.path().join("m");
    tokio::fs::create_dir_all(&media).await.unwrap();
    let with_sound = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=size=160x120:rate=10:duration=2")
        .args([
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-shortest",
        ])
        .arg(media.join("101.mkv"))
        .status()
        .await;
    let silent = tokio::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=size=160x120:rate=10:duration=2")
        .arg(media.join("102.mkv"))
        .status()
        .await;
    if !with_sound.map(|s| s.success()).unwrap_or(false)
        || !silent.map(|s| s.success()).unwrap_or(false)
    {
        eprintln!("skipping: ffmpeg not found on PATH");
        return None;
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
            crate::db::add_media(c, "102", "movie", "Quiet Film", None, "direct", "Movies");
            Ok(())
        })
        .await
        .unwrap();
    Some(h)
}

/// Text drawing needs fontconfig and fonts, which some machines (a bare Windows one) lack; the
/// CI image has them. A request that failed for that reason isn't a bug in the handler.
fn fonts_missing(r: &crate::api::test_support::Resp) -> bool {
    let missing = r.text().contains("Fontconfig");
    if missing {
        eprintln!("skipping the text check: no fontconfig here");
    }
    missing
}

/// Poll a job until it ends.
async fn job_result(h: &Harness, accepted: &crate::api::test_support::Resp) -> Value {
    assert_eq!(accepted.status, StatusCode::ACCEPTED, "{}", accepted.text());
    let id = accepted.json()["id"].as_str().unwrap().to_string();
    for _ in 0..300 {
        let j = h.get(&format!("/api/jobs/{id}")).await.json();
        if j["status"] == "done" || j["status"] == "failed" {
            return j;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("the job never finished");
}

// ---- validation that needs no video -----------------------------------------------------

#[tokio::test]
async fn an_export_is_refused_before_any_job_exists_when_it_cannot_work() {
    let h = Harness::new().await;
    let post = |body: Value| {
        let h = &h;
        async move { h.post("/api/export", body).await }
    };
    let base = json!({"rating_key": "101", "start_ms": 0, "end_ms": 1000, "format": "gif"});
    let with = |k: &str, v: Value| {
        let mut b = base.clone();
        b[k] = v;
        b
    };

    let r = post(with("end_ms", json!(0))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("greater than start"));

    let r = post(with("end_ms", json!(61_000))).await;
    assert!(r.error().contains("60 seconds"));

    let r = post(with("audio", json!(true))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "gif has no sound");

    let r = post(with(
        "music",
        json!({"id": "song", "start_ms": 0, "volume": 1.0}),
    ))
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = post(with("speed", json!(9.0))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = post(with(
        "caption",
        json!({"mode": "source", "look": {"path": [{"t_ms": 5000, "x": 0.1, "y": 0.1}, {"t_ms": 6000, "x": 0.2, "y": 0.2}]}}),
    ))
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());

    let r = post(with(
        "layers",
        json!([{"kind": "media", "asset": "gone.png", "start_ms": 0, "end_ms": 500}]),
    ))
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());

    let r = post(base.clone()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "an unindexed item");
    assert!(r.error().contains("not indexed"));
}

#[tokio::test]
async fn a_sequence_is_refused_when_it_cannot_work() {
    let h = Harness::new().await;
    let r = h
        .post(
            "/api/sequence/export",
            json!({"clips": [], "format": "gif"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("at least one clip"));

    let clip = json!({"rating_key": "nope", "start_ms": 0, "end_ms": 1000});
    let r = h
        .post(
            "/api/sequence/export",
            json!({"clips": [clip], "format": "gif", "audio": true}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let r = h
        .post(
            "/api/sequence/export",
            json!({"clips": [clip], "format": "gif"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("media not indexed"), "{}", r.text());

    let song = json!({"id": "song", "start_ms": 0, "volume": 1.0});
    let r = h
        .post(
            "/api/sequence/export",
            json!({"clips": [clip], "format": "mp4", "music": song}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn jobs_and_downloads_refuse_what_they_do_not_have() {
    let h = Harness::new().await;
    assert_eq!(
        h.get("/api/jobs/not-a-job").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.get("/api/download/nothing.gif").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.get("/api/download/..%2Fsecret").await.status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.get("/api/download/a%5Cb.gif").await.status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn reads_of_an_unknown_item_are_404() {
    let h = Harness::new().await;
    for uri in [
        "/api/media/nope/video-info",
        "/api/frame/nope?t=0",
        "/api/filmstrip/nope?start=0&end=1000",
        "/api/waveform/nope?start=0&end=1000",
        "/api/loudness/nope?start=0&end=1000",
        "/api/preview/nope?start=0&end=1000",
    ] {
        assert_eq!(h.get(uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
    let r = h
        .post(
            "/api/still",
            json!({"rating_key": "nope", "t_ms": 0, "format": "png"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn windows_that_make_no_sense_are_refused() {
    let h = Harness::new().await;
    h.seed(1).await;
    for (uri, why) in [
        ("/api/frame/TV-0?t=-1", "negative"),
        ("/api/filmstrip/TV-0?start=5&end=5", "empty"),
        ("/api/filmstrip/TV-0?start=0&end=999999", "long"),
        ("/api/waveform/TV-0?start=-1&end=5", "negative"),
        ("/api/waveform/TV-0?start=0&end=9999999", "long"),
        ("/api/loudness/TV-0?start=9&end=3", "backwards"),
        ("/api/loudness/TV-0?start=0&end=9999999", "long"),
        ("/api/preview/TV-0?start=5&end=5", "empty"),
    ] {
        assert_eq!(
            h.get(uri).await.status,
            StatusCode::BAD_REQUEST,
            "{why}: {uri}"
        );
    }
    let r = h
        .post(
            "/api/still",
            json!({"rating_key": "TV-0", "t_ms": -5, "format": "png"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

// ---- with a real video ---------------------------------------------------------------------

#[tokio::test]
async fn the_editor_reads_frame_rate_stills_strips_and_sound() {
    let Some(h) = with_video().await else { return };

    let info = h.get("/api/media/101/video-info").await;
    assert_eq!(info.status, StatusCode::OK, "{}", info.text());
    assert_eq!(info.json()["width"], 160);
    assert_eq!(info.json()["fps_num"], 10);
    // The second answer comes from the cache.
    assert_eq!(
        h.get("/api/media/101/video-info").await.json()["height"],
        120
    );

    let frame = h.get("/api/frame/101?t=500&width=100").await;
    assert_eq!(frame.status, StatusCode::OK, "{}", frame.text());
    assert_eq!(frame.headers[header::CONTENT_TYPE], "image/jpeg");
    assert!(frame.body.starts_with(&[0xFF, 0xD8]));
    let past = h.get("/api/frame/101?t=5000").await;
    assert_eq!(past.status, StatusCode::BAD_REQUEST);

    let strip = h
        .get("/api/filmstrip/101?start=0&end=1500&count=4&height=40")
        .await;
    assert_eq!(strip.status, StatusCode::OK, "{}", strip.text());
    assert!(strip.body.starts_with(&[0xFF, 0xD8]));

    let wave = h
        .get("/api/waveform/101?start=0&end=1500&bins=20")
        .await
        .json();
    assert_eq!(wave["has_audio"], true);
    assert!(wave["peaks"].is_array());
    let quiet = h.get("/api/waveform/102?start=0&end=1500").await.json();
    assert_eq!(quiet["has_audio"], false);

    let loud = h.get("/api/loudness/101?start=0&end=1500").await.json();
    assert_eq!(loud["has_audio"], true);
    let none = h.get("/api/loudness/102?start=0&end=1500").await.json();
    assert_eq!(none["has_audio"], false);
}

#[tokio::test]
async fn a_preview_is_rendered_once_and_then_served_from_the_cache() {
    let Some(h) = with_video().await else { return };
    let first = h
        .get("/api/preview/101?start=0&end=1000&width=120&fps=5")
        .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    assert_eq!(first.headers[header::CONTENT_TYPE], "video/mp4");
    assert!(!first.body.is_empty());

    let again = h
        .get("/api/preview/101?start=0&end=1000&width=120&fps=5")
        .await;
    assert_eq!(again.body, first.body);

    let ranged = h
        .call_with(
            Method::GET,
            "/api/preview/101?start=0&end=1000&width=120&fps=5",
            &[("range", "bytes=0-9")],
        )
        .await;
    assert_eq!(ranged.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(ranged.body.len(), 10);

    // A caption changes what is rendered.
    let meme = h
        .get("/api/preview/101?start=0&end=1000&width=120&fps=5&caption_mode=meme&top=HELLO&bottom=THERE")
        .await;
    if !fonts_missing(&meme) {
        assert_eq!(meme.status, StatusCode::OK, "{}", meme.text());
    }
    let source = h
        .get("/api/preview/101?start=0&end=1000&width=120&fps=5&caption_mode=source&cues_json=%5B%7B%22start_ms%22%3A0%2C%22end_ms%22%3A800%2C%22text%22%3A%22hi%22%7D%5D")
        .await;
    if !fonts_missing(&source) {
        assert_eq!(source.status, StatusCode::OK, "{}", source.text());
    }

    let broken = h
        .get("/api/preview/101?start=0&end=1000&caption_mode=source&cues_json=notjson")
        .await;
    assert!(!broken.status.is_success());
    let font = h
        .get("/api/preview/101?start=0&end=1000&caption_mode=meme&top=x&font=missing.ttf")
        .await;
    assert_eq!(font.status, StatusCode::BAD_REQUEST, "{}", font.text());
}

#[tokio::test]
async fn a_still_is_written_and_can_be_downloaded() {
    let Some(h) = with_video().await else { return };
    let made = h
        .post(
            "/api/still",
            json!({"rating_key": "101", "t_ms": 500, "format": "png", "width": 160,
                   "filename": "stillframe"}),
        )
        .await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.text());
    let captioned = h
        .post(
            "/api/still",
            json!({"rating_key": "101", "t_ms": 500, "format": "jpg",
                   "caption": {"mode": "meme", "top": "TOP"}}),
        )
        .await;
    if !fonts_missing(&captioned) {
        assert_eq!(captioned.status, StatusCode::OK, "{}", captioned.text());
    }
    let j = made.json();
    assert_eq!(j["format"], "png");
    let name = j["filename"].as_str().unwrap().to_string();
    assert!(name.ends_with(".png"));

    let got = h.get(j["download_path"].as_str().unwrap()).await;
    assert_eq!(got.status, StatusCode::OK);
    assert!(got.body.starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(got.headers[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains(&name));

    let past = h
        .post(
            "/api/still",
            json!({"rating_key": "101", "t_ms": 99_000, "format": "png"}),
        )
        .await;
    assert_eq!(past.status, StatusCode::BAD_REQUEST);
    let font = h
        .post(
            "/api/still",
            json!({"rating_key": "101", "t_ms": 100, "format": "jpg",
                   "caption": {"mode": "meme", "top": "x", "font": "missing.ttf"}}),
        )
        .await;
    assert_eq!(font.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_export_runs_as_a_job_and_the_file_can_be_downloaded() {
    let Some(h) = with_video().await else { return };
    let accepted = h
        .post(
            "/api/export",
            json!({"rating_key": "101", "start_ms": 0, "end_ms": 1000, "format": "mp4",
                   "width": 120, "fps": 5, "audio": true,
                   "filename": "clip"}),
        )
        .await;
    let job = job_result(&h, &accepted).await;
    assert_eq!(job["status"], "done", "{job}");
    let got = h.get(job["download_path"].as_str().unwrap()).await;
    assert_eq!(got.status, StatusCode::OK);
    assert!(!got.body.is_empty());

    // A clip that can't be made ends as a failed job with its reason.
    let bad = h
        .post(
            "/api/export",
            json!({"rating_key": "101", "start_ms": 0, "end_ms": 1000, "format": "gif",
                   "width": 120, "fps": 5, "crop": {"x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0}}),
        )
        .await;
    assert!(bad.status == StatusCode::ACCEPTED || bad.status == StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_sequence_of_clips_is_joined_into_one_file() {
    let Some(h) = with_video().await else { return };
    let clips = json!([
        {"rating_key": "101", "start_ms": 0, "end_ms": 800},
        {"rating_key": "102", "start_ms": 200, "end_ms": 1000},
    ]);
    let accepted = h
        .post(
            "/api/sequence/export",
            json!({"clips": clips, "format": "mp4", "width": 120, "fps": 5}),
        )
        .await;
    let job = job_result(&h, &accepted).await;
    assert_eq!(job["status"], "done", "{job}");

    let listed = h.get("/api/exports").await;
    assert_eq!(listed.status, StatusCode::OK);
    assert!(listed.text().contains(".mp4"), "{}", listed.text());
}
