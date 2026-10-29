# Architecture

## System overview

```
Browser (React SPA, built to static files)
   │  HTTP, same origin
   ▼
axum HTTP server (single Rust binary, single port 8080)
   │
   ├─► auth middleware (session cookie check, see AUTHENTICATION.md)
   │
   ├─► SQLite (bundled, WAL mode)
   │      ├─ media / subtitle_cue / subtitle_fts   (catalog + dialogue index)
   │      ├─ settings / path_mapping                (app config, in-DB not env)
   │      ├─ activity_log                           (persistent event log)
   │      └─ sessions / oidc_flows / plex_flows      (auth state)
   │
   ├─► Plex HTTP API (reqwest client)  — library/item metadata, search, auth
   │
   ├─► ffmpeg / ffprobe (subprocess)   — probe, extract subs, preview, export
   │      └─ reads original media files directly via mounted volumes
   │
   └─► plex.tv / an Authentik instance (reqwest) — only during a login flow
```

One container. The Rust binary serves the JSON API under `/api/*` (plus
`/healthz`, `/status`, `/metrics`) **and** the built SPA as static files from
the same port — there's no separate frontend server in production.

## Why these choices (in brief)

- **Rust + axum, ffmpeg as a subprocess, not FFI bindings.** ffmpeg does the
  real work regardless of language; shelling out keeps the binary small and
  the failure modes simple (a timeout or non-zero exit, not a segfault in-process).
- **Direct file access over Plex's own streaming/download.** Plex reports
  each file's path *on the Plex server*; this app mounts the same shares and
  rewrites that prefix to its own container path (`paths.rs`), so ffmpeg
  seeks directly into the original file — instant, lossless. A Plex-API
  download fallback is scaffolded (`plex::part_download_url`) but not wired
  in; if a file isn't reachable on a mounted volume, that item is simply
  marked unreachable rather than falling back to downloading it.
- **SQLite + FTS5, not a separate DB server.** Embedded, no extra service,
  millisecond full-text search over tens of millions of subtitle cues, ships
  inside the one container. See `docs/OPERATIONS.md` and the project's deep
  research into whether a from-scratch index could beat FTS5 on size — the
  prototype exists (`compactindex.rs`) but FTS5 is still what live search runs on.
- **Settings live in the DB, not purely in env vars.** Plex URL/token and
  path mappings are configured on the Settings page and persisted in SQLite;
  env vars (`PLEX_URL`, `PLEX_TOKEN`, `PATH_MAP`) only *seed* the DB on first
  run, for existing env-configured deployments. After first run, editing the
  env vars does nothing — use Settings.
- **Vite SPA served as static files.** The trim timeline needs real
  interactivity (scrub, caption preview), but there's no reason to run Node
  in the runtime image — it's built once at image-build time.

## Request lifecycle (typical authenticated API call)

1. Request hits axum's router (`api::router` in `src/api/mod.rs`).
2. The `require_auth` middleware checks the path against `is_public_api_path`
   (an allowlist: `/healthz`, `/status`, `/metrics`, `/api/version`, and
   everything under `/api/auth/*`). Everything else needs a valid session
   cookie, checked against the `sessions` table (SHA-256 hash of the cookie
   value, never the raw token, stored server-side).
3. The handler runs. Most handlers that touch SQLite wrap the actual query in
   `tokio::task::spawn_blocking`, since `rusqlite` is synchronous — this
   keeps blocking DB/filesystem work off the async executor.
4. JSON response. Errors use a single `AppError` wrapper
   (`impl IntoResponse`) that turns any `anyhow::Error` into a `500` with
   `{"error": "..."}`; handlers that need other status codes (`400`, `401`,
   `404`, `409`) build those responses directly.

## Module map (`src/`)

Each one-liner below is lifted directly from that file's own `//!` doc
comment — check the file itself for the full picture, this is just
orientation.

