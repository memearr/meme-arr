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
   │      ├─ subtitle_request / scheduled_task        (Wanted requests, task schedules)
   │      └─ sessions / oidc_flows / plex_flows      (auth state)
   │
   ├─► Plex or Jellyfin API (reqwest client) — libraries, items, subtitle search
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
  seeks directly into the original file — instant, lossless. If a file
  isn't reachable on a mounted volume, that item is marked unreachable;
  downloading it through the media server's API is not supported.
- **SQLite + FTS5, not a separate DB server.** Embedded, no extra service,
  millisecond full-text search over tens of millions of subtitle cues, ships
  inside the one container. See `docs/OPERATIONS.md` and the project's deep
  research into whether a from-scratch index could beat FTS5 on size — the
  prototype exists (`compactindex/`) but FTS5 is still what live search runs on.
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

Orientation only: each file's own `//!` comment is the authoritative description.

**Entry and shared state**

| Module | Purpose |
|---|---|
| `main.rs` | Binary entry point. All real logic lives in the library crate (`lib.rs`) so integration tests can reach the same modules. |
| `lib.rs` | Builds `AppState`, starts the background tasks (scheduler, compact-index load/rebuild) and serves the HTTP API and the SPA. `run()` is the whole server entry point. |
| `state.rs` | `AppState` (shared, cloned into every handler) and the in-memory status types for the indexer and export jobs. |
| `config.rs` | Runtime configuration, loaded from environment variables once at startup. |
| `sync.rs` | Mutex and RwLock helpers that survive a panic elsewhere, so one failed request can't poison shared state for good. |

**HTTP API** (`api/`; see [API.md](API.md) for every endpoint)

| Module | Purpose |
|---|---|
| `api/mod.rs` | The router, `AppError` and shared helpers. |
| `api/search.rs` | Libraries, dialogue search, title search and media details. |
| `api/library.rs` | Library browsing (with the libraries and missing-subtitle filters), the Wanted list, stats and reindexing. |
| `api/clips.rs` | Preview clips, single and sequence exports, jobs and downloads. |
| `api/subtitle_api.rs` | Online subtitle search and download (Plex and Jellyfin), the Requests list, diagnosis and re-indexing one item. |
| `api/settings.rs` | Media-server settings, the connection test and the folder browser. |
| `api/session.rs` | Sessions, API keys, login/logout/password, trusted proxies and the `require_auth` middleware. |
| `api/authentik.rs`, `api/plex_login.rs` | Authentik (OIDC) and Plex single sign-on: login, callback and their settings. |
| `api/fonts.rs` | List, upload and delete caption fonts, and resolve the font a caption chose. |
| `api/system.rs` | Health, status, metrics, activity and server logs, storage, backups and scheduled tasks. |

