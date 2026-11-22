//! HTTP-level tests for status, metrics, logs, storage, backups, health and the task endpoints.

use crate::api::test_support::Harness;
use axum::http::{header, Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn the_liveness_endpoints_answer_without_a_session() {
    let h = Harness::bare();
    let ok = h.get("/healthz").await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.json()["ok"], true);

    let st = h.get("/status").await.json();
    assert_eq!(st["status"], "ok");
    assert_eq!(st["index_running"], false);
    assert!(st["uptime_seconds"].is_number());
}

#[tokio::test]
async fn metrics_are_prometheus_text_with_the_expected_series() {
    let h = Harness::bare();
    h.seed(2).await;
    let r = h.get("/metrics").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    let body = r.text();
    for series in [
        "meme_arr_build_info{version=",
        "meme_arr_media_indexed_total 6",
        "meme_arr_subtitle_lines_total 6",
        "meme_arr_index_running 0",
        "meme_arr_db_bytes{component=\"file\"}",
        "meme_arr_export_jobs{status=\"queued\"} 0",
        "meme_arr_export_jobs{status=\"failed\"} 0",
    ] {
        assert!(body.contains(series), "missing {series} in:\n{body}");
    }
}

#[tokio::test]
async fn the_index_status_carries_counts() {
    let h = Harness::new().await;
    h.seed(1).await;
    let j = h.get("/api/index/status").await.json();
    assert_eq!(j["status"]["running"], false);
    assert_eq!(j["stats"]["media_count"], 3);
}

