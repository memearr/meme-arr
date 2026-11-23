//! Tests for the single-sign-on handlers (Authentik/OIDC and Plex), the export-job bookkeeping and
//! the shared origin helper. Plex's own login pages can't be stood in for (the addresses are fixed),
//! so the Plex tests cover what happens before and after that round trip.

use crate::api::test_support::Harness;
use crate::state::{Job, JobStatus};
use axum::{http::StatusCode, routing::get, Json, Router};
use serde_json::json;

const HOST: &[(&str, &str)] = &[("host", "app.test")];

/// An OIDC provider that only answers discovery.
async fn provider() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let issuer = base.clone();
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(move || {
                let issuer = issuer.clone();
                async move {
                    Json(json!({
                        "issuer": issuer,
                        "authorization_endpoint": format!("{issuer}/authorize"),
                        "token_endpoint": format!("{issuer}/token"),
                        "jwks_uri": format!("{issuer}/jwks"),
                        "response_types_supported": ["code"],
                        "subject_types_supported": ["public"],
                        "id_token_signing_alg_values_supported": ["RS256"],
                    }))
                }
            }),
        )
        .route("/jwks", get(|| async { Json(json!({"keys": []})) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn configure_authentik(h: &Harness, issuer: &str) {
    let r = h
        .put(
            "/api/settings/authentik",
            json!({"issuer_url": format!("{issuer}/"), "client_id": " meme-arr ", "client_secret": "oidc-test-secret"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
}

// ---- Authentik settings ----------------------------------------------------------

#[tokio::test]
async fn authentik_settings_are_saved_without_returning_the_secret() {
    let h = Harness::new().await;
    let none = h.get("/api/settings/authentik").await.json();
    assert_eq!(none["configured"], false);
    assert_eq!(none["secret_set"], false);

    configure_authentik(&h, "http://idp.local/").await;
    let saved = h.get("/api/settings/authentik").await;
    let j = saved.json();
    assert_eq!(j["configured"], true);
    assert_eq!(j["issuer_url"], "http://idp.local");
    assert_eq!(j["client_id"], "meme-arr");
    assert_eq!(j["secret_set"], true);
    assert!(!saved.text().contains("oidc-test-secret"));

    // Leaving the secret out keeps the saved one.
    let r = h
        .put(
            "/api/settings/authentik",
            json!({"issuer_url": "http://idp.local", "client_id": "other"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let j = h.get("/api/settings/authentik").await.json();
    assert_eq!(j["client_id"], "other");
    assert_eq!(j["secret_set"], true);

    assert_eq!(
        h.delete("/api/settings/authentik").await.status,
        StatusCode::OK
    );
    assert_eq!(
        h.get("/api/settings/authentik").await.json()["configured"],
        false
    );
}

// ---- Authentik login ---------------------------------------------------------------

#[tokio::test]
async fn logging_in_with_authentik_needs_it_set_up_and_reachable() {
    let h = Harness::bare();

    let no_host = h.get("/api/auth/oidc/login").await;
    assert_eq!(no_host.status, StatusCode::BAD_REQUEST);

    let unset = h
        .call_with(axum::http::Method::GET, "/api/auth/oidc/login", HOST)
        .await;
    assert_eq!(unset.status, StatusCode::BAD_REQUEST);
    assert!(unset.error().contains("isn't configured"));

    let mut h = Harness::bare();
    h.sign_in().await;
    configure_authentik(&h, "http://127.0.0.1:1").await;
    let down = h
        .call_with(axum::http::Method::GET, "/api/auth/oidc/login", HOST)
        .await;
    assert_eq!(down.status, StatusCode::BAD_REQUEST);
    assert!(down.error().contains("Could not reach Authentik"));
}

#[tokio::test]
async fn a_reachable_authentik_sends_the_browser_to_its_login_page() {
    let issuer = provider().await;
    let h = Harness::new().await;
    configure_authentik(&h, &issuer).await;
    let r = h
        .call_with(axum::http::Method::GET, "/api/auth/oidc/login", HOST)
        .await;
    assert!(r.status.is_redirection(), "{} {}", r.status, r.text());
    let to = r.headers.get("location").unwrap().to_str().unwrap();
    assert!(to.starts_with(&format!("{issuer}/authorize")), "{to}");
    assert!(to.contains("code_challenge="));
    assert!(to.contains("app.test"), "the callback names this app: {to}");
}

#[tokio::test]
async fn the_authentik_callback_rejects_bad_returns() {
    let h = Harness::new().await;
    let get = |q: &'static str| {
        let h = &h;
        async move {
            h.call_with(
                axum::http::Method::GET,
                &format!("/api/auth/oidc/callback{q}"),
                HOST,
            )
            .await
        }
    };

    let denied = get("?error=access_denied").await;
    assert_eq!(denied.status, StatusCode::BAD_REQUEST);
    assert!(denied.error().contains("access_denied"));

    let missing = get("").await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
    assert!(missing.error().contains("Missing code"));

    let unknown = get("?code=c&state=never-started").await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert!(unknown.error().contains("expired"));

    // A live attempt, but Authentik was removed meanwhile.
    h.state
        .db(|c| {
            let now = crate::api::now_secs();
            crate::db::create_oidc_flow(c, "live-state", "nonce", "verifier", now, now + 600)
        })
        .await
        .unwrap();
    let unset = get("?code=c&state=live-state").await;
    assert_eq!(unset.status, StatusCode::BAD_REQUEST);
    assert!(unset.error().contains("isn't configured"));

    // The same attempt can't be used twice.
    let again = get("?code=c&state=live-state").await;
    assert!(again.error().contains("expired"));
}

#[tokio::test]
async fn a_callback_the_provider_cannot_complete_is_a_failed_login() {
    let issuer = provider().await;
    let h = Harness::new().await;
    configure_authentik(&h, &issuer).await;
    h.state
        .db(|c| {
            let now = crate::api::now_secs();
            crate::db::create_oidc_flow(c, "s1", "nonce", "verifier", now, now + 600)
        })
        .await
        .unwrap();
    let r = h
        .call_with(
            axum::http::Method::GET,
            "/api/auth/oidc/callback?code=abc&state=s1",
            HOST,
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("Login failed"), "{}", r.text());
}

// ---- Plex login ------------------------------------------------------------------------

#[tokio::test]
async fn the_plex_login_link_is_empty_until_an_account_is_linked() {
    let h = Harness::new().await;
    let none = h.get("/api/settings/plex-login").await.json();
    assert_eq!(none["linked"], false);

    h.state
        .db(|c| crate::db::set_plex_login_account(c, "uuid-1", "alice"))
        .await
        .unwrap();
    let linked = h.get("/api/settings/plex-login").await.json();
    assert_eq!(linked["linked"], true);
    assert_eq!(linked["username"], "alice");

    assert_eq!(
        h.delete("/api/settings/plex-login").await.json()["ok"],
        true
    );
    assert_eq!(
        h.get("/api/settings/plex-login").await.json()["linked"],
        false
    );
}

#[tokio::test]
async fn the_plex_callback_rejects_attempts_that_are_unknown_or_used() {
    let h = Harness::bare();
    let r = h.get("/api/auth/plex/callback?pin_id=999").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.error().contains("expired"));
    let bad = h.get("/api/auth/plex/callback?pin_id=notanumber").await;
    assert!(bad.status.is_client_error());
}

#[tokio::test]
async fn starting_a_plex_login_needs_a_host_or_a_session() {
    let h = Harness::bare();
    let no_host = h.get("/api/auth/plex/login").await;
    assert_eq!(no_host.status, StatusCode::BAD_REQUEST);
    assert!(no_host.error().contains("Host"));

    let link = h.get("/api/settings/plex-login/link").await;
    assert_eq!(link.status, StatusCode::UNAUTHORIZED);

    let signed_in = Harness::new().await;
    let no_host = signed_in.get("/api/settings/plex-login/link").await;
    assert_eq!(no_host.status, StatusCode::BAD_REQUEST);
}

// ---- export jobs -----------------------------------------------------------------------

async fn finished(st: &crate::state::AppState, id: &str) -> Job {
    for _ in 0..100 {
        if let Some(j) = st.get_job(id) {
            if matches!(j.status, JobStatus::Done | JobStatus::Failed) {
                return j;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the job never finished");
}

#[tokio::test]
async fn a_queued_job_runs_to_done_with_its_download_address() {
    let h = Harness::bare();
    let (id, files) =
        super::jobs::queue(&h.state, Some("My Clip"), crate::export::Format::Gif, true);
    let queued = h.state.get_job(&id).unwrap();
    assert!(matches!(queued.status, JobStatus::Queued));
    assert!(queued.audio);
    assert!(files.filename.ends_with(".gif"));
    assert!(files
        .download
        .ends_with(&format!("/api/download/{}", files.filename)));

    super::jobs::run_in_background(&h.state, id.clone(), "Export", files, async { Ok(()) });
    let done = finished(&h.state, &id).await;
    assert!(matches!(done.status, JobStatus::Done));
    assert!(done.download_path.unwrap().contains("/api/download/"));
    assert!(done.error.is_none());
}

#[tokio::test]
async fn a_failing_job_keeps_its_error_and_is_logged() {
    let h = Harness::bare();
    let (id, files) = super::jobs::queue(&h.state, None, crate::export::Format::Mp4, false);
    super::jobs::run_in_background(&h.state, id.clone(), "Export", files, async {
        anyhow::bail!("ffmpeg exploded")
    });
    let failed = finished(&h.state, &id).await;
    assert!(matches!(failed.status, JobStatus::Failed));
    assert!(failed.error.unwrap().contains("ffmpeg exploded"));
    assert!(failed.download_path.is_none());
}

#[tokio::test]
async fn job_files_and_tools_follow_the_config() {
    let h = Harness::bare();
    let files = super::jobs::JobFiles::with_ext(
        &h.state,
        Some("../../evil name"),
        "webm",
        "0123456789abcdef",
    );
    assert!(files.out_path.starts_with(&h.state.config.output_dir));
    assert!(!files.filename.contains(".."));
    assert!(files.filename.ends_with(".webm"));
    let t = super::jobs::tools(&h.state);
    assert_eq!(t.ffmpeg, h.state.config.ffmpeg_bin.as_str());

    let id = "abcdef0123456789";
    let accepted = super::jobs::accepted(&h.state, id);
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
}
