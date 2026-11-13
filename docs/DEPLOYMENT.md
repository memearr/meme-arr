# Deployment

## Image

`ghcr.io/memearr/meme-arr` — multi-stage build (`Dockerfile`):

1. `node:24-bookworm-slim` builds the SPA (`web/` → `dist/`).
2. `rust:1-bookworm` builds the release binary (dependency compilation
   cached as a separate layer — a dummy `src/main.rs` is built first so
   `Cargo.toml`/`Cargo.lock` changes alone don't force a full rebuild).
3. `debian:bookworm-slim` runtime — installs `ffmpeg`, `fonts-dejavu-core`,
   `ca-certificates`, copies the binary + built SPA, bundles DejaVu as the
   default caption font (drop an `Impact.ttf` into `/app/fonts` to override
   — preferred when present).

One container, port `8080`, `linux/amd64` only. `HEALTHCHECK` hits
`/healthz`. `VOLUME`s declared for `/data` and `/output`.

**Runs as non-root.** `docker-entrypoint.sh` starts as root only long enough
to `chown` `/data` and `/output` to `PUID:PGID` (default `1000:1000`, the same
convention as the LinuxServer.io Sonarr/Radarr images), then drops privileges
with `setpriv`. The chown is what lets a volume written by an older,
root-running version keep working after an upgrade; it only touches files
not already owned correctly, so it's cheap after the first start. Media
mounts stay readable as long as they're world-readable (the example CIFS
options use `file_mode=0444,dir_mode=0555`). `PUID=0` opts back into running
as root. CI replays the root-owned-volume upgrade path on every build.

**The image is public**, so pulling needs no login. (If you run a private fork, its package
is private too: `docker login ghcr.io` with a `read:packages` token, or make the package
public in the GitHub UI.)

## Release process

`.github/workflows/release.yml` triggers on any `v*` tag push (or manual
dispatch) and pushes both `:latest` and `:vX.Y.Z` to GHCR, with
`APP_VERSION` baked in as a build arg (shown in the UI's sidebar and
`/api/version`/`/status`).

```bash
git checkout main && git pull
cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
(cd web && npm run build)
git tag -a vX.Y.0 -m "vX.Y.0: <summary of what's shipping>"
git push origin vX.Y.0
gh run watch <run-id> --exit-status   # or: gh run list --workflow=release.yml
```

Version numbers so far have bumped the minor version per shipped batch of
PRs (no real pattern beyond "something shipped, bump it") — there's no
enforced semver discipline, just sequential `v0.N.0` tags.

## Running it

### docker-compose / Portainer (`docker-compose.yml`)

A step-by-step version with screenshots is [Getting started](GETTING-STARTED.md); this
section is the reference.

The committed `docker-compose.yml` is the simple setup: one read-only bind mount of
your media, plus `./data` (settings and the index) and `./output`:

```yaml
services:
  meme-arr:
    image: ghcr.io/memearr/meme-arr:latest
    container_name: meme-arr
    restart: unless-stopped
    ports:
      - "8089:8080"
    environment:
      PUID: "1000"
      PGID: "1000"
      INDEX_ON_START: "true"
    volumes:
      - ./data:/data
      - ./output:/output
      - /volume1/video:/media:ro   # your media; add more mounts if it is spread over several places
```