#[tokio::test]
async fn the_activity_log_lists_clips_and_clears() {
    let h = Harness::new().await;
    h.state
        .log_event(crate::state::LogLevel::Info, "first thing");
    h.state
        .log_event(crate::state::LogLevel::Warn, "second thing");

    let all = h.get("/api/activity/log").await.json();
    let texts: Vec<String> = all["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.to_string())
        .collect();
    assert!(texts.iter().any(|t| t.contains("first thing")));
    assert!(texts.iter().any(|t| t.contains("second thing")));

    let one = h.get("/api/activity/log?limit=1").await.json();
    assert_eq!(one["entries"].as_array().unwrap().len(), 1);

    assert_eq!(h.delete("/api/activity/log").await.status, StatusCode::OK);
    let after = h.get("/api/activity/log").await.json();
    assert!(after["entries"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn server_logs_and_the_log_level_can_be_read_and_changed() {
    let h = Harness::new().await;
    let logs = h.get("/api/logs?level=debug&q=&limit=10").await;
    assert_eq!(logs.status, StatusCode::OK);
    assert!(logs.json()["entries"].is_array());

    let lvl = h.get("/api/settings/log-level").await.json();
    assert!(lvl["options"].as_array().unwrap().len() >= 3);

    let bad = h
        .put("/api/settings/log-level", json!({"level": "shouting"}))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    // Tests run with a detached log handle, so a valid level can't be applied to a real filter.
    let valid = h
        .put("/api/settings/log-level", json!({"level": "warn"}))
        .await;
    assert_eq!(valid.status, StatusCode::BAD_REQUEST);
    assert!(valid.error().contains("couldn't apply log level"));
}

#[tokio::test]
async fn storage_and_exports_reflect_the_output_folder_and_it_can_be_emptied() {
    let h = Harness::new().await;
    let out = h.state.config.output_dir.clone();
    std::fs::write(out.join("a.gif"), vec![0u8; 100]).unwrap();
    std::fs::write(out.join("b.mp4"), vec![0u8; 50]).unwrap();

    let s = h.get("/api/system/storage").await.json();
    assert_eq!(s["exports"]["files"], 2);
    assert_eq!(s["exports"]["bytes"], 150);
    assert!(s["database"].is_object());

    let list = h.get("/api/exports").await.json();
    assert_eq!(list["files"].as_array().unwrap().len(), 2);

    let cleared = h.post("/api/system/clear-exports", json!({})).await.json();
    assert_eq!(cleared["files"], 2);
    assert_eq!(cleared["bytes"], 150);
    assert!(h.get("/api/exports").await.json()["files"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn the_maintenance_buttons_run_their_tasks() {
    let h = Harness::new().await;
    let tmp = h.state.config.tmp_dir();
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("preview.gif"), b"x").unwrap();

    let cache = h.post("/api/system/clear-preview-cache", json!({})).await;
    assert_eq!(cache.status, StatusCode::OK, "{}", cache.text());
    let vac = h.post("/api/system/vacuum", json!({})).await;
    assert_eq!(vac.status, StatusCode::OK, "{}", vac.text());
}

#[tokio::test]
async fn a_backup_can_be_made_listed_downloaded_and_restored() {
    let h = Harness::new().await;
    assert!(h.get("/api/backups").await.json()["backups"]
        .as_array()
        .unwrap()
        .is_empty());

    let made = h.post("/api/backups", json!({})).await;
    assert_eq!(made.status, StatusCode::OK, "{}", made.text());

    let listed = h.get("/api/backups").await.json();
    assert!(listed["keep"].as_u64().unwrap() >= 1);
    let name = listed["backups"][0]["name"].as_str().unwrap().to_string();

    let dl = h.get(&format!("/api/backups/{name}")).await;
    assert_eq!(dl.status, StatusCode::OK);
    assert!(dl.headers[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains(&name));

    // Restoring that same file works; a made-up name or a bad file does not.
    let restored = h
        .call_raw(
            Method::POST,
            "/api/backups/restore",
            "application/json",
            dl.body.clone(),
        )
        .await;
    assert_eq!(restored.status, StatusCode::OK, "{}", restored.text());
    let junk = h
        .call_raw(
            Method::POST,
            "/api/backups/restore",
            "application/json",
            b"not json".to_vec(),
        )
        .await;
    assert_eq!(junk.status, StatusCode::BAD_REQUEST);

    assert_eq!(
        h.get("/api/backups/evil.txt").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.get("/api/backups/settings-2020-01-01-000000.json")
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_health_report_can_be_read_and_refreshed() {
    let h = Harness::new().await;
    let r = h.get("/api/health").await.json();
    assert!(r["issues"].is_array());
    assert_eq!(r["running"], false);

    let fresh = h.post("/api/health/check", json!({})).await;
    assert_eq!(fresh.status, StatusCode::OK);
    assert!(fresh.json()["checked_at"].is_number());
}

#[tokio::test]
async fn tasks_are_listed_run_and_rescheduled() {
    let h = Harness::new().await;
    crate::tasks::init(&h.state.pool.get().unwrap(), false).unwrap();

    let list = h.get("/api/tasks").await;
    assert_eq!(list.status, StatusCode::OK);
    let tasks = list.json();
    assert!(
        tasks
            .as_array()
            .map(|a| a.len())
            .unwrap_or_else(|| tasks["tasks"].as_array().unwrap().len())
            >= 5
    );

    assert_eq!(
        h.post("/api/tasks/no_such_task/run", json!({}))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let started = h.post("/api/tasks/trim_activity_log/run", json!({})).await;
    assert!(
        started.status == StatusCode::ACCEPTED || started.status == StatusCode::CONFLICT,
        "{}",
        started.status
    );

    let missing = h
        .put(
            "/api/tasks/no_such_task",
            json!({"enabled": true, "interval_secs": 3600}),
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let too_short = h
        .put(
            "/api/tasks/trim_activity_log",
            json!({"enabled": true, "interval_secs": 1}),
        )
        .await;
    assert_eq!(too_short.status, StatusCode::BAD_REQUEST);
    let ok = h
        .put(
            "/api/tasks/trim_activity_log",
            json!({"enabled": false, "interval_secs": 24 * 3600}),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());
}

#[tokio::test]
async fn the_connection_report_describes_the_request_and_warns_about_proxies() {
    let h = Harness::new().await;
    let plain = h.get("/api/system/connection").await.json();
    assert_eq!(plain["peer"], "127.0.0.1");
    assert_eq!(plain["https"], false);
    assert!(plain["warnings"].as_array().unwrap().is_empty());

    let proxied = h
        .call_with(
            Method::GET,
            "/api/system/connection",
            &[
                ("x-forwarded-for", "198.51.100.7"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-host", "memes.example.com"),
            ],
        )
        .await
        .json();
    assert_eq!(proxied["headers"]["x-forwarded-for"], "198.51.100.7");
    assert_eq!(proxied["headers"]["x-forwarded-proto"], "https");
    // The peer (127.0.0.1) is private but not on the Trusted proxies list.
    assert!(proxied["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains("Trusted proxies")));
}
