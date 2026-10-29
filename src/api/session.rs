//! Sessions, API keys, login/logout/password, trusted proxies, and the auth middleware.

use super::*;

/// Cookie carrying the session token. HttpOnly (no JS access) and
/// SameSite=Lax; deliberately NOT `Secure` -- this app is routinely served
/// over plain HTTP on a LAN, and `Secure` would silently break the cookie
/// there. It still works fine if someone puts this behind HTTPS later.
pub(super) const SESSION_COOKIE: &str = "memearr_session";

/// How long a session lasts from login. Fixed, not sliding (no refresh on
/// use) -- simple, and 30 days is generous for a personal LAN app.
pub(super) const SESSION_TTL_SECS: i64 = 30 * 24 * 60 * 60;

/// Paths reachable without a session — the auth endpoints themselves
/// (chicken-and-egg otherwise), health/version checks, and nothing else.
/// Static SPA assets are served outside this router entirely (see
/// `lib.rs`'s `fallback_service`), so the login page itself always loads;
/// this only gates the JSON API.
pub(super) fn is_public_api_path(path: &str) -> bool {
    path == "/healthz"
        || path == "/status"
        || path == "/metrics"
        || path == "/api/version"
        || path.starts_with("/api/auth/")
}

/// Shared by the `require_auth` middleware and anywhere else (the Plex
/// "link" callback — see below) that needs an inline, non-middleware
/// session check.
pub(super) async fn has_valid_session(st: &AppState, jar: &CookieJar) -> bool {
    let Some(token) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) else {
        return false;
    };
    let pool = st.pool.clone();
    let valid = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        let conn = pool.get()?;
        db::session_valid(&conn, &auth::hash_token(&token), now_secs())
    })
    .await;
    matches!(valid, Ok(Ok(true)))
}

pub(super) async fn require_auth(
    State(st): State<AppState>,
    jar: CookieJar,
    req: Request,
    next: Next,
) -> Response {
    if is_public_api_path(req.uri().path()) {
        return next.run(req).await;
    }
    // Owned copies: a `&Request` held across an await isn't Send.
    let presented_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .filter(|_| auth::api_key_may_access(req.method(), req.uri().path()));
    if has_valid_session(&st, &jar).await || has_valid_api_key(&st, presented_key).await {
        next.run(req).await
    } else {
        unauthorized()
    }
}

/// A presented `X-Api-Key` (already filtered to endpoints a key may reach) matching the stored hash. Read-only
/// endpoint a key is allowed to reach (see `auth::api_key_may_access`).
/// Header only -- query strings end up in access logs and browser history.
pub(super) async fn has_valid_api_key(st: &AppState, presented: Option<String>) -> bool {
    let Some(presented) = presented else {
        return false;
    };
    let presented = auth::hash_token(&presented);
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || {
        pool.get()
            .ok()
            .and_then(|c| db::get_setting(&c, "api_key_hash").ok().flatten())
            .is_some_and(|stored| stored == presented)
    })
    .await
    .unwrap_or(false)
}

pub(super) async fn api_key_status(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let enabled = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        let conn = pool.get()?;
        Ok(db::get_setting(&conn, "api_key_hash")?.is_some())
    })
    .await??;
    Ok(Json(json!({ "enabled": enabled })))
}

/// Generate (or replace) the API key. The plaintext is returned exactly
/// once; only its hash is stored, like a GitHub token.
pub(super) async fn api_key_generate(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let key = auth::new_api_key();
    let hash = auth::hash_token(&key);
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::set_setting(&conn, "api_key_hash", &hash)
    })
    .await??;
    st.log_event(LogLevel::Info, "API key generated");
    Ok(Json(json!({ "api_key": key })))
}

pub(super) async fn api_key_revoke(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::delete_setting(&conn, "api_key_hash")
    })
    .await??;
    st.log_event(LogLevel::Info, "API key revoked");
    Ok(Json(json!({ "ok": true })))
}

pub(super) fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "authentication required" })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
pub(super) struct PasswordBody {
    password: String,
}