| Module | Purpose |
|---|---|
| `main.rs` | Binary entry point. All real logic lives in the library crate (`lib.rs`) so integration tests can reach the same modules. |
| `lib.rs` | Builds `AppState`, kicks off background tasks (initial index, compact-index load/rebuild), and serves the HTTP API + SPA. `run()` is the whole server entry point. |
| `api/mod.rs` | Every HTTP handler and the router. The biggest file in the crate — see API.md for the endpoint-by-endpoint reference. |
| `state.rs` | `AppState` (shared, cloned into every handler) and the in-memory status types for the background indexer and export jobs. |
| `config.rs` | Runtime configuration, loaded from environment variables once at startup. |
| `db.rs` | All SQLite access: schema/migrations, and every query function. Nothing outside `db.rs` writes raw SQL against the app's own tables. |
| `index.rs` | The background library indexer: walks every movie/episode, resolves its file, extracts the best subtitle track with ffmpeg, parses cues, stores them. |
| `diagnose.rs` | Read-only "why is this subtitle still Waiting?" report: compares the video's folder (files, what the indexer picks, stored vs current fingerprint) with the subtitle streams the media server lists, and turns it into plain-words findings. Served at `GET /api/media/{id}/subtitles/diagnose`, shown by the Requests tab's Diagnose button. |
| `querysyntax.rs` | The search-box operators (`show:`, `ep:`, `movie:`, `library:`, `season:`, a leading `-` to exclude, `-word`): a small pure parser that splits a query into the text to search for, filters and excluded words. The database layer turns the filters into SQL (`db::Scope`). |
| `subtitles.rs` | Parses SRT/VTT text into timed cues, and builds a trimmed SRT for a clip window (shifted to start at zero) for burning at export time. |
| `ffmpeg.rs` | Thin orchestration over `ffmpeg`/`ffprobe` subprocesses. Argument-building is pure and unit-tested; actually running the process is not. |
| `export.rs` | Turns a chosen clip window into an animated WebP/MP4/WebM/GIF, with optional burned subtitles or custom meme text. Filtergraph construction is pure/tested. |
| `mediaserver.rs` | The seam between the app and the media server: `MediaServer` (Plex or Jellyfin), the shared `Section` / `Item` types, and `ServerKind`. Everything downstream (indexer, health, path mapping) is server-agnostic. |
| `jellyfin.rs` | Minimal Jellyfin client — `GET /Library/VirtualFolders` and paged `GET /Items`, authenticated with an API key. Mirrors `plex.rs`. |
| `plex.rs` | Minimal Plex Media Server client — list libraries, enumerate movies/episodes with their on-disk file path and part key. |
| `paths.rs` | Translates the paths Plex reports into paths reachable from this container, and decides whether a part is reachable directly on disk. |
| `auth.rs` | Single-admin password auth: Argon2id password hashing, SHA-256 session-token hashing (deliberately a different, fast hash — see AUTHENTICATION.md). |
| `oidc.rs` | Authentik login via native OIDC (Authorization Code + PKCE), using the `openidconnect` crate. |
| `plex_auth.rs` | "Sign in with Plex" — the real PIN-based OAuth-style flow, allowlisted to one linked account (Plex.tv is a public identity provider, unlike a self-hosted Authentik instance). |
| `backup.rs` | Settings backups: JSON snapshots of settings/mappings/task schedules (not the index), retention of 14, strict file-name validation, all-or-nothing restore. |
| `spa.rs` | Serves the single-page app: `index.html` cached with the `URL_BASE` injected (so one build works at the root or a sub-path) and re-read whenever the file changes on disk, plus the trailing-slash redirect for the bare base. |
| `logs.rs` | The in-app log viewer: installs the tracing subscriber (stdout + a 2,000-entry in-memory ring buffer + a reloadable filter), scrubs credentials from stored messages, and applies the log level live. |
| `health.rs` | Health checks (tools, media server reachability, unmapped libraries, unreadable mappings, writable dirs, disk space, VACUUM headroom, update check, last scan) — run by the Check health task, cached in `AppState::health`, each issue linked to `docs/HEALTH.md`. |
| `tasks.rs` | Scheduled tasks (library scan, database optimize, preview-cache clear, activity-log trim): per-task intervals persisted in `scheduled_task`, a 30s scheduler that runs whatever is due, run-now, and conflict rules (scan and optimize never overlap). |
| `proxies.rs` | Trusted reverse proxies: IP/CIDR parsing and the right-to-left `X-Forwarded-For` walk that resolves the real client IP, honoring the header only from a configured proxy. |
| `ratelimit.rs` | Per-IP login lockout with exponential backoff, keyed on the TCP peer address (never `X-Forwarded-For`). |
| `compact_rebuild.rs` | Background build/load of the research compact dialogue index. Shadow-mode only. |
| `compactindex.rs` | A from-scratch, purpose-built compressed inverted index — a research prototype benchmarked against FTS5, not (yet) the live search path. See its own extensive module doc comment. |
| `diskstats.rs` | Disk-usage helpers for the System tab: directory size/file-count, clearing a directory, and flat file listings (used for the exported-memes browser). |
| `version.rs` | App version + "is there a newer release?" check against GitHub tags. |