Media access is **direct file mount**, not the media server's API: this app reads the
same files Plex or Jellyfin does, over the same folders or shares, and rewrites the
path the server reports to wherever you've mounted it (see `paths.rs` and the
Settings page's path-mapping UI). If Docker itself has to mount network shares,
[`examples/smb/`](https://github.com/memearr/meme-arr/tree/main/examples/smb)
has a compose file wiring up five read-only CIFS/SMB mounts; adjust it to however
your media is actually shared.

### Environment variables

Everything here except the bind address/dirs is **first-run bootstrap
only** — Plex URL/token and path mappings are meant to be configured on the
Settings page after that, which persists them in SQLite. Editing these env
vars on an already-configured deployment does nothing.

| Variable | Default | Purpose |
|---|---|---|
| `BIND_ADDR` | `0.0.0.0:8080` | |
| `DATA_DIR` | `/data` | SQLite DB, compact-index file, temp/preview-cache dir |
| `OUTPUT_DIR` | `/output` | finished exports land here |
| `WEB_DIR` | `/app/web` | built SPA static files |
| `FONTS_DIR` | `/app/fonts` | caption fonts |
| `BROWSE_ROOTS` | `/` | comma-separated folders the setup wizard's folder picker may browse; set e.g. `/media,/mnt` to keep it out of the rest of the container |
| `INDEX_ON_START` | `true` | whether the **Library scan** task starts enabled when it's first created; after that its schedule lives in System → Tasks. A restart no longer rescans unconditionally — the scan runs on boot only if it's due (or has never run) |
| `FFMPEG_BIN` / `FFPROBE_BIN` | `ffmpeg` / `ffprobe` | overridable for non-standard installs |
| `GIFSICLE_BIN` | `gifsicle` | post-process lossy recompression pass on GIF exports; a missing/misconfigured binary just skips the optimization (logged, non-fatal) rather than failing the export |
| `PLEX_URL` / `PLEX_TOKEN` | — | first-run-only bootstrap |
| `PATH_MAP` | — | first-run-only bootstrap, format `plexPrefix1=>containerPrefix1,plexPrefix2=>containerPrefix2` |
| `APP_VERSION` | `dev` (baked in by the release build) | shown in the UI, `/api/version`, `/status` |
| `GITHUB_REPO` | `memearr/meme-arr` | update-check target |
| `GITHUB_TOKEN` | — | only for the update check against a **private** fork — without it, GitHub's API returns 404 (not 401/403) for every tags lookup, and the sidebar version check will never find a newer release. A classic PAT with `repo` scope (or a fine-grained token scoped to the repo with "Contents: Read-only") works. Not needed for the public repo. |
| `URL_BASE` | — | host the app under a sub-path, e.g. `/meme-arr` (like Sonarr's URL Base) — see "Behind a reverse proxy" below. Invalid values stop startup with an explanation |
| `PUBLIC_URL` | — | the address people use to reach the app, e.g. `https://memes.example.com` (may include a sub-path). Overrides what a reverse proxy's headers say about HTTPS, host and prefix — see "Behind a reverse proxy" |
| `HSTS_MAX_AGE` | `0` | seconds for a `Strict-Transport-Security` header on HTTPS requests; `0` = off |
| `RESET_ADMIN_PASSWORD` | — | auth recovery, see AUTHENTICATION.md — checked every startup, not auto-cleared |
| `PUID` / `PGID` | `1000` / `1000` | user/group the app runs as after the entrypoint fixes volume ownership; `PUID=0` keeps running as root |
| `RUST_LOG` | — | standard `tracing-subscriber` env filter, e.g. `info` |

## Behind a reverse proxy

The app speaks plain HTTP on port 8080. To serve it over HTTPS, put a reverse
proxy (nginx, Caddy, Traefik, Apache, Nginx Proxy Manager, SWAG…) in front, let
it hold the certificate, and forward to `http://meme-arr:8080`.

### What the app needs from the proxy

Four headers describe the visitor's real connection. Without them the app
only sees the proxy's own plain-HTTP connection:

| Header | Used for |
|---|---|
| `X-Forwarded-Proto` (or `Forwarded: proto=`) | marking the login cookie `Secure`, and `https://` in the Authentik and Plex login callback addresses |
| `X-Forwarded-Host` (or `Host`, or `Forwarded: host=`) | the host name in those callback addresses |
| `X-Forwarded-For` | the visitor's address, for login lockouts and logs |
| `X-Forwarded-Prefix` | a path your proxy adds and strips (see below) |

A header is **believed only when the connection comes from a proxy of the
app's**: an address on **Settings → Trusted proxies**, or a private address
(`10.x`, `172.16–31.x`, `192.168.x`, loopback, IPv6 unique-local) — which is where a
Docker or home-network proxy lives. A visitor on the open internet who sends
these headers is ignored, so nobody can make the app build a login callback to
a site of their choosing. The exception is `X-Forwarded-For`: it is used only
from the addresses you list under Trusted proxies, so list your proxy there
too, or every visitor looks like the proxy to the login lockout.

If your proxy can't send the headers, or you'd rather not rely on them, set
**`PUBLIC_URL`** (for example `https://memes.example.com`) and the app uses
that address for everything, ignoring the headers.

**Settings → Check this connection** shows what the app makes of the connection
you are using (HTTPS or not, the host, the client address, the callback
addresses) and points out what looks wrong. Open the app the way you normally
do, through the proxy, and press Check.

### Examples

At a host name of its own:

```nginx
server {
    listen 443 ssl;
    server_name memes.example.com;
    # ssl_certificate ... ssl_certificate_key ...

    client_max_body_size 20m;        # font uploads
    location / {
        proxy_pass http://meme-arr:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_read_timeout 300s;     # long previews and exports
        proxy_buffering off;         # stream previews and downloads
    }
}
```

```caddyfile
memes.example.com {
    reverse_proxy meme-arr:8080
}
```

Caddy sends the forwarded headers by itself, and gets the certificate.

```yaml
# Traefik, as labels on the meme-arr container
labels:
  - traefik.enable=true
  - traefik.http.routers.memes.rule=Host(`memes.example.com`)
  - traefik.http.routers.memes.entrypoints=websecure
  - traefik.http.routers.memes.tls.certresolver=letsencrypt
  - traefik.http.services.memes.loadbalancer.server.port=8080
```

```apache
<VirtualHost *:443>
    ServerName memes.example.com
    # SSLEngine on ... certificates ...
    ProxyPreserveHost On
    RequestHeader set X-Forwarded-Proto "https"
    ProxyPass        / http://meme-arr:8080/
    ProxyPassReverse / http://meme-arr:8080/
    ProxyTimeout 300
</VirtualHost>
```

In **Nginx Proxy Manager**, add a proxy host to `meme-arr:8080`, turn on "Block
Common Exploits" if you like, enable SSL (Let's Encrypt), and leave "Websockets
Support" off; it sends the forwarded headers itself. Whatever the proxy, give
previews and exports a long read timeout (a few minutes) and don't buffer
responses; the app supports `Range` requests, which the video player and
resumable downloads need, so don't strip that header.

### Under a sub-path

To share a host name (`https://home.example.com/meme-arr/`) there are two ways:

- **The proxy keeps the prefix** (same convention as Sonarr/Radarr): set
  `URL_BASE=/meme-arr` and forward the whole path unchanged.

  ```nginx
  location /meme-arr/ {
      proxy_pass http://meme-arr:8080;   # no trailing slash/URI: keeps /meme-arr
      proxy_set_header Host $host;
      proxy_set_header X-Forwarded-Proto $scheme;
      proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
  }
  ```

- **The proxy strips the prefix** (Traefik `StripPrefix`, nginx with a trailing
  slash on `proxy_pass`, Caddy `handle_path`): leave `URL_BASE` unset and have
  the proxy send **`X-Forwarded-Prefix: /meme-arr`**. The app puts that prefix in
  front of every link and callback address. (Traefik's `StripPrefix` middleware
  sets this header itself.)

  ```nginx
  location /meme-arr/ {
      proxy_pass http://meme-arr:8080/;  # trailing slash: strips /meme-arr
      proxy_set_header X-Forwarded-Prefix /meme-arr;
      proxy_set_header Host $host;
      proxy_set_header X-Forwarded-Proto $scheme;
  }
  ```

With a base set, everything (page, assets, `/api/*`) lives under it, and
`/meme-arr` redirects to `/meme-arr/`. `/healthz` also still answers at the
container's root, so the image's `HEALTHCHECK` works unchanged. Callback URLs
(Authentik, Plex login) include the base: register
`https://home.example.com/meme-arr/api/auth/oidc/callback` with Authentik. The
built frontend is base-agnostic (relative assets + a value the server injects
into `index.html`), so one image serves every layout.

### HTTPS details

- The session cookie is marked **Secure** whenever the visitor used HTTPS (as seen
  through the headers or `PUBLIC_URL`) and left unmarked on plain HTTP, where a
  browser would drop it.
- **`HSTS_MAX_AGE`** (seconds, default `0` = off) adds `Strict-Transport-Security`
  to responses for HTTPS requests. Many people set this on the proxy instead;
  use one or the other. Start small (`300`) before committing to a year.
- Opening the app over plain HTTP on a LAN keeps working; only the clipboard
  "Copy" buttons and Secure cookies differ, and the app handles both.

## First run

Open the app, set the admin password, and a **setup wizard** appears while
Plex isn't configured: (1) enter the Plex URL and token and test the
connection; (2) map each movie/TV library's Plex folder to where it's mounted
in the container — suggestions are pre-filled by matching folder names
against what's under `/media` (a Windows `D:\` matches a `d` or `drive-d`
folder); (3) save, see the health check's verdict (unreadable mounts, Plex
unreachable, missing tools), and start indexing. Skipping it is fine — the
same settings live under Settings. It reappears (per browser session) until
Plex is configured.