/// Whether first-run setup is still needed, and whether the current
/// request's cookie (if any) is a valid session — lets the frontend decide
/// setup screen vs. login screen vs. the app itself without guessing.
pub(super) async fn auth_status(
    State(st): State<AppState>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    let token = jar.get(SESSION_COOKIE).map(|c| c.value().to_string());
    let pool = st.pool.clone();
    let (setup_required, authenticated, authentik_available, plex_login_available) =
        tokio::task::spawn_blocking(move || -> anyhow::Result<(bool, bool, bool, bool)> {
            let conn = pool.get()?;
            let setup_required = db::get_admin_password_hash(&conn)?.is_none();
            let authenticated = match &token {
                Some(t) if !setup_required => {
                    db::session_valid(&conn, &auth::hash_token(t), now_secs())?
                }
                _ => false,
            };
            let authentik_available = db::get_authentik_settings(&conn)?.is_some();
            let plex_login_available = db::get_plex_login_account(&conn)?.is_some();
            Ok((
                setup_required,
                authenticated,
                authentik_available,
                plex_login_available,
            ))
        })
        .await??;
    Ok(Json(json!({
        "setup_required": setup_required,
        "authenticated": authenticated,
        "authentik_available": authentik_available,
        "plex_login_available": plex_login_available,
    })))
}

/// First-run only: set the admin password. Refuses once one already
/// exists — use `RESET_ADMIN_PASSWORD` (env var, requires a restart) to
/// replace it after that, not this endpoint; see `auth`'s module docs.
pub(super) async fn auth_setup(
    State(st): State<AppState>,
    Json(body): Json<PasswordBody>,
) -> Result<impl IntoResponse, AppError> {
    if body.password.len() < 8 {
        return Ok(bad_request("Password must be at least 8 characters").into_response());
    }
    let pool = st.pool.clone();
    let password = body.password.clone();
    let already_set = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        let conn = pool.get()?;
        if db::get_admin_password_hash(&conn)?.is_some() {
            return Ok(true);
        }
        let hash = auth::hash_password(&password)?;
        db::set_admin_password_hash(&conn, &hash)?;
        Ok(false)
    })
    .await??;
    if already_set {
        return Ok(bad_request("Already set up — log in instead").into_response());
    }
    st.log_event(LogLevel::Info, "Admin password set up");
    login_response(&st, &body.password).await
}

pub(super) async fn auth_login(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(body): Json<PasswordBody>,
) -> Result<Response, AppError> {
    let ip = client_ip(&st, peer.ip(), &headers).await;
    if let Some(retry_after) = st.login_limiter.check(ip) {
        warn!("login from {ip} rejected: locked out for {retry_after:?}");
        return Ok(too_many_attempts(retry_after));
    }
    let resp = login_response(&st, &body.password).await?;
    if resp.status() == StatusCode::UNAUTHORIZED {
        // Every failure is logged with its source IP (Jellyfin does too),
        // so a brute-force attempt is visible in the Activity log. Bounded:
        // once locked out, further attempts return above without logging.
        let locked = st.login_limiter.record_failure(ip);
        warn!("failed login from {ip}");
        st.log_event(LogLevel::Warn, format!("Failed login from {ip}"));
        if let Some(lock) = locked {
            st.log_event(
                LogLevel::Warn,
                format!(
                    "Locked out {ip} for {}s after repeated failed logins",
                    lock.as_secs()
                ),
            );
        }
    } else {
        st.login_limiter.record_success(ip);
    }
    Ok(resp)
}

/// The caller's real IP: the TCP peer, or -- only when the peer is a
/// configured trusted proxy -- the client it forwarded for (see `proxies`).
pub(super) async fn client_ip(
    st: &AppState,
    peer: std::net::IpAddr,
    headers: &axum::http::HeaderMap,
) -> std::net::IpAddr {
    let pool = st.pool.clone();
    let raw = tokio::task::spawn_blocking(move || {
        let conn = pool.get().ok()?;
        db::get_setting(&conn, "trusted_proxies").ok().flatten()
    })
    .await
    .ok()
    .flatten();
    let trusted = raw
        .map(|r| crate::proxies::parse_list(&r).unwrap_or_default())
        .unwrap_or_default();
    let xff = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    crate::proxies::client_ip(peer, xff, &trusted)
}