## Frontend (`web/`)

Single-page app, no router library — a `view` string in top-level state
picks which section renders. Built with Vite; `npm run build` emits static
files the Rust binary serves directly (`WEB_DIR`, default `/app/web` in the
container).

- `src/main.jsx` — mounts `<App />`.
- `src/App.jsx` — the shell and most tabs. Organized as: the auth gate
  (`App` itself checks `/api/auth/status` and renders a setup/login screen,
  or `AppShell`), then `AppShell` (sidebar + top bar + the seven tabs), then
  each tab's component, then shared sub-components (`ShowDrill`,
  `LibraryGrid`, etc.) reused across tabs rather than duplicated. It is the
  largest file in the project (about 3,500 lines; `Editor` alone is ~565), and
  new features have gone into their own files instead:
  - `SubtitleSearch.jsx` — the interactive subtitle search used by the editor
    and the Wanted tab, plus the System-page status panel.
  - `WantedView.jsx` — the Wanted tab.
  - Pure, unit-tested logic lives beside them with no React in it:
    `subtitles.js`, `wizard.js`, `health.js`, `logs.js`, `theme.js`,
    `update.js`, `cueFind.js`, `format.js`, `base.js`.
- `src/api.js` — a thin `fetch` wrapper. One function per endpoint; a shared
  `json()` helper that throws on non-OK responses and fires a
  `memearr:unauthorized` window event on 401 (the App listens for that and drops
  back to the login screen from anywhere, not just on next reload).
- `src/styles.css` — one stylesheet, CSS custom properties for the color
  tokens (`:root { --accent: ...; }` etc.), no CSS framework.

### Tabs

| Tab | What it is |
|---|---|
| Make | Search (dialogue or title), pick a result or a matching show, trim/caption/export in the `Editor`. The editor also has **Find subtitles online** (`SubtitleSearch.jsx`): pick a language, search the media server's subtitle sources, download one, then index the item to make it searchable. |
| Sequence | Queue multiple clips, export them back-to-back as one file. |
| Library | Browse the whole catalog (movies + shows), filter by text, drill into a show's episodes. |
| Wanted | **Missing**: items with no searchable subtitles whose files are reachable (`WantedView.jsx`); run the same online subtitle search inline, index the item, and it drops off the list. **Requests** (`SubtitleRequests.jsx`): every download requested, whether it has become searchable yet, with Re-index / Open / Remove. |
| Activity | Indexing progress, the persistent event log (with Clear), this session's export jobs. |
| Settings | Plex connection + path mappings, admin password change, Authentik config, Plex-login linking. |
| System | Health, online-subtitle-search status, index/storage stats (with drill-downs), DB maintenance actions, exported-memes browser. |

## The compact dialogue index — what it actually is today

`src/compactindex.rs` is a real, benchmarked, from-scratch inverted index
built during a deep space/speed research investigation — see the file's own
module doc for the full design rationale (candidate-generation +
verification, delta-encoded postings, an optional `block_size` grouping
parameter, Golomb-Rice bit-packing). Measured against real data from this
app's own library, it lands around **9.6% of FTS5's size at `block_size=32`
combined with Rice coding, while running ~3.7x faster per query**.

**It is not live.** `compact_rebuild.rs` builds/persists it in the
background and `AppState.compact` holds the result, but the only thing that
touches it today is a **shadow check** inside the `/api/search` handler: for
every real dialogue search, it also runs the compact index's
candidate-blocks-then-verify logic and compares the result set against
FTS5's own native phrase match, logging any disagreement (to both
`tracing::warn!` and the Activity log) — but always returns FTS5's result to
the user. Promoting it to the actual query path is an explicitly separate,
not-yet-made decision.
