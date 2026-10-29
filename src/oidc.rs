//! Authentik login via native OIDC — a registered OAuth2/OIDC client, not a
//! forward-auth reverse proxy (checked against Sonarr's own closest analog,
//! its "External" trusted-header method, before deciding: that requires
//! routing ALL traffic through an Authentik proxy, which rules out also
//! showing a plain password/Plex login on the same page). Standard
//! Authorization Code + PKCE flow via the `openidconnect` crate rather than
//! hand-rolled JWT verification — ID token signature/issuer/audience/nonce
//! checks are exactly the kind of thing worth not re-implementing.
//!
//! Single-admin model: a successful login from the configured Authentik
//! issuer is sufficient to grant the one admin session — there's no second
//! user identity to match it against. Restricting *who* can authenticate
//! through that Authentik application is the operator's job, configured on
//! the Authentik side (e.g. scoping the application to one user/group), not
//! something this app re-checks.

use anyhow::{anyhow, Context, Result};
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};

#[derive(Debug, Clone)]
pub struct OidcSettings {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
}

/// Everything the callback needs to finish the flow, generated at
/// `/api/auth/oidc/login` and persisted (via `db::create_oidc_flow`) keyed
/// by `state` until the callback consumes it.
#[derive(Debug)]
pub struct LoginStart {
    pub auth_url: String,
    pub state: String,
    pub nonce: String,
    pub pkce_verifier: String,
}

/// An HTTP client with redirects disabled — required by `openidconnect`'s
/// own docs to avoid SSRF via a malicious redirect from the issuer.
fn http_client() -> Result<openidconnect::reqwest::Client> {
    Ok(openidconnect::reqwest::ClientBuilder::new()
        .redirect(openidconnect::reqwest::redirect::Policy::none())
        .build()?)
}

/// Discover the provider's OIDC configuration (`.well-known/...`). Kept
/// separate from client construction because `CoreClient`'s type encodes
/// *which* endpoints are known (a type-state builder) — naming that type
/// as a shared helper's return type doesn't work once `.set_redirect_uri`
/// is chained on, so each caller below builds its own client inline
/// instead of sharing a `build_client` helper.
async fn discover(
    settings: &OidcSettings,
) -> Result<(CoreProviderMetadata, openidconnect::reqwest::Client)> {
    let http = http_client()?;
    let issuer =
        IssuerUrl::new(settings.issuer_url.clone()).context("invalid Authentik issuer URL")?;
    let metadata = CoreProviderMetadata::discover_async(issuer, &http)
        .await
        .context(
            "discovering Authentik's OIDC configuration (.well-known) — check the issuer URL",
        )?;
    Ok((metadata, http))
}

/// Begin a login: discover the provider, build the authorization URL with a
/// fresh state/nonce/PKCE challenge. The caller persists `state`/`nonce`/
/// `pkce_verifier` and redirects the browser to `auth_url`.
pub async fn start_login(settings: &OidcSettings, redirect_uri: &str) -> Result<LoginStart> {
    let (metadata, _http) = discover(settings).await?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(settings.client_id.clone()),
        Some(ClientSecret::new(settings.client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).context("invalid redirect URI")?);

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    Ok(LoginStart {
        auth_url: auth_url.to_string(),
        state: csrf_token.secret().clone(),
        nonce: nonce.secret().clone(),
        pkce_verifier: pkce_verifier.secret().clone(),
    })
}

/// Finish a login: exchange the authorization code, verify the ID token
/// (signature, issuer, audience, and the nonce from `start_login`). Returns
/// the subject claim on success, purely for the activity log — nothing in
/// this app keys off it (see module docs, single-admin model).
pub async fn complete_login(
    settings: &OidcSettings,
    redirect_uri: &str,
    code: &str,
    nonce: &str,
    pkce_verifier: &str,
) -> Result<String> {
    let (metadata, http) = discover(settings).await?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(settings.client_id.clone()),
        Some(ClientSecret::new(settings.client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).context("invalid redirect URI")?);

    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))?
        .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
        .request_async(&http)
        .await
        .context("exchanging the authorization code with Authentik")?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| anyhow!("Authentik did not return an ID token (is the `openid` scope enabled on the application?)"))?;
    let verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&verifier, &Nonce::new(nonce.to_string()))
        .context("verifying Authentik's ID token")?;

    Ok(claims.subject().as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_login_fails_cleanly_on_a_bad_issuer() {
        let settings = OidcSettings {
            issuer_url: "http://127.0.0.1:1".to_string(), // nothing listens here
            client_id: "x".to_string(),
            client_secret: "y".to_string(),
        };
        let err = start_login(&settings, "http://localhost/callback")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("discovering"));
    }
}
