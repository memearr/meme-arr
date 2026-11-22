//! HTTP-level tests for search, the library views and the media-server settings.

use crate::api::test_support::Harness;
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn dialogue_search_finds_lines_and_pages_through_them() {
    let h = Harness::new().await;
    h.seed(3).await;

    let all = h.get("/api/search?q=knock").await;
    assert_eq!(all.status, StatusCode::OK);
    let j = all.json();
    assert_eq!(j["results"].as_array().unwrap().len(), 9);
    assert_eq!(j["has_more"], false);
    assert_eq!(j["listing"], false);

    let page = h.get("/api/search?q=knock&limit=4").await.json();
    assert_eq!(page["results"].as_array().unwrap().len(), 4);
    assert_eq!(page["has_more"], true);
    let next = h.get("/api/search?q=knock&limit=4&offset=8").await.json();
    assert_eq!(next["results"].as_array().unwrap().len(), 1);
    assert_eq!(next["offset"], 8);
    // Show-name matches only come with the first page.
    assert_eq!(next["shows"].as_array().unwrap().len(), 0);

    let scoped = h.get("/api/search?q=knock&library=Movies").await.json();
    assert_eq!(scoped["results"].as_array().unwrap().len(), 3);

    let none = h.get("/api/search?q=zebra").await.json();
    assert!(none["results"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_search_with_only_operators_lists_the_matching_items() {
    let h = Harness::new().await;
    h.seed(2).await;
    let j = h.get("/api/search?q=show:Anime").await.json();
    assert_eq!(j["listing"], true);
    assert!(!j["results"].as_array().unwrap().is_empty());
    assert!(!j["filters"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn title_search_matches_item_titles() {
    let h = Harness::new().await;
    h.seed(2).await;
    let j = h.get("/api/search/title?q=item").await;
    assert_eq!(j.status, StatusCode::OK);
    assert!(!j.json()["results"].as_array().unwrap().is_empty());
    let off = h
        .get("/api/search/title?q=item&offset=1&limit=1")
        .await
        .json();
    assert_eq!(off["offset"], 1);
}

#[tokio::test]
async fn one_indexed_item_can_be_fetched_and_an_unknown_one_is_404() {
    let h = Harness::new().await;
    h.seed(1).await;
    let ok = h.get("/api/media/TV-0").await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.json()["rating_key"], "TV-0");
    let missing = h.get("/api/media/nope").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert!(missing.error().contains("not indexed"));
}

#[tokio::test]
async fn the_library_list_says_when_no_server_is_configured() {
    let h = Harness::new().await;
    let r = h.get("/api/libraries").await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert!(r.error().contains("not configured"));
}

#[tokio::test]
async fn library_views_report_what_is_indexed() {
    let h = Harness::new().await;
    h.seed(2).await;

    let stats = h.get("/api/libraries/stats").await.json();
    assert_eq!(stats["libraries"].as_array().unwrap().len(), 3);

    let browse = h.get("/api/library/browse").await.json();
    assert!(browse["total"].as_i64().unwrap() > 0);
    let filtered = h
        .get("/api/library/browse?q=Show&status=indexed&limit=1&offset=0&library=TV")
        .await
        .json();
    assert!(filtered["entries"].as_array().unwrap().len() <= 1);

    let eps = h.get("/api/library/episodes?show=TV%20Show").await.json();
    assert_eq!(eps["episodes"].as_array().unwrap().len(), 2);

    let missing = h.get("/api/libraries/missing?library=TV").await;
    assert_eq!(missing.status, StatusCode::OK);
    assert!(missing.json()["items"].is_array());

    let wanted = h.get("/api/wanted?q=&limit=10&offset=0").await.json();
    assert!(wanted["total"].is_number());
    assert!(wanted["unreachable"].is_number());
}

#[tokio::test]
async fn reindexing_without_a_server_is_accepted_or_reported_busy() {
    let h = Harness::new().await;
    let first = h.post("/api/index/reindex", json!({})).await;
    assert!(
        first.status == StatusCode::ACCEPTED || first.status == StatusCode::CONFLICT,
        "unexpected {}",
        first.status
    );
}

#[tokio::test]
async fn settings_start_empty_and_never_return_the_secrets() {
    let h = Harness::new().await;
    let s = h.get("/api/settings").await.json();
    assert_eq!(s["configured"], false);
    assert_eq!(s["token_set"], false);
    assert_eq!(s["jellyfin_key_set"], false);

    let tok = format!("tok-{}", uuid::Uuid::new_v4().simple());
    let saved = h
        .put(
            "/api/settings",
            json!({
                "plex_url": " http://plex.local:32400/ ",
                "plex_token": tok,
                "save_fetched_subtitles": true,
                "mappings": [{"plex_prefix": "/tv", "container_path": "/media/tv", "label": "TV"}],
            }),
        )
        .await;
    assert_eq!(saved.status, StatusCode::OK);
    let j = saved.json();
    assert_eq!(j["plex_url"], "http://plex.local:32400");
    assert_eq!(j["token_set"], true);
    assert_eq!(j["configured"], true);
    assert_eq!(j["save_fetched_subtitles"], true);
    assert_eq!(j["mappings"].as_array().unwrap().len(), 1);
    assert!(!saved.text().contains(&tok), "the token must not be echoed");

    // A blank token keeps the saved one.
    let kept = h
        .put("/api/settings", json!({"plex_token": "  "}))
        .await
        .json();
    assert_eq!(kept["token_set"], true);
}

#[tokio::test]
async fn switching_to_jellyfin_and_clearing_settings() {
    let h = Harness::new().await;
    let key = format!("key-{}", uuid::Uuid::new_v4().simple());
    let j = h
        .put(
            "/api/settings",
            json!({"media_server": "jellyfin", "jellyfin_url": "http://jf.local:8096/", "jellyfin_api_key": key}),
        )
        .await
        .json();
    assert_eq!(j["media_server"], "jellyfin");
    assert_eq!(j["jellyfin_url"], "http://jf.local:8096");
    assert_eq!(j["jellyfin_key_set"], true);

    assert_eq!(h.delete("/api/settings").await.status, StatusCode::OK);
    let cleared = h.get("/api/settings").await.json();
    assert_eq!(cleared["configured"], false);
    assert_eq!(cleared["mappings"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn testing_a_connection_needs_credentials_and_reports_failure() {
    let h = Harness::new().await;
    let r = h.post("/api/settings/test", json!({})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("Plex URL and token"));

    let r = h
        .post("/api/settings/test", json!({"media_server": "jellyfin"}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("Jellyfin URL and API key"));

    // Nothing listens on port 1: both servers report a failed test, not an HTTP error.
    let r = h
        .post(
            "/api/settings/test",
            json!({"plex_url": "http://127.0.0.1:1/", "plex_token": "t"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["ok"], false);
    let r = h
        .post(
            "/api/settings/test",
            json!({"media_server": "jellyfin", "jellyfin_url": "http://127.0.0.1:1", "jellyfin_api_key": "k"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["ok"], false);
    assert!(r.json()["error"].is_string());
}

#[tokio::test]
async fn the_folder_browser_stays_inside_the_allowed_roots() {
    let h = Harness::new().await;
    std::fs::create_dir_all(h.dir.path().join("shows/one")).unwrap();
    std::fs::create_dir_all(h.dir.path().join("shows/two")).unwrap();
    let base = h
        .dir
        .path()
        .join("shows")
        .to_string_lossy()
        .replace('\\', "/");

    let ok = h.get(&format!("/api/system/browse?path={base}")).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());
    let dirs = ok.json()["dirs"].as_array().unwrap().len();
    assert_eq!(dirs, 2);

    let rel = h.get("/api/system/browse?path=relative/dir").await;
    assert_eq!(rel.status, StatusCode::BAD_REQUEST);
    assert!(rel.error().contains("absolute"));

    // The folder above the allowed root is out of bounds.
    let above = h
        .dir
        .path()
        .parent()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let outside = h.get(&format!("/api/system/browse?path={above}")).await;
    assert_eq!(outside.status, StatusCode::BAD_REQUEST);
    assert!(outside.error().contains("isn't available"));

    let gone = h
        .get(&format!("/api/system/browse?path={base}/missing"))
        .await;
    assert_eq!(gone.status, StatusCode::BAD_REQUEST);
}