#[derive(Debug, Deserialize)]
pub(super) struct TrustedProxiesBody {
    trusted_proxies: String,
}

pub(super) async fn get_trusted_proxies(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let v = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<String>> {
        let conn = pool.get()?;
        db::get_setting(&conn, "trusted_proxies")
    })
    .await??;
    Ok(Json(json!({ "trusted_proxies": v.unwrap_or_default() })))
}

pub(super) async fn put_trusted_proxies(
    State(st): State<AppState>,
    Json(body): Json<TrustedProxiesBody>,
) -> Result<Response, AppError> {
    if let Err(msg) = crate::proxies::parse_list(&body.trusted_proxies) {
        return Ok(bad_request(msg));
    }
    let value = body.trusted_proxies.trim().to_string();
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        if value.is_empty() {
            db::delete_setting(&conn, "trusted_proxies")
        } else {
            db::set_setting(&conn, "trusted_proxies", &value)
        }
    })
    .await??;
    st.log_event(LogLevel::Info, "Trusted proxies updated");
    Ok(Json(json!({ "ok": true })).into_response())
}

pub(super) fn too_many_attempts(retry_after: std::time::Duration) -> Response {
    // Round up so a client honoring Retry-After never retries a hair early.
    let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, secs.to_string())],
        Json(json!({
            "error": format!("too many failed login attempts — try again in {secs}s"),
            "retry_after_secs": secs,
        })),
    )
        .into_response()
}

/// Shared by setup (auto-login right after creating the password) and
/// login: verify, create a session, set the cookie.
pub(super) async fn login_response(st: &AppState, password: &str) -> Result<Response, AppError> {
    let pool = st.pool.clone();
    let password = password.to_string();
    let token = auth::new_session_token();
    let token_hash = auth::hash_token(&token);

    let ok = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        let conn = pool.get()?;
        let Some(stored) = db::get_admin_password_hash(&conn)? else {
            return Ok(false);
        };
        if !auth::verify_password(&password, &stored) {
            return Ok(false);
        }
        let now = now_secs();
        db::prune_expired_sessions(&conn, now)?;
        db::create_session(&conn, &token_hash, now, now + SESSION_TTL_SECS)?;
        Ok(true)
    })
    .await??;

    if !ok {
        return Ok(unauthorized());
    }

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(SESSION_TTL_SECS))
        .build();
    let jar = CookieJar::new().add(cookie);
    Ok((jar, Json(json!({ "ok": true }))).into_response())
}

pub(super) async fn auth_logout(State(st): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Some(token) = jar.get(SESSION_COOKIE).map(|c| c.value().to_string()) {
        let pool = st.pool.clone();
        let _ = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = pool.get()?;
            db::delete_session(&conn, &auth::hash_token(&token))
        })
        .await;
    }
    let removal = Cookie::build((SESSION_COOKIE, ""))
        .path("/")
        .max_age(time::Duration::ZERO)
        .build();
    let jar = jar.add(removal);
    (jar, Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub(super) struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

/// Authenticated-only: change the admin password. The session that made
/// this request stays valid — only a reason to log everywhere out would be
/// "I think my session was compromised," which isn't this flow.
pub(super) async fn auth_change_password(
    State(st): State<AppState>,
    Json(body): Json<ChangePasswordBody>,
) -> Result<impl IntoResponse, AppError> {
    if body.new_password.len() < 8 {
        return Ok(bad_request("New password must be at least 8 characters").into_response());
    }
    let pool = st.pool.clone();
    let current = body.current_password.clone();
    let new_password = body.new_password.clone();
    let ok = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        let conn = pool.get()?;
        let Some(stored) = db::get_admin_password_hash(&conn)? else {
            return Ok(false);
        };
        if !auth::verify_password(&current, &stored) {
            return Ok(false);
        }
        let hash = auth::hash_password(&new_password)?;
        db::set_admin_password_hash(&conn, &hash)?;
        Ok(true)
    })
    .await??;
    if !ok {
        return Ok(bad_request("Current password is incorrect").into_response());
    }
    st.log_event(LogLevel::Info, "Admin password changed");
    Ok(Json(json!({ "ok": true })).into_response())
}

// ---- Authentik (OIDC) -----------------------------------------------------
