//! "Sign in with Plex" — the PIN-based OAuth-style flow Plex's own apps and
//! third-party tools (Overseerr, etc.) use. Endpoints and field names
//! verified against Plex's own forum docs and the python-plexapi reference
//! before writing this (not guessed) — see `oidc_redirect_uri`'s sibling
//! reasoning in `api/mod.rs` for the matching Authentik work.
//!
//! Unlike Authentik (self-hosted, operator-scoped), Plex.tv is a public
//! third-party identity provider — anyone can create a free account and
//! complete this flow. So a successful Plex login is NOT by itself proof of
//! being this app's admin, unlike the Authentik/password paths. It only
//! grants a session when the authenticated account's `uuid` matches a
//! previously **linked** account (see `db::get_plex_login_account` /
//! the Settings "Link your Plex account" action) — a deliberate allowlist
//! check, not a convenience shortcut.
//!
//! `X-Plex-Token`/`X-Plex-Product`/`X-Plex-Client-Identifier` are sent as
//! query parameters, not headers, matching both Plex's own docs and this
//! codebase's existing `plex.rs` convention (`X-Plex-Token` as a query
//! param there too — Plex's API accepts either, this app already picked
//! one).

use anyhow::{Context, Result};
use serde::Deserialize;

const PLEX_PRODUCT: &str = "meme-arr";

pub struct PlexPin {
    pub id: i64,
    pub code: String,
}

#[derive(Debug, Deserialize)]
struct PinResponse {
    id: i64,
    code: String,
}

/// Start a new PIN. `client_identifier` should be a stable value reused for
/// every request in this flow (and across the app's lifetime) — generated
/// once and persisted, per Plex's own guidance.
pub async fn create_pin(client_identifier: &str) -> Result<PlexPin> {
    let resp = reqwest::Client::new()
        .post("https://plex.tv/api/v2/pins")
        .header("accept", "application/json")
        .form(&[
            ("strong", "true"),
            ("X-Plex-Product", PLEX_PRODUCT),
            ("X-Plex-Client-Identifier", client_identifier),
        ])
        .send()
        .await
        .context("creating a Plex PIN")?
        .error_for_status()
        .context("Plex rejected the PIN request")?
        .json::<PinResponse>()
        .await
        .context("parsing Plex's PIN response")?;
    Ok(PlexPin {
        id: resp.id,
        code: resp.code,
    })
}

/// The URL to send the user's browser to. `forward_url` is where Plex sends
/// them back afterward — our own callback, carrying whatever we need to
/// look the flow back up (the PIN id).
pub fn auth_url(client_identifier: &str, code: &str, forward_url: &str) -> String {
    format!(
        "https://app.plex.tv/auth#?clientID={}&code={}&forwardUrl={}&context%5Bdevice%5D%5Bproduct%5D={}",
        urlencoding::encode(client_identifier),
        urlencoding::encode(code),
        urlencoding::encode(forward_url),
        urlencoding::encode(PLEX_PRODUCT),
    )
}

#[derive(Debug, Deserialize)]
struct CheckPinResponse {
    #[serde(rename = "authToken")]
    auth_token: Option<String>,
}

/// Whether the PIN has been claimed (the user approved the login at Plex)
/// yet. `None` means not yet, denied, or expired — all indistinguishable
/// from Plex's response, and all mean "not logged in."
pub async fn check_pin(client_identifier: &str, pin_id: i64, code: &str) -> Result<Option<String>> {
    let resp = reqwest::Client::new()
        .get(format!("https://plex.tv/api/v2/pins/{pin_id}"))
        .header("accept", "application/json")
        .query(&[
            ("code", code),
            ("X-Plex-Client-Identifier", client_identifier),
        ])
        .send()
        .await
        .context("checking the Plex PIN")?
        .error_for_status()
        .context("Plex rejected the PIN check")?
        .json::<CheckPinResponse>()
        .await
        .context("parsing Plex's PIN check response")?;
    Ok(resp.auth_token.filter(|t| !t.is_empty()))
}

#[derive(Debug, Deserialize, Clone)]
pub struct PlexAccount {
    pub uuid: String,
    pub username: String,
}

/// Resolve a Plex account token (from `check_pin`) to the account it
/// belongs to. `uuid` is the stable identifier this app actually checks
/// against the linked allowlist — `username`/`email` are mutable, `uuid`
/// isn't (matches Plex's own identity convention elsewhere, e.g. server
/// machine identifiers).
pub async fn get_account(token: &str, client_identifier: &str) -> Result<PlexAccount> {
    reqwest::Client::new()
        .get("https://plex.tv/api/v2/user")
        .header("accept", "application/json")
        .query(&[
            ("X-Plex-Product", PLEX_PRODUCT),
            ("X-Plex-Client-Identifier", client_identifier),
            ("X-Plex-Token", token),
        ])
        .send()
        .await
        .context("validating the Plex account")?
        .error_for_status()
        .context("Plex rejected the account token")?
        .json::<PlexAccount>()
        .await
        .context("parsing Plex's account response")
}
