//! Authentik (OIDC) single sign-on: login, callback, and its settings.

use super::*;

/// How long an in-progress OIDC login attempt is good for before the
/// callback has to be rejected as stale -- generous enough for someone to
/// actually go log in at Authentik, short enough that an abandoned attempt
/// doesn't linger.
pub(super) const OIDC_FLOW_TTL_SECS: i64 = 10 * 60;

/// Build this deployment's own callback URL from the incoming request's
/// Host header, trusting `X-Forwarded-Proto` if a reverse proxy set it
/// (defaulting to plain `http` otherwise, since this app is routinely
/// served that way on a LAN). No separate "public URL" setting to keep in
/// sync — the same approach several other self-hosted apps use.
/// This deployment's own origin (`scheme://host`), from the incoming
/// request's Host header, trusting `X-Forwarded-Proto` if a reverse proxy
/// set it (defaulting to plain `http` otherwise — this app is routinely
/// served that way on a LAN). Used to build callback URLs for both
/// Authentik and Plex login without a separate "public URL" setting to
/// keep in sync.
pub(super) fn request_origin(headers: &axum::http::HeaderMap) -> Result<String, &'static str> {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing Host header")?;
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    Ok(format!("{scheme}://{host}"))
}

/// `request_origin` plus the configured URL base -- where this app lives, for
/// building URLs a browser or Authentik will be sent back to.
pub(super) fn app_origin(
    st: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<String, &'static str> {
    Ok(format!(
        "{}{}",
        request_origin(headers)?,
        st.config.url_base
    ))
}

pub(super) fn oidc_redirect_uri(
    st: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<String, &'static str> {
    Ok(format!(
        "{}/api/auth/oidc/callback",
        app_origin(st, headers)?
    ))
}

/// Redirects the browser to Authentik. Public (no session needed yet) —
/// this *is* a login path.
pub(super) async fn oidc_login(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<Response, AppError> {
    let redirect_uri = match oidc_redirect_uri(&st, &headers) {
        Ok(u) => u,
        Err(msg) => return Ok(bad_request(msg).into_response()),
    };
    let pool = st.pool.clone();
    let settings = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_authentik_settings(&conn)
    })
    .await??;
    let Some(settings) = settings else {
        return Ok(bad_request("Authentik isn't configured").into_response());
    };

    let start = match oidc::start_login(&settings, &redirect_uri).await {
        Ok(s) => s,
        Err(e) => {
            warn!("starting Authentik login failed: {e:#}");
            return Ok(bad_request(format!("Could not reach Authentik: {e:#}")).into_response());
        }
    };

    let pool = st.pool.clone();
    let (state_token, nonce, verifier) = (start.state, start.nonce, start.pkce_verifier);
    tokio::task::spawn_blocking({
        let (state_token, nonce, verifier) = (state_token.clone(), nonce.clone(), verifier.clone());
        move || -> anyhow::Result<()> {
            let conn = pool.get()?;
            let now = now_secs();
            db::prune_expired_oidc_flows(&conn, now)?;
            db::create_oidc_flow(
                &conn,
                &state_token,
                &nonce,
                &verifier,
                now,
                now + OIDC_FLOW_TTL_SECS,
            )
        }
    })
    .await??;

    Ok(axum::response::Redirect::to(&start.auth_url).into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct OidcCallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Authentik redirects back here after the user approves (or denies) the
/// login. Public, same reasoning as `oidc_login`.
pub(super) async fn oidc_callback(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(p): Query<OidcCallbackParams>,
) -> Result<Response, AppError> {
    if let Some(e) = p.error {
        return Ok(bad_request(format!("Authentik login failed: {e}")).into_response());
    }
    let (Some(code), Some(state_token)) = (p.code, p.state) else {
        return Ok(bad_request("Missing code/state from Authentik").into_response());
    };
    let redirect_uri = match oidc_redirect_uri(&st, &headers) {
        Ok(u) => u,
        Err(msg) => return Ok(bad_request(msg).into_response()),
    };

    let pool = st.pool.clone();
    let flow = tokio::task::spawn_blocking({
        let state_token = state_token.clone();
        move || -> anyhow::Result<Option<(String, String)>> {
            let conn = pool.get()?;
            db::take_oidc_flow(&conn, &state_token, now_secs())
        }
    })
    .await??;
    let Some((nonce, pkce_verifier)) = flow else {
        return Ok(
            bad_request("Login attempt expired or was already used — try again").into_response(),
        );
    };

    let pool = st.pool.clone();
    let settings = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_authentik_settings(&conn)
    })
    .await??;
    let Some(settings) = settings else {
        return Ok(bad_request("Authentik isn't configured").into_response());
    };

    let subject =
        match oidc::complete_login(&settings, &redirect_uri, &code, &nonce, &pkce_verifier).await {
            Ok(s) => s,
            Err(e) => {
                warn!("Authentik login failed: {e:#}");
                return Ok(bad_request(format!("Login failed: {e:#}")).into_response());
            }
        };

    let token = auth::new_session_token();
    let token_hash = auth::hash_token(&token);
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        let now = now_secs();
        db::prune_expired_sessions(&conn, now)?;
        db::create_session(&conn, &token_hash, now, now + SESSION_TTL_SECS)
    })
    .await??;

    st.log_event(
        LogLevel::Info,
        format!("Logged in via Authentik (subject: {subject})"),
    );

    let cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(SESSION_TTL_SECS))
        .build();
    let jar = CookieJar::new().add(cookie);
    Ok((
        jar,
        axum::response::Redirect::to(&format!("{}/", st.config.url_base)),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub(super) struct AuthentikSettingsPayload {
    issuer_url: String,
    client_id: String,
    /// Only applied when present and non-empty; otherwise the saved secret is kept.
    #[serde(default)]
    client_secret: Option<String>,
}

pub(super) async fn get_authentik_settings(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let settings = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_authentik_settings(&conn)
    })
    .await??;
    Ok(Json(match settings {
        Some(s) => json!({
            "issuer_url": s.issuer_url,
            "client_id": s.client_id,
            "secret_set": true,
            "configured": true,
        }),
        None => {
            json!({ "issuer_url": "", "client_id": "", "secret_set": false, "configured": false })
        }
    }))
}

pub(super) async fn put_authentik_settings(
    State(st): State<AppState>,
    Json(p): Json<AuthentikSettingsPayload>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        let issuer_url = p.issuer_url.trim().trim_end_matches('/').to_string();
        let client_id = p.client_id.trim().to_string();
        let existing_secret = db::get_authentik_settings(&conn)?.map(|s| s.client_secret);
        let client_secret = match p.client_secret.as_deref().map(str::trim) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => existing_secret.unwrap_or_default(),
        };
        db::set_authentik_settings(&conn, &issuer_url, &client_id, &client_secret)
    })
    .await??;
    st.log_event(LogLevel::Info, "Authentik settings updated");
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn delete_authentik_settings_handler(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::delete_authentik_settings(&conn)
    })
    .await??;
    st.log_event(LogLevel::Info, "Authentik settings removed");
    Ok(Json(json!({ "ok": true })))
}

// ---- Plex login -------------------------------------------------------
