# Authentication

Single admin account, no sign-up, three ways to prove you're that admin:
password, Authentik (OIDC), or a linked Plex.tv account. All three are
available side by side once configured — whichever are set up show up as
options on the login screen.

The design follows the \*arr apps' approach: credentials live in the app's own database, never
in a config file, so they can't be read back, only replaced. Recovery is driven by configuration
and a restart (see [Recovery](#password-auth-authrs) below).

## Sessions

- A random token (two concatenated UUIDv4s, ~244 bits — `auth::new_session_token`)
  goes in an `HttpOnly`, `SameSite=Lax` cookie named `memearr_session`.
  Deliberately **not** `Secure` — this app is routinely served over plain
  HTTP on a LAN, and `Secure` would silently break the cookie there. It
  still works fine behind HTTPS if you put a reverse proxy in front later.
- Only the **SHA-256 hash** of that token is stored server-side (`sessions`
  table) — a DB leak alone doesn't hand out valid sessions. This is a
  deliberately different (fast) hash from the password hash below, since
  every authenticated request needs a session lookup, not just login.
- Fixed 30-day TTL from login, no sliding/refresh-on-use. Expired sessions
  are swept opportunistically on the next login, not on a timer.
- The `require_auth` axum middleware (`api/mod.rs`) checks every request's
  path against `is_public_api_path` (an allowlist — see ARCHITECTURE.md's
  request lifecycle) and, for everything else, the session cookie against
  the `sessions` table.

## Password auth (`auth.rs`)

- **Argon2id** (the `argon2` crate, default parameters) for the password
  itself — deliberately slow, resists brute-forcing a stolen hash. Stored as
  a self-contained PHC string in `settings.admin_password_hash` (salt and
  parameters travel with it).
- First run: `setup_required` (no stored hash yet) shows a one-time "create
  your admin password" screen instead of a login form.
- **Recovery**: set `RESET_ADMIN_PASSWORD=<new password>` in the
  environment and restart. Checked once at the top of `lib::run()` on every
  startup — if set, it (re)writes the stored hash and logs a loud warning.
  Nothing auto-clears it; remove the env var yourself once you've logged in
  with the new password. This is the one and only recovery path — there's no
  email-based reset (self-hosted, no mail sending configured).
- **Brute-force protection** (`ratelimit.rs`): after 5 failed logins from
  one IP, that IP gets `429` with a `Retry-After` header — 30s, doubling
  with each further failure, capped at 15 minutes. A successful login
  resets the count; attempts made *during* a lockout don't extend it (so
  someone hammering the endpoint can't keep the real admin locked out
  indefinitely). In-memory only — a restart clears it.
  - Keyed on the TCP peer address by default, **not** `X-Forwarded-For`:
    that header is client-controlled, so trusting it would let an attacker
    claim a new "IP" every attempt and bypass the limit entirely. Behind a reverse proxy,
    all proxied traffic then shares the proxy's single bucket — stricter
    than ideal, but fail-safe.
  - **Trusted proxies** (Settings → Trusted proxies; like Sonarr's "Trusted
    Networks" / Jellyfin's "Known Proxies"): list your proxy's IP or CIDR
    range, and `X-Forwarded-For` is honored *only* when the TCP peer is in
    that list. `src/proxies.rs` then walks the header **from the right**,
    skipping further trusted hops; the first untrusted address is the
    client, and anything to its left (attacker-supplied) is ignored. A
    malformed entry stops the walk at the last good address. Empty = never
    trust the header. (Other places that read forwarded headers — the OIDC
    redirect URI's `X-Forwarded-Proto` — are unchanged.)
  - **Every failed login is logged** to the Activity log with its IP, and
    the moment an IP is locked out is logged too. Bounded: once locked out,
    further attempts are rejected without logging, so an attacker can't
    flood the log (it holds the newest 5,000 entries).
  - If you lock yourself out, wait it out, or restart the container.

## API key

An optional key for dashboards/scripts, managed in Settings → API key. It is
deliberately weaker than a login: **read-only, on a short allowlist of
endpoints** (health, tasks, index status, library stats, storage), sent as
an `X-Api-Key` header — never in the query string. It can't read settings or
backups (which contain secrets) or change anything, and it can't manage keys.
The key is 244 random bits with a `memearr_` prefix; only its SHA-256 hash is
stored, and it's shown once at generation, like a GitHub token. Sonarr's key
is full-power; this one isn't, because a dashboard widget has no business
being able to rewrite your Plex connection.

## Authentik (`oidc.rs`)

Native OIDC client (Authorization Code + PKCE via the `openidconnect`
crate), **not** a forward-auth reverse proxy. That alternative was
considered and rejected: Sonarr's closest equivalent is its "External"
trusted-header method, which requires routing *all* traffic through the
Authentik proxy — incompatible with also showing a plain password/Plex
button on the same login page.

Flow:
1. `GET /api/auth/oidc/login` discovers the provider (`.well-known/...`),
   builds an authorize URL with a fresh state/nonce/PKCE challenge, persists
   them (`oidc_flows`, 10-minute TTL, one-time-use), redirects the browser.
2. User approves at Authentik.
3. `GET /api/auth/oidc/callback` exchanges the code, verifies the ID token
   (signature, issuer, audience, **and the nonce** from step 1), creates a
   session.

The redirect URI is built from the **incoming request's own Host header**
(`request_origin()` in `api/mod.rs`, trusting `X-Forwarded-Proto` behind a
reverse proxy) rather than a separate "public URL" setting to keep in sync.

**Any successful login from the configured issuer grants the one admin
session** — there's no second identity to match against. *Who* can
authenticate through that Authentik application is scoped on the Authentik
side (restrict the application to one user/group), not re-checked here.
This is safe specifically *because* Authentik is self-hosted and the
operator controls access — contrast with Plex below.

Under a `URL_BASE` the redirect URI includes it (e.g. `https://host/meme-arr/api/auth/oidc/callback`).

Configure via Settings → issuer URL, client ID, client secret
(`GET/PUT/DELETE /api/settings/authentik`). Redirect URI to register on the
Authentik side: `https://<your-host>/api/auth/oidc/callback`.

## Plex login (`plex_auth.rs`)

The real PIN-based OAuth-style flow Plex's own apps and third-party tools
(Overseerr, etc.) use — endpoints and field names verified against Plex's
own forum docs and the python-plexapi reference before implementing, not
guessed:

1. `POST https://plex.tv/api/v2/pins` (with a stable, once-generated
   `X-Plex-Client-Identifier` — `settings.plex_client_identifier`) creates a
   PIN.
2. Browser goes to `https://app.plex.tv/auth#?...` with that PIN's code and
   a `forwardUrl` pointing back at this app's own callback.
3. User approves at Plex.
4. Plex redirects to the `forwardUrl`; `GET /api/v2/pins/<id>` confirms the
   claim and returns an access token; `GET /api/v2/user` resolves it to an
   account (`uuid`, `username`).

**Critical difference from Authentik**: Plex.tv is a *public* third-party
identity provider — anyone can create a free account. A bare successful
Plex login is therefore **not** proof of adminhood by itself. It only
grants a session when the authenticated account's `uuid` matches a
previously **linked** account (`settings.plex_login_account_uuid`).

Linking is its own authenticated-only flow: Settings → "Link your Plex
account" (`GET /api/settings/plex-login/link`) runs the *same* PIN mechanics
with `purpose="link"` stored on the `plex_flows` row instead of
`"login"`. The shared callback branches on that purpose — for `"link"` it
does its own inline session check (`has_valid_session`) rather than relying
on the blanket middleware, since the whole `/api/auth/plex/*` path has to
stay public for `"login"` to work at all.

`uuid` (not `username`/`email`) is the identifier actually checked —
stable, unlike the mutable display fields.
