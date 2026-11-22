use crate::api::test_support::{secret, Harness};
use axum::http::{header, Method, StatusCode};
use serde_json::json;

#[tokio::test]
async fn setup_is_required_until_a_password_exists_and_only_once() {
    let h = Harness::bare();
    let s = h.get("/api/auth/status").await.json();
    assert_eq!(s["setup_required"], true);
    assert_eq!(s["authenticated"], false);

    let short = h
        .post("/api/auth/setup", json!({"password": "short"}))
        .await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);
    assert!(short.error().contains("at least 8"));

    let pw = secret();
    let ok = h.post("/api/auth/setup", json!({"password": pw})).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert!(ok.headers.get(header::SET_COOKIE).is_some());

    let again = h
        .post("/api/auth/setup", json!({"password": secret()}))
        .await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    assert!(again.error().contains("Already set up"));
    assert_eq!(
        h.get("/api/auth/status").await.json()["setup_required"],
        false
    );
}

#[tokio::test]
async fn protected_routes_need_a_session_but_public_ones_do_not() {
    let h = Harness::bare();
    assert_eq!(h.get("/healthz").await.status, StatusCode::OK);
    assert_eq!(h.get("/api/version").await.status, StatusCode::OK);
    assert_eq!(
        h.get("/api/settings").await.status,
        StatusCode::UNAUTHORIZED
    );

    let signed_in = Harness::new().await;
    assert_eq!(
        signed_in.get("/api/auth/status").await.json()["authenticated"],
        true
    );
    assert_ne!(
        signed_in.get("/api/settings").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_checks_the_password_and_logout_ends_the_session() {
    let h = Harness::new().await;
    let wrong = h
        .post("/api/auth/login", json!({"password": secret()}))
        .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);

    let right = h
        .post("/api/auth/login", json!({"password": h.password}))
        .await;
    assert_eq!(right.status, StatusCode::OK);
    assert!(right.headers.get(header::SET_COOKIE).is_some());

    let out = h.post("/api/auth/logout", json!({})).await;
    assert_eq!(out.status, StatusCode::OK);
    // The cookie the harness still holds now names a deleted session.
    assert_eq!(
        h.get("/api/settings").await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn repeated_wrong_passwords_lock_the_address_out() {
    let h = Harness::new().await;
    let mut locked = None;
    for _ in 0..12 {
        let r = h
            .post("/api/auth/login", json!({"password": secret()}))
            .await;
        if r.status == StatusCode::TOO_MANY_REQUESTS {
            locked = Some(r);
            break;
        }
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let r = locked.expect("a lockout after repeated failures");
    assert!(r.headers.get(header::RETRY_AFTER).is_some());
    assert!(r.json()["retry_after_secs"].as_u64().unwrap() > 0);
    // Even the right password is refused while locked out.
    let right = h
        .post("/api/auth/login", json!({"password": h.password}))
        .await;
    assert_eq!(right.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn changing_the_password_needs_the_current_one() {
    let h = Harness::new().await;
    let new_pw = secret();
    let short = h
        .post(
            "/api/auth/change-password",
            json!({"current_password": h.password, "new_password": "short"}),
        )
        .await;
    assert_eq!(short.status, StatusCode::BAD_REQUEST);

    let wrong = h
        .post(
            "/api/auth/change-password",
            json!({"current_password": secret(), "new_password": new_pw}),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);
    assert!(wrong.error().contains("incorrect"));

    let ok = h
        .post(
            "/api/auth/change-password",
            json!({"current_password": h.password, "new_password": new_pw}),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(
        h.post("/api/auth/login", json!({"password": new_pw}))
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        h.post("/api/auth/login", json!({"password": h.password}))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn an_api_key_reads_only_the_endpoints_it_is_allowed_to() {
    let h = Harness::new().await;
    assert_eq!(
        h.get("/api/settings/api-key").await.json()["enabled"],
        false
    );

    let made = h.post("/api/settings/api-key", json!({})).await;
    assert_eq!(made.status, StatusCode::OK);
    let key = made.json()["api_key"].as_str().unwrap().to_string();
    assert_eq!(h.get("/api/settings/api-key").await.json()["enabled"], true);

    // A key (and no session) opens a read-only endpoint, but nothing else.
    let anon = Harness::bare();
    anon.state
        .db({
            let hash = crate::auth::hash_token(&key);
            move |c| crate::db::set_setting(c, "api_key_hash", &hash)
        })
        .await
        .unwrap();
    let ok = anon
        .call_with(Method::GET, "/api/index/status", &[("x-api-key", &key)])
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    let no = anon
        .call_with(Method::GET, "/api/settings", &[("x-api-key", &key)])
        .await;
    assert_eq!(no.status, StatusCode::UNAUTHORIZED);
    let bad = anon
        .call_with(Method::GET, "/api/index/status", &[("x-api-key", "nope")])
        .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);

    assert_eq!(
        h.delete("/api/settings/api-key").await.status,
        StatusCode::OK
    );
    assert_eq!(
        h.get("/api/settings/api-key").await.json()["enabled"],
        false
    );
}

#[tokio::test]
async fn trusted_proxies_are_validated_saved_and_cleared() {
    let h = Harness::new().await;
    assert_eq!(
        h.get("/api/settings/trusted-proxies").await.json()["trusted_proxies"],
        ""
    );

    let bad = h
        .put(
            "/api/settings/trusted-proxies",
            json!({"trusted_proxies": "not an address"}),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    let ok = h
        .put(
            "/api/settings/trusted-proxies",
            json!({"trusted_proxies": " 10.0.0.1 "}),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(
        h.get("/api/settings/trusted-proxies").await.json()["trusted_proxies"],
        "10.0.0.1"
    );

    let cleared = h
        .put(
            "/api/settings/trusted-proxies",
            json!({"trusted_proxies": ""}),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK);
    assert_eq!(
        h.get("/api/settings/trusted-proxies").await.json()["trusted_proxies"],
        ""
    );
}

#[tokio::test]
async fn unknown_api_paths_are_a_json_404() {
    let h = Harness::new().await;
    let r = h.get("/api/no/such/thing").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(r.error().contains("no such API endpoint"));
}
