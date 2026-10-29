//! Sign in with Plex: the PIN flow, callback and its settings.

use super::*;

/// Same reasoning as `OIDC_FLOW_TTL_SECS`.
pub(super) const PLEX_FLOW_TTL_SECS: i64 = 10 * 60;

/// Begin a Plex PIN flow for `purpose` ("login" or "link"), persist it,
/// and return the URL to redirect the browser to.
pub(super) async fn start_plex_flow(
    st: &AppState,
    purpose: &'static str,
    origin: &str,
) -> Result<String, String> {
    let pool = st.pool.clone();
    let client_id = tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let conn = pool.get()?;
        db::get_or_create_plex_client_identifier(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let pin = plex_auth::create_pin(&client_id)
        .await
        .map_err(|e| format!("Could not reach Plex: {e:#}"))?;

    let pool = st.pool.clone();
    tokio::task::spawn_blocking({
        let code = pin.code.clone();
        move || -> anyhow::Result<()> {
            let conn = pool.get()?;
            let now = now_secs();
            db::prune_expired_plex_flows(&conn, now)?;
            db::create_plex_flow(&conn, pin.id, &code, purpose, now, now + PLEX_FLOW_TTL_SECS)
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let forward_url = format!("{origin}/api/auth/plex/callback?pin_id={}", pin.id);
    Ok(plex_auth::auth_url(&client_id, &pin.code, &forward_url))
}

/// Redirects the browser to Plex. Public — this *is* a login path.
pub(super) async fn plex_login(
    State(st): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Ok(origin) = app_origin(&st, &headers) else {
        return bad_request("Missing Host header").into_response();
    };
    match start_plex_flow(&st, "login", &origin).await {
        Ok(url) => axum::response::Redirect::to(&url).into_response(),
        Err(e) => bad_request(e).into_response(),
    }
}

/// Begin linking: authenticated only (Settings-driven). Same mechanics as
/// `plex_login`, different `purpose`, which the callback uses to decide
/// "grant a session" vs. "set the allowlist."
pub(super) async fn plex_login_link_start(
    State(st): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
) -> Response {
    if !has_valid_session(&st, &jar).await {
        return unauthorized();
    }
    let Ok(origin) = app_origin(&st, &headers) else {
        return bad_request("Missing Host header").into_response();
    };
    match start_plex_flow(&st, "link", &origin).await {
        Ok(url) => axum::response::Redirect::to(&url).into_response(),
        Err(e) => bad_request(e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct PlexCallbackParams {
    pin_id: i64,
}

/// Plex redirects the browser back here (via `forwardUrl`) once the user
/// has approved or denied the login — by the time this fires, the PIN is
/// already claimed or denied, so one check suffices (no polling). Public
/// (same reasoning as `oidc_callback`); the "link" purpose does its own
/// inline session check instead of relying on the blanket middleware,
/// since this whole path has to stay reachable without one for "login".
pub(super) async fn plex_callback(
    State(st): State<AppState>,
    jar: CookieJar,
    Query(p): Query<PlexCallbackParams>,
) -> Response {
    let pool = st.pool.clone();
    let flow = match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::take_plex_flow(&conn, p.pin_id, now_secs())
    })
    .await
    {
        Ok(Ok(f)) => f,
        _ => {
            return bad_request("Login attempt expired or was already used — try again")
                .into_response()
        }
    };
    let Some((code, purpose)) = flow else {
        return bad_request("Login attempt expired or was already used — try again")
            .into_response();
    };

    let pool = st.pool.clone();
    let client_id = match tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        let conn = pool.get()?;
        db::get_or_create_plex_client_identifier(&conn)
    })
    .await
    {
        Ok(Ok(id)) => id,
        _ => return bad_request("Internal error").into_response(),
    };

    let token = match plex_auth::check_pin(&client_id, p.pin_id, &code).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return bad_request("Plex login was not completed or was denied").into_response()
        }
        Err(e) => {
            warn!("Plex PIN check failed: {e:#}");
            return bad_request(format!("Could not reach Plex: {e:#}")).into_response();
        }
    };
    let account = match plex_auth::get_account(&token, &client_id).await {
        Ok(a) => a,
        Err(e) => {
            warn!("Plex account lookup failed: {e:#}");
            return bad_request(format!("Could not verify the Plex account: {e:#}"))
                .into_response();
        }
    };

    if purpose == "link" {
        if !has_valid_session(&st, &jar).await {
            return unauthorized();
        }
        let pool = st.pool.clone();
        let (account_uuid, username) = (account.uuid.clone(), account.username.clone());
        if let Err(e) = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let conn = pool.get()?;
            db::set_plex_login_account(&conn, &account_uuid, &username)
        })
        .await
        {
            warn!("saving linked Plex account failed: {e}");
            return bad_request("Could not save the linked account").into_response();
        }
        st.log_event(
            LogLevel::Info,
            format!("Linked Plex account for login: {}", account.username),
        );
        return axum::response::Redirect::to(&format!("{}/", st.config.url_base)).into_response();
    }

    // purpose == "login": only grants a session if this account was
    // previously linked -- see plex_auth's module docs on why a bare
    // successful Plex.tv login is never enough by itself.
    let pool = st.pool.clone();
    let linked = match tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_plex_login_account(&conn)
    })
    .await
    {
        Ok(Ok(l)) => l,
        _ => return bad_request("Internal error").into_response(),
    };
    match linked {
        Some((uuid, _)) if uuid == account.uuid => {}
        _ => {
            st.log_event(
                LogLevel::Warn,
                format!(
                    "Rejected Plex login from an unlinked account ({})",
                    account.username
                ),
            );
            return bad_request("This Plex account isn't linked to this app").into_response();
        }
    }

    let session_token = auth::new_session_token();
    let token_hash = auth::hash_token(&session_token);
    let pool = st.pool.clone();
    if let Err(e) = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        let now = now_secs();
        db::prune_expired_sessions(&conn, now)?;
        db::create_session(&conn, &token_hash, now, now + SESSION_TTL_SECS)
    })
    .await
    {
        warn!("creating session after Plex login failed: {e}");
        return bad_request("Internal error").into_response();
    }

    st.log_event(
        LogLevel::Info,
        format!("Logged in via Plex ({})", account.username),
    );
    let cookie = Cookie::build((SESSION_COOKIE, session_token))
        .http_only(true)
        .same_site(axum_extra::extract::cookie::SameSite::Lax)
        .path("/")
        .max_age(time::Duration::seconds(SESSION_TTL_SECS))
        .build();
    let jar = CookieJar::new().add(cookie);
    (
        jar,
        axum::response::Redirect::to(&format!("{}/", st.config.url_base)),
    )
        .into_response()
}

pub(super) async fn get_plex_login_settings(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    let linked = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let conn = pool.get()?;
        db::get_plex_login_account(&conn)
    })
    .await??;
    Ok(Json(match linked {
        Some((_, username)) => json!({ "linked": true, "username": username }),
        None => json!({ "linked": false, "username": null }),
    }))
}

pub(super) async fn delete_plex_login_settings(
    State(st): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let pool = st.pool.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let conn = pool.get()?;
        db::delete_plex_login_account(&conn)
    })
    .await??;
    st.log_event(LogLevel::Info, "Plex login unlinked");
    Ok(Json(json!({ "ok": true })))
}