## Backups

The **Back up settings** task writes a JSON snapshot of your configuration to
`/data/backups/` daily (keeping 14) — manage them under System → Backups.
Only settings, path mappings and task schedules are included, not the
subtitle index (rebuild it with a library scan), so restoring onto a fresh
container is: mount a new `/data`, open the app, System → Backups → Restore,
then run the library scan. Files contain your Plex token and login secrets.
Because `/data` is where they live, they're lost with the volume — download
a copy somewhere else if you want protection from that.

## CI (`.github/workflows/ci.yml`)

Runs on GitHub-hosted Linux runners on every push to `main` and every PR (forks
included). Jobs:
- **docs**: the documentation tests (`scripts/`): every doc is mapped to a wiki page and every
  relative link resolves.
- **backend**: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --all` (installs `ffmpeg` first; the pipeline tests need it for real).
- **audit**: `cargo audit` against `Cargo.lock` (reviewed exceptions in `.cargo/audit.toml`).
- **frontend**: `npm test`, `npm run build` and `npm audit --audit-level=high` in `web/`.
- **docker**: after backend and frontend pass, builds the image and smoke-tests it (non-root
  start, upgrade from a root-owned volume, health check, subtitle burn-in).

All of them must pass before a PR is merged (see `docs/DEVELOPMENT.md` for the workflow).

## Known deploy gotchas

See `docs/OPERATIONS.md` for the full list (CIFS `$` escaping, named-volume
`driver_opts` caching, snap-Docker AppArmor confinement, Windows path
normalization) — real issues hit and solved during this app's own
deployment, not hypothetical.