**Storage** (`db/`; the only code that writes SQL against the app's tables)

| Module | Purpose |
|---|---|
| `db/mod.rs` | Schema and migrations, the connection pool. |
| `db/media.rs` | Media rows, subtitle cues, dialogue search and the library/Wanted/browse queries. |
| `db/settings.rs`, `db/sessions.rs`, `db/requests.rs`, `db/tasks.rs`, `db/logs.rs`, `db/maintenance.rs` | Key-value settings and path mappings; the admin password, sessions and login flows; subtitle requests; task schedules; the Activity log; database statistics, checkpoint and vacuum. |

**Indexing and search**

| Module | Purpose |
|---|---|
| `index/` | The background library indexer: walks every movie and episode, resolves its file, picks the best subtitle (sidecar, embedded track, or the one the server keeps), parses the cues and stores them. `item.rs` indexes one item, `sidecar.rs` finds sidecar files and fingerprints changes. |
| `subtitles.rs` | Parses SRT/VTT into timed cues, and builds a trimmed SRT for a clip window for burning at export time. |
| `remote_sub.rs` | Subtitles the media server keeps for itself (Plex stores downloaded ones in its own database). |
| `querysyntax.rs` | The search-box operators (`show:`, `ep:`, `movie:`, `library:`, `season:`, a leading `-`): a small pure parser that splits a query into text, filters and excluded words. `db::Scope` turns the filters into SQL. |
| `diagnose.rs` | The read-only "why is this subtitle still Waiting?" report behind the Diagnose button. |
| `compactindex/`, `compact_rebuild.rs` | A from-scratch compressed inverted index, benchmarked against FTS5 and running in shadow mode only (see below). |

**Media server and files**

| Module | Purpose |
|---|---|
| `mediaserver.rs` | The seam between the app and the server: `MediaServer` (Plex or Jellyfin) and the shared `Section` / `Item` types. Everything downstream is server-agnostic. |
| `plex.rs`, `jellyfin.rs` | The minimal clients for each server: libraries, items with their on-disk path, subtitle streams and search. |
| `fonts.rs` | Caption fonts: the built-in ones and the uploaded ones in `DATA_DIR/fonts`, how an upload is checked, and reading a font's family name. |
| `paths.rs` | Translates the paths the server reports into paths reachable from this container. |

**Exports and previews**

| Module | Purpose |
|---|---|
| `ffmpeg.rs` | Thin orchestration over `ffmpeg`/`ffprobe`. Argument building is pure and unit-tested; running the process is not. |
| `export/` | Turns a clip window or a sequence into WebP/MP4/WebM/GIF. `request.rs` holds the request types, limits and naming; `filters.rs` the pure filtergraph and metadata-stripping arguments; `run.rs` runs ffmpeg. |
| `preview_cache.rs` | Renders previews safely: to a temporary file, checked for completeness before it becomes a cache entry. |
| `diskstats.rs` | Disk-usage helpers for the System tab. |

**Authentication and operations**

| Module | Purpose |
|---|---|
| `auth.rs` | Single-admin password auth: Argon2id password hashing and SHA-256 session-token hashing. |
| `oidc.rs`, `plex_auth.rs` | The Authentik OIDC client and the Plex PIN login flow. |
| `ratelimit.rs`, `proxies.rs` | Per-address login lockout, and trusted-proxy handling (the right-to-left `X-Forwarded-For` walk). |
| `backup.rs` | Settings backups: JSON snapshots, retention of 14, strict file-name validation, all-or-nothing restore. |
| `health.rs` | The health checks behind System → Health, each linked to [HEALTH.md](HEALTH.md). |
| `tasks.rs` | Scheduled tasks and their scheduler. |
| `logs.rs` | The in-app log viewer: tracing subscriber, in-memory ring buffer, credential scrubbing, live level changes. |
| `spa.rs` | Serves the single-page app with `URL_BASE` injected. |
| `version.rs` | The app version and the "newer release?" check against GitHub tags. |

## Frontend (`web/`)

Single-page app, no router library: a `view` string in top-level state picks which section
renders. Built with Vite; `npm run build` emits static files the Rust binary serves directly
(`WEB_DIR`, default `/app/web` in the container).

- `web/src/main.jsx` mounts `<App />`; `web/src/App.jsx` is the shell: the auth gate (setup or login
  screen), the sidebar and top bar, and which tab is showing.
- **One component file per area**: `Editor.jsx` (trim, caption, export), `SequenceView.jsx`,
  `Library.jsx`, `WantedView.jsx`, `SubtitleRequests.jsx`, `SubtitleSearch.jsx`,
  `SubtitleDiagnosis.jsx`, `ActivityView.jsx`, `Settings.jsx`, `SystemView.jsx` (with
  `TasksPanel.jsx`, `BackupsPanel.jsx`, `LogsPanel.jsx`), `SetupWizard.jsx`, `AuthScreen.jsx`,
  `Sidebar.jsx`, `TopBar.jsx`, `SearchScope.jsx` and `LibraryFilter.jsx` (the filter chips and
  libraries picker), and `FontsPanel.jsx` (Settings → Caption fonts).
- **Pure, unit-tested logic** sits beside them with no React in it: `searchScope.js`,
  `searchTips.js`, `libraryFilter.js`, `wantedFocus.js`, `subtitles.js`, `clipSettings.js`,
  `wizard.js`, `fonts.js`, `health.js`, `logs.js`, `theme.js`, `update.js`, `cueFind.js`, `format.js`,
  `base.js`.
- `web/src/api.js` is a thin `fetch` wrapper: one function per endpoint, and a shared `json()`
  helper that throws on non-OK responses and fires a `memearr:unauthorized` window event on a
  401 (the app listens for that and drops back to the login screen from anywhere).
- `web/src/styles.css` is one stylesheet with CSS custom properties for the colour tokens and no
  CSS framework.

### Tabs

| Tab | What it is |
|---|---|
| Make | Search (dialogue or title, with filter chips and operators), pick a result or a matching show, then trim, caption and export in the `Editor`. The editor also has **Find subtitles online**: pick a language, search the media server's subtitle sources, download one, then index the item to make it searchable. |
| Sequence | Queue multiple clips and export them back to back as one file. |
| Library | Browse the whole catalog (movies and shows), filtered by library, by missing or fully indexed subtitles, and by text; drill into a show's episodes. |
| Wanted | **Missing**: items with no searchable subtitles whose files are reachable; run the online subtitle search inline, index the item, and it drops off the list. **Requests**: every download requested, whether it has become searchable yet, with Re-index / Diagnose / Open / Remove. |
| Activity | Indexing progress, the persistent event log, this session's export jobs. |
| Settings | Media-server connection, path mappings, admin password, Authentik, Plex login, API key, trusted proxies. |
| System | Health, online-subtitle-search status, index and storage stats, tasks, backups, logs, the exported-memes browser. |

## The compact dialogue index: what it is today

`src/compactindex/` is a real, benchmarked, from-scratch inverted index
built during research into index size and speed — see the file's own
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
