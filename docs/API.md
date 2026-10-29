# API reference

All endpoints are relative to the app's own origin (one server, one port —
no CORS to think about in production). JSON in, JSON out, except where noted
(binary media, Prometheus text, file downloads).

**Auth**: every route is session-gated **except** the ones marked 🔓 below, and
except the few read-only endpoints an [API key](#api-key) may `GET`
(`/healthz`, `/status`, `/metrics`, `/api/version`, and everything under
`/api/auth/*`). See [AUTHENTICATION.md](AUTHENTICATION.md) for how sessions
work. An ungated request to a protected route gets `401
{"error": "authentication required"}`.

**Errors**: most handlers return `anyhow::Error` wrapped into a `500
{"error": "<message>"}` by a shared `AppError` type. Handlers that need a
more specific status build it directly — `400` for bad input, `404` for "not
found", `409` for a conflicting state (e.g. vacuuming while an index pass is
running). Check each endpoint below for its specific non-500 cases.

- **Unknown API paths** (anything under `/api` that isn't an endpoint below, including
  a stray trailing slash) answer `404 {"error": "no such API endpoint"}`, not the web
  page. An unauthenticated caller still gets the `401` first, so the paths that exist
  aren't revealed. A known path with the wrong method is `405`.
- **No media server configured** is `409` with the explanation (e.g. `GET /api/libraries`),
  not `500`: it's a state to fix in Settings, not a server fault.
- **Range requests.** `GET /api/download/{filename}` and `GET /api/preview/{rating_key}`
  honour a single `Range: bytes=` (explicit `a-b`, open-ended `a-`, suffix `-n`) with
  `206` and `Content-Range`, and answer an unsatisfiable range with `416` and
  `Content-Range: bytes */<length>`. A multi-range or malformed `Range` is ignored and
  the whole file is returned with `200` (RFC 9110 allows that).

---

## Health, status, metrics

### `GET /healthz` 🔓
Bare liveness check for things like Docker's `HEALTHCHECK`.
```json
{ "ok": true }
```

### `GET /status` 🔓
Slightly richer liveness check for uptime monitors/status pages. No DB
query — in-memory only, stays fast even if SQLite were stuck.
```json
{ "status": "ok", "version": "v0.11.0", "uptime_seconds": 1234, "index_running": false }
```

### `GET /metrics` 🔓
Prometheus text exposition format. See [OBSERVABILITY.md](OBSERVABILITY.md)
for the full metric list and an example Grafana dashboard
(`grafana/meme-arr-dashboard.json`).

### `GET /api/version` 🔓
Current build version and whether a newer GitHub release tag exists.
`checked: false` means the GitHub lookup itself failed (network, or — the
common case for this repo — a private repo with no/invalid `GITHUB_TOKEN`,
which GitHub reports as a 404 rather than 401/403 to avoid leaking the
repo's existence to an unauthenticated caller); `error` carries the reason
in that case.
```json
{
  "current": "v0.25.0", "latest": "v0.25.0", "update_available": false,
  "checked": true, "error": null, "repo_url": "...", "releases_url": "..."
}
```

---

## Authentication

See [AUTHENTICATION.md](AUTHENTICATION.md) for the full design (password +
Authentik OIDC + Plex PIN flow, session model, recovery).

### `GET /api/auth/status` 🔓
What the login screen needs to decide what to show.
```json
{ "setup_required": false, "authenticated": true, "authentik_available": true, "plex_login_available": false }
```

### `POST /api/auth/setup` 🔓
First-run only — refuses once a password already exists (`400`, use
`change-password` instead). Body: `{ "password": "..." }` (min 8 chars). On
success, also logs the new session in (same response shape as `login`).

### `POST /api/auth/login` 🔓
Body: `{ "password": "..." }`. `401` on wrong password. On success, sets the
session cookie and returns `{ "ok": true }`.

### `POST /api/auth/logout`
Clears the session (both server-side and the cookie). `{ "ok": true }`.

### `POST /api/auth/change-password`
Body: `{ "current_password": "...", "new_password": "..." }` (new, min 8
chars). `400` if current password is wrong. Current session stays valid.

### `GET /api/auth/oidc/login` 🔓
Redirects (`303`) the browser to Authentik. `400` if Authentik isn't
configured, or if Authentik itself can't be reached (discovery failure).

### `GET /api/auth/oidc/callback` 🔓
Authentik redirects here after approval/denial. Query: `code`, `state` (or
`error` on denial). On success: sets the session cookie, redirects to `/`.
`400` on a missing/stale state, a provider error, or ID-token verification
failure.

### `GET /api/auth/plex/login` 🔓
Redirects to Plex's own auth page (`app.plex.tv/auth`) to start the PIN
flow. `400` if the Plex PIN API can't be reached.

### `GET /api/auth/plex/callback` 🔓
Plex redirects here via its `forwardUrl`. Query: `pin_id`. Checks the PIN,
resolves the account, and — **only if that account was previously
linked** — grants a session and redirects to `/`. `400` otherwise (not
completed/denied, or an unlinked account).

### `GET /api/settings/authentik` / `PUT` / `DELETE`
Configure Authentik OIDC. `GET` never returns the stored secret, only
whether one is set:
```json
{ "issuer_url": "https://auth.example.com/application/o/...", "client_id": "...", "secret_set": true, "configured": true }
```
`PUT` body: `{ "issuer_url": "...", "client_id": "...", "client_secret": "..." }`
— `client_secret` optional (omit/blank to keep the existing one, same
convention as the Plex token field).

### `GET /api/settings/plex-login` / `DELETE`
```json
{ "linked": true, "username": "plexuser" }
```
`DELETE` unlinks (`{ "ok": true }`).

### `GET /api/settings/plex-login/link`
Authenticated-only (unlike the other Plex-login routes above) — starts the
same PIN flow but with `purpose=link`, so a successful completion updates
the linked-account allowlist instead of granting a session.

---

## Search

### Search operators (`q`, for both searches below)
`q` understands web-search-style operators. Whatever is left after them is the
**text** to search for: dialogue in `/api/search`, titles in `/api/search/title`.

| Operator | Meaning |
|---|---|
| `show:"Breaking Bad"` | the show's name contains the value (any case) |
| `ep:pilot` / `episode:pilot` | an **episode** whose title contains the value |
| `movie:dune` | a **movie** whose title contains the value |
| `ep:` / `movie:` (no value) | only episodes / only movies |
| `library:Anime` / `lib:Anime` | in the library with exactly this name (any case) |
| `season:2` | the season number |
| `-` in front of any of them | the opposite: `-show:friends`, `-ep:"the pilot"`, `-movie:` |
| `-word` / `-"a phrase"` | leave out dialogue lines (Dialogue) or titles and shows (Title) containing it |

Rules: values may be `"quoted"` to hold spaces (an unclosed quote runs to the end);
several operators combine with AND, and with the `library=` picker parameter; a
`key:` that isn't an operator (`note: hello`, `12:30`) is plain text; `season:` needs
a number or it is plain text; a leading `-` only counts when something follows it
directly (`well - hello` is plain text); a quoted phrase is always text, so
`"show:me the money"` and `"-5 degrees"` search literally. `-show:x` keeps items
that have no show (movies). The parser is `src/querysyntax.rs`.

Responses add `"filters": ["show: Breaking Bad", "not episode: pilot",
"without “walter”"]` (how the query was read, for display). In Dialogue mode, a
query with operators but **no dialogue text** (`show:"Breaking Bad"`) returns the
matching *items* (title-search shaped, no `text`) with `"listing": true`, instead of
nothing. Exclusions alone can't be matched against dialogue, so they need text.

### `GET /api/search?q=<text>&limit=<n>&offset=<n>&library=<name>...`
Dialogue search (SQLite FTS5 phrase match over subtitle text), best match
first. Also returns any show names matching `q`, so a dialogue search that
finds no dialogue still surfaces a matching show as an option.

Results come in **pages**: `limit` is the page size (default 50, max 500) and
`offset` how many matches to skip. `has_more` says whether more matches follow
this page (found by asking for one extra row, so there is no slow count over
the whole index); the UI shows "Showing the first 50 matches" and a **Show more**
button. `library` limits the search to some libraries and may be repeated
(`&library=TV&library=Movies`); leave it out, or leave it blank, for all
libraries. `shows` is only filled on the first page (`offset=0`).
```json
{
  "results": [
    { "media_id": 1, "rating_key": "...", "kind": "episode", "title": "Ozymandias",
      "show": "Breaking Bad", "season": 5, "episode": 14,
      "start_ms": 12345, "end_ms": 14200, "text": "..." }
  ],
  "shows": ["Breaking Bad"],
  "has_more": true,
  "offset": 0
}
```

### `GET /api/search/title?q=<text>&limit=<n>&offset=<n>&library=<name>...`
Each result also has `has_subtitles` (whether any subtitle lines are indexed for it);
the UI uses `false` to offer a "no subtitles" link to the Wanted tab.
Title search (`LIKE` match against movie/episode/show title). Same `shows`,
paging (`limit`, `offset`, `has_more`), `library` filter and operators as above.
Operators matter most here for narrowing by show and episode title, e.g.
`pilot show:friends` or `ep:"the one with" -show:office`.
```json
{
  "results": [
    { "rating_key": "...", "kind": "movie", "title": "Dune", "show": null,
      "season": null, "episode": null, "year": 2021, "duration_ms": 7800000 }
  ],
  "shows": [],
  "has_more": false,
  "offset": 0
}
```

---

## Media, preview, export

### `GET /api/media/:rating_key`
`200` with the `Media` row, or `404` if not indexed.
```json
{ "id": 1, "rating_key": "...", "kind": "episode", "title": "...", "show": "...",
  "season": 5, "episode": 14, "year": 2013, "file_path": "...", "part_key": null,
  "duration_ms": 2760000, "access_mode": "direct" }
```

### `GET /api/media/:rating_key/subtitles?start=<ms>&end=<ms>`
Indexed cues overlapping a window. `404` if the media isn't indexed.
```json
{ "cues": [{ "start_ms": 12000, "end_ms": 14000, "text": "..." }] }
```

### `GET /api/preview/:rating_key?start=&end=&width=&fps=&caption_mode=&top=&bottom=&font_size=&offset_ms=&cues_json=`
Returns `video/mp4` bytes — a short, fast-seeked, browser-safe render of
just that window, with the same caption treatment the real export would
use. Cached on disk keyed by every parameter that affects the rendered
frames. `caption_mode` is `"none"` (default) | `"source"` | `"meme"`.
`cues_json` (URL-encoded JSON array of `{start_ms,end_ms,text}`) lets the
editor preview its own edited subtitle lines instead of re-fetching the
indexed ones.

### `POST /api/export`
Starts a background export job, returns immediately. `400` if the window is
invalid or over 60s, or the source isn't on a mounted volume. `404` if the
media isn't indexed.

Request (`ExportRequest`):
```json
{
  "rating_key": "...", "start_ms": 12000, "end_ms": 18000, "format": "gif",
  "width": 480, "fps": 15, "quality": 80,
  "caption": { "mode": "source", "cues": null },
  "subtitle_offset_ms": 0, "audio": false, "credit_source": true, "filename": "my-meme"
}
```
- `format`: `"gif" | "webp" | "mp4" | "webm"`.
- `caption`: `null` | `{"mode":"none"}` | `{"mode":"source","cues":null-or-[...]}` |
  `{"mode":"meme","top":"...","bottom":"...","font_size":48}`. `cues: null`
  under `"source"` means "burn whatever's indexed"; a real array means "burn
  exactly this instead" (the editor's user-edited lines).
- `quality` drives libwebp's `-q:v` and x264/VP9 CRF for webp/mp4/webm, and
  (as of the gifsicle integration) a `gifsicle --lossy=N` post-process pass
  for GIF — ffmpeg's own palette pipeline for GIF has no quality lever of its
  own. A missing/misconfigured `gifsicle` binary skips that pass (logged,
  non-fatal) rather than failing the export; see `docs/DEPLOYMENT.md`'s
  `GIFSICLE_BIN`.
- `audio`: keep the source audio track, encoded as Opus (`libopus`, 48kHz
  stereo, 128kbps). Only `mp4`/`webm` can carry it — GIF and WebP have no
  audio channel in their format spec. `400` if `audio: true` is combined
  with `format: "gif"` or `"webp"` (`export::validate_audio_format`). A
  source clip with no audio track of its own just exports silent; no error.
- **Metadata.** An export never carries the source file's metadata: no global,
  per-stream or chapter tags (release name, comments, creation time, languages, track
  names), no subtitle or data tracks (captions are burned into the picture), and no
  ffmpeg/encoder version strings (`export::privacy_args`). The one tag that may be
  written is `title`: the movie (`Paper Moon Heist (2019)`) or show/episode
  (`The Lantern Cove - S02E03 - Tideline`) the clip came from, built from the
  library's metadata, not the file's. `credit_source` (default `true`) turns that
  title off; GIF and WebP have no place for a title and never get one.

Response: `{ "job_id": "..." }` — poll `GET /api/jobs/:id` for progress.

### `POST /api/sequence/export`
Same idea, multiple clips back to back (cap: 20 clips / 3 minutes total,
validated before touching Plex/ffmpeg at all).
```json
{
  "clips": [
    { "rating_key": "...", "start_ms": 0, "end_ms": 3000, "caption": null, "subtitle_offset_ms": 0 }
  ],
  "format": "mp4", "width": 480, "fps": 15, "quality": 80, "audio": false, "credit_source": true,
  "filename": "sequence"
}
```
`audio` is sequence-wide, same constraint as above — every clip in the
sequence is either exported with sound or without, there's no per-clip
toggle. A clip whose own source has no audio track gets silence substituted
for its segment (so the concatenated result never has gaps or an
inconsistent stream layout); see `src/export/`'s
`sequence_clip_args_with_audio` for the mechanics.

The same metadata rules apply, including to the intermediate files, so nothing from any
clip's source survives into the result. With `credit_source`, the `title` lists each
distinct source once, joined with ` / ` (cut short past 200 characters).

Response: same `{ "job_id": "..." }` shape.

### `GET /api/jobs/:id`
```json
{ "id": "...", "status": "done", "format": "gif", "filename": "my-meme.gif",
  "audio": false, "download_path": "/api/download/my-meme.gif", "error": null }
```
`status`: `"queued" | "running" | "done" | "failed"`. Jobs are in-memory,
scoped to the running process — they don't survive a restart (that's what
`GET /api/exports`, below, is for). `audio` echoes the request's `audio`
flag — the frontend uses it to decide whether the result preview's `<video>`
should play unmuted.

### `GET /api/download/:filename`
Streams a finished export from `OUTPUT_DIR` with a
`Content-Disposition: attachment` header. `400` on a filename containing
`/`, `\`, or `..`; `404` if it doesn't exist.

---

## Indexing

### `GET /api/index/status`
```json
{
  "status": { "running": false, "total": 0, "done": 12508, "skipped": 0,
              "errors": 0, "current": null, "finished_at": 1792700000, "last_error": null },
  "stats": { "media_count": 12508, "cue_count": 24531902 }
}
```

### `POST /api/index/reindex`
Runs the **Library scan** task now (same as `POST /api/tasks/library_scan/run`,
and it updates that task's last-run record). `202 { "started": true }`, or
`409 { "error": "..." }` if a scan is already running or the database
optimize task is (they never overlap).

---

## Logs

See `src/logs.rs`. A ring buffer of the last 2,000 `tracing` events (in memory,
gone on restart; everything also still goes to stdout). **Secrets are scrubbed
before storing** — `X-Plex-Token=…`, `token=` / `password=` / `secret=` /
`api_key=` values, and `Bearer …` become `***`. Session-only (not reachable with
an API key).

### `GET /api/logs?level=info&q=<text>&after=<id>&limit=300`
Oldest first. `level` is the minimum severity (`error` `warn` `info` `debug`
`trace`; default `info`), `q` a case-insensitive text/target filter, `after` an
entry id (only newer ones — how the UI polls), `limit` 1–2000.
```json
{ "level": "info",
  "entries": [{ "id": 412, "ts_ms": 1792900949123, "level": "warn",
                "target": "meme_arr::api::session", "message": "failed login from 192.168.0.9" }] }
```

### `GET /api/settings/log-level` / `PUT`
`{ "level": "info", "options": ["error", "warn", "info", "debug", "trace"] }`.
`PUT { "level": "debug" }` changes it **live** (no restart) and remembers it;
`400` for an unknown level. For `debug`/`trace` only this app's own logging
goes up — dependencies (hyper, reqwest…) stay at `info`, or the output would
drown. A saved level beats `RUST_LOG`; before one is saved, `GET` reports
`"env"` when `RUST_LOG` set the filter.

---

## Setup wizard

### `GET /api/system/browse?path=<abs path>`
Subfolders of a path inside the container, for the first-run wizard's mapping
suggestions: `{ "path": "/media", "dirs": ["/media/movies", "/media/tv"] }`.
Directories only (no file names), at most 500, sorted. With no `path`, the
first of `/media`, `/mnt`, `/` that exists. `400` for a relative or unreadable
path. Session-only (not available to an API key).

The wizard itself is frontend-only, built from existing endpoints:
`POST /api/settings/test` → `PUT /api/settings` → `POST /api/health/check` →
`POST /api/index/reindex`.

---

## Trusted proxies

### `GET /api/settings/trusted-proxies` / `PUT`
`{ "trusted_proxies": "172.18.0.0/16, 192.168.0.5" }` — comma/space
separated IPs or CIDRs (IPv4 and IPv6). `PUT` validates: `400` with the
offending entry if any is malformed; empty clears it. See
[AUTHENTICATION.md](AUTHENTICATION.md#password-auth-authrs) for how it's used.

---

## API key

For dashboards and scripts (e.g. a Homepage widget): send `X-Api-Key: memearr_…`.
**Read-only and allowlisted** (`auth::api_key_may_access`): `GET` of
`/api/health`, `/api/tasks`, `/api/index/status`, `/api/libraries`,
`/api/libraries/stats`, `/api/system/storage` — nothing else. Never settings
(they hold the Plex token), backups, exports, or any write. Header only: a
key in a query string is ignored, since query strings land in access logs.
Only the SHA-256 hash is stored; the plaintext is shown once, at generation.
A key can't manage keys.

### `GET /api/settings/api-key`
`{ "enabled": true }`

### `POST /api/settings/api-key`
Generates (or replaces) the key: `{ "api_key": "memearr_…" }` — the only time it's
returned. The old key stops working immediately.

### `DELETE /api/settings/api-key`
Revokes it.

---

## Backups

Settings backups (`src/backup.rs`): a JSON file of the settings table (Plex
and Jellyfin URL/credentials, Authentik, the admin password hash, …), path mappings, and task
schedules. **The subtitle index is not included** — a library scan rebuilds
it. Files live in `DATA_DIR/backups/` and **contain secrets**.

### `GET /api/backups`
`{ "backups": [{ "name": "settings-20261025-035235.json", "bytes": 1078, "modified_at": 1792900355 }], "keep": 14 }`, newest first.

### `POST /api/backups`
Runs the **Back up settings** task now and waits: `{ "name": "..." }`. The
newest 14 are kept; older ones are deleted. Also runs daily on a schedule.

### `GET /api/backups/:name`
Downloads one. Only names this app generates (`settings-YYYYMMDD-HHMMSS.json`)
resolve; anything else is `404`, so it can't read other files.

### `POST /api/backups/restore`
Body: the backup file's JSON. Replaces settings, mappings, and schedules in
one transaction (a rejected file changes nothing): `400` for invalid JSON or
an unsupported format version. Restoring replaces the admin password with the
backup's, and triggers a health re-check; an Activity log entry is written.

---

## Health checks

See [HEALTH.md](HEALTH.md) for every check and its fix.

### `GET /api/health`
The latest cached report (the **Check health** task's result) — cheap enough
to poll; the sidebar badge does, every 60s.
```json
{ "checked_at": 1792700000, "running": false,
  "issues": [{ "kind": "update_check_failed", "level": "warning",
               "message": "Can't check for updates: GitHub returned 404 Not Found",
               "fix": "For a private repo, set GITHUB_TOKEN ...",
               "docs_url": "https://github.com/<repo>/blob/main/docs/HEALTH.md#update_check_failed" }] }
```
`checked_at` is `null` until the first pass after startup finishes. `level`:
`error` | `warning` | `info`; issues are sorted most severe first.

### `POST /api/health/check`
Re-runs the checks now and returns the fresh report (same shape). If a check
is already running (saving settings starts one), it waits for that one (up to
30s) rather than failing, so the report always reflects what was just changed. Saving Plex settings or path mappings
(`PUT`/`DELETE /api/settings`) also triggers a re-check in the background.

---

## Scheduled tasks

See `src/tasks.rs`. Each task runs on its own interval; schedules and last
outcomes are persisted (`scheduled_task` table), so a restart only runs
what's actually due.

### `GET /api/tasks`
```json
[{ "id": "library_scan", "label": "Library scan", "description": "...",
   "enabled": true, "interval_secs": 86400,
   "min_interval_secs": 3600, "max_interval_secs": 7776000,
   "running": false, "last_started": 1792700000, "last_finished": 1792700410,
   "last_duration_ms": 410000, "last_error": null,
   "last_summary": "212 indexed, 12296 unchanged, 0 errors",
   "next_run": 1792786400 }]
```
Task ids: `backup_settings` (daily), `check_health` (every 6h, min 15 min; also runs on every startup
since its result lives in memory), `library_scan` (default every 24h), `optimize_database` (7 days,
min 24h), `clear_preview_cache` (7 days), `trim_activity_log` (daily; drops
entries older than 30 days), `recheck_requests` (hourly, min 15 min; re-indexes the items of
subtitle requests that are still Waiting and under 3 days old, up to 50 per run, the same work
as the Re-index button; skipped while a library scan runs; only items that gain subtitle lines are
written to the Activity log). `next_run` is `null` while disabled, and `now`
when overdue. A skipped run (e.g. "an export is in progress") is recorded in
`last_summary` as `Skipped — <reason>`, not as an error; a failed run sets
`last_error` and also writes an Error entry to the Activity log.

### `POST /api/tasks/:id/run`
Starts it in the background: `202 { "started": true }`; `409` with the reason
if it's already running or blocked by a conflicting task (library scan and
database optimize never run at the same time); `404` for an unknown id.

### `PUT /api/tasks/:id`
```json
{ "enabled": true, "interval_secs": 43200 }
```
`400` if the interval is outside the task's `min_interval_secs`..
`max_interval_secs`. The next run is recomputed from the last start.

---

## Activity log

### `GET /api/activity/log?limit=<n>` (default 200, max 500)
Persisted in SQLite (`activity_log` table), survives restarts. Newest first.
```json
{ "entries": [{ "ts": 1792700000, "level": "info", "message": "Indexing finished: 12508 indexed, 0 skipped, 0 errors" }] }
```

### `DELETE /api/activity/log`
Clears it. `{ "ok": true }`.

---

## System / storage / maintenance

### `GET /api/system/storage`
```json
{
  "database": { "db_file_bytes": 5800000000, "wal_bytes": 12000000, "total_bytes": 5812000000,
                "media_rows": 12508, "cue_rows": 24531902,
                "subtitle_text_bytes": 900000000, "media_text_bytes": 2000000,
                "index_overhead_bytes": 4910000000 },
  "preview_cache": { "bytes": 45000000, "files": 12 },
  "exports": { "bytes": 230000000, "files": 8 }
}
```

### `GET /api/exports`
Lists files actually sitting in `OUTPUT_DIR`, newest first — what's really
on disk, independent of the in-memory job list above.
```json
{ "files": [{ "name": "my-meme.gif", "bytes": 4200000, "modified_at": 1792700000 }] }
```
Re-download any of them via `GET /api/download/:filename`.

### `POST /api/system/vacuum`
Deep-clean: FTS5 `optimize` + `VACUUM` — runs the **Optimize database** task
and waits for it. `409` if that task is already running or a library scan is
(they'd contend for the same database file).
```json
{ "before_bytes": 5812000000, "after_bytes": 5450000000, "saved_bytes": 362000000, "duration_ms": 48000 }
```

### `POST /api/system/clear-preview-cache`
Runs the **Clear preview cache** task and waits for it.
```json
{ "bytes": 45000000, "files": 12 }
```
`409 { "error": "skipped: an export is in progress" }` while any export is
queued or running — exports write their temp files to the same directory, so
clearing it mid-export would break them.

### `POST /api/system/clear-exports`
Deletes every file in `OUTPUT_DIR` — these are real deliverables, not a
cache; the frontend confirms with the user before calling this.
```json
{ "bytes": 230000000, "files": 8 }
```

---

## Library / media server

### `GET /api/libraries`
Live from the media server (not the local DB) — every library section.
```json
{ "libraries": [{ "key": "1", "type": "show", "title": "TV Shows", "locations": ["/volume1/media/tv"] }] }
```

### `GET /api/libraries/stats`
Per-library/kind indexed-vs-total counts, from the local DB.
```json
{ "libraries": [{ "library": "TV", "kind": "episode", "total": 12000, "indexed": 11800,
                  "no_subtitles": 150, "unreachable": 50, "cue_count": 24000000 }] }
```

### `GET /api/libraries/missing?library=<name>`
Items in that library with no subtitle index, and why.
```json
{ "items": [{ "rating_key": "...", "title": "...", "show": "...", "season": 1, "episode": 1,
              "file_path": "...", "reason": "no_subtitles" }] }
```
`reason`: `"no_subtitles" | "unreachable"`.

### `GET /api/library/browse?q=&offset=&limit=&library=&status=` (limit default 60, max 200)
Catalog browse — movies individually, episodes grouped by show. `library`
(optional, repeatable) limits it to those Plex libraries instead of the whole
catalog. `status` is `all` (default), `missing` (movies, or shows with at least
one episode, lacking indexed subtitles) or `indexed` (nothing missing).
```json
{
  "entries": [
    { "kind": "show", "name": "Breaking Bad", "show": "Breaking Bad", "rating_key": null,
      "count": 62, "indexed": 60, "year": 2008 },
    { "kind": "movie", "name": "Dune", "show": null, "rating_key": "...", "count": 1, "indexed": 1, "year": 2021 }
  ],
  "total": 2
}
```

### `GET /api/library/episodes?show=<name>`
```json
{ "episodes": [{ "rating_key": "...", "title": "Pilot", "show": "Breaking Bad",
                 "season": 1, "episode": 1, "indexed": true }] }
```

---

## Settings (media server connection)

The app indexes from **Plex or Jellyfin** (`media_server`). Both servers' credentials
are stored, so switching back and forth doesn't lose them; secrets (Plex
token, Jellyfin API key) are never returned — only `token_set` /
`jellyfin_key_set`.

### `GET /api/settings`
```json
{ "media_server": "plex", "plex_url": "http://192.168.1.10:32400", "token_set": true,
  "jellyfin_url": null, "jellyfin_key_set": false, "configured": true,
  "save_fetched_subtitles": false,
  "mappings": [{ "plex_prefix": "/volume1/media", "container_path": "/media/drive1", "label": "Movies" }] }
```

### `PUT /api/settings`
Body (all optional — only present fields are updated):
```json
{ "media_server": "plex" | "jellyfin", "plex_url": "...", "plex_token": "...",
  "jellyfin_url": "...", "jellyfin_api_key": "...",
  "save_fetched_subtitles": false,
  "mappings": [{ "plex_prefix": "...", "container_path": "...", "label": "..." }] }
```
`plex_prefix` is the *media server's* path prefix (Plex or Jellyfin); the name predates
Jellyfin support and is kept so the API, saved settings and backups stay compatible.
`configured` reflects the *selected* server. An empty/absent `plex_token` /
`jellyfin_api_key` keeps the saved one (same pattern as the
Authentik client secret). Returns the updated settings (same shape as
`GET`).

`save_fetched_subtitles` (default `false`, Plex only): when a re-index reads a subtitle
Plex keeps in its database (see `POST /api/media/{id}/reindex`), also write it as a `.srt`
next to the video, named after it (`Show S01E02.en.srt`, `.en.forced.srt` for a forced track;
no language tag if the language is unknown). The media folder must be mounted writable. A file
that already has that name is never overwritten, and a failed write (a read-only mount) is a
warning in the Activity log; the subtitle is indexed either way. Plex may then show the
subtitle twice (its own copy and the file) until one is removed. Cleared by `DELETE /api/settings`.

### `DELETE /api/settings`
Disconnects the media server entirely — clears the server choice, both servers'
URLs/credentials, and all mappings.

### `POST /api/settings/test`
Body: `media_server` plus that server's URL/secret (optional — tests the
saved values if omitted). Doesn't persist anything. For Jellyfin the check is
`GET /System/Info`, which needs authorization, so a wrong API key fails here
rather than later.
```json
{ "ok": true, "server_name": null, "libraries": [{ "title": "Movies", "type": "movie", "locations": ["/data/movies"] }] }
```
(`server_name` is Jellyfin's server name, `null` for Plex.)
or `{ "ok": false, "error": "..." }`.

---

## Online subtitle search (Plex and Jellyfin)

Works with whichever media server is selected.

* **Plex** uses its built-in search (OpenSubtitles.com-backed). Plex's
  endpoints for this are not officially documented; they follow the ones
  python-plexapi uses. Needs Plex Media Server **1.13.8.5395 or newer**,
  checked against `GET /identity` ([softver](https://github.com/memearr/softver)
  parses the version). A version that can't be read is attempted anyway, not
  refused.
* **Jellyfin** uses `GET/POST /Items/{id}/RemoteSearch/Subtitles/…` (in its
  published OpenAPI spec). Search only returns results if a **subtitle provider
  plugin** (e.g. OpenSubtitles) is installed and configured; Jellyfin has no
  endpoint to list providers, so an empty result comes back with a `note`
  instead of an error. There is no documented minimum Jellyfin version, so the
  version is reported but not gated on. The API key's user needs the
  `SubtitleManagement` permission.

`{id}` is the item id from the index: a number for Plex, a GUID for Jellyfin.

### `GET /api/subtitles/capability`
Always `200`. Plex: `{ "supported": true, "server": "plex", "version": "1.41.3.9314-…", "version_checked": true, "minimum_version": "1.13.8.5395" }`.
Jellyfin: `{ "supported": true, "server": "jellyfin", "version": "10.10.3", "version_checked": false, "needs_provider_plugin": true }`.
Or `{ "supported": false, "reason": "…" }` (not configured, unreachable, Plex too old).

### `GET /api/media/{id}/subtitles/search?language=en`
`language` defaults to `en` (letters and `-`, max 8). Jellyfin takes a
three-letter code; common two-letter codes (`en`, `pt-BR`) are mapped for you,
and an unmapped two-letter code is a `400`. Returns
`{ "version": "…", "results": [{ "key", "title", "language", "format", "provider", "hearing_impaired", "forced", … }] }`
plus `note` when Jellyfin returns nothing. Optional fields, present only when
the server gave them:
- `hash_match`: a perfect match for this file (Jellyfin `IsHashMatch`, Plex `perfectMatch`).
- `downloads` (Jellyfin) / `score` (Plex): popularity. python-plexapi documents Plex's `score` as the
  subtitle's download count; that is not confirmed against a real server.
- `extended_title`: a longer description than `title`. For Plex, `title` is its `title` attribute
  (for searched subtitles usually the release name), falling back to the display titles.
- `ai_translated`, `machine_translated` (Jellyfin).
- `details`: every other attribute the server sent, as text, under the server's own names
  (Plex: e.g. `canAutoSync`, `sourceKey`, `languageCode`; Jellyfin: `Author`, `Comment`,
  `CommunityRating`, `FrameRate`, `DateCreated`). Attributes with their own field, and internal
  ids (`id`, `index`, `userID`, …), are left out.
Errors: `400` bad id/language · `404` Jellyfin doesn't know the item · `409`
server not configured · `422` Plex too old · `502` server unreachable, or
Jellyfin refused the key/permission (never 401/403, which would read as "you
are logged out").

### `POST /api/media/{id}/subtitles/download`
Body: `{ "key": "<key from a search result>", "title"?, "provider"?, "language"? }`. The
optional three only describe the request in the Requests list (see below; clipped to 200 /
100 / 16 characters). Plex keys must be plain
server-relative paths; Jellyfin keys are opaque (bounded, no control
characters, percent-encoded into the request). Returns
`202 { "accepted": true }`. The server fetches the subtitle in the background;
then index the item with `POST /api/media/{id}/reindex` below (a library scan
also works, now that change detection includes the sidecar).

### Requests (tracking downloads)
Every accepted download is remembered so the Wanted tab's **Requests** view can follow it
until it's searchable. The most recent 500 are kept.

- `GET /api/subtitles/requests`: `{ "requests": [{ "id", "rating_key", "server", "language",
  "subtitle", "provider", "requested_at", "state", "lines", "item_title", "show", "season",
  "episode", "year", "library" }] }`, newest first. `state` is **derived on read, not stored**:
  `indexed` once the item has subtitle lines and was indexed at or after the request;
  `waiting` otherwise (the server may still be fetching the file, or saved it somewhere this
  app doesn't look); `item_missing` if the item is no longer in the index.
- `DELETE /api/subtitles/requests/{id}`: forget one (`404` if it doesn't exist).
- `DELETE /api/subtitles/requests?finished=true`: forget every `indexed` one and keep the
  waiting ones; `{ "removed": n }`. Without `finished=true` it is a `400`, so a bare DELETE
  can't wipe what's still being waited on.

To act on a request, re-index its item with the endpoint below.

### `GET /api/media/{rating_key}/subtitles/diagnose`
Why hasn't a requested subtitle become searchable? Read-only. Combines what this app can see
with what the media server says and states the likely cause.
`{ "local": {...}, "server": {...}, "findings": [{ "level": "ok|info|warn|error", "text": "..." }] }`
- `local`: the video as the server reports it (`server_path`) and as this app looks for it
  (`mapped_path`, `video_reachable`), `files` (subtitle files in the video's folder that are this
  video's or belong to no video; `matches_video`, `indexable`, `picked`), `other_videos_subtitles`
  (a season folder holds every episode's subtitles; those are only counted), `sidecar` (what the
  indexer would use), `indexed_lines`, and `stored_fingerprint` / `current_fingerprint`
  (`[mtime, size]`; a difference means a re-index will pick up a change).
- `server`: `{ kind, streams, error }`. `streams` is what the server lists for the item (name,
  language, format, `external`, and `path` for Jellyfin / `key` for Plex). `error` is set when
  the server couldn't be asked (10 s timeout), in which case only the local picture is shown.
- `findings` are the plain-words conclusions, e.g. "Jellyfin lists 1 external subtitle(s) ... but
  none is a file beside the video ... most likely stored the downloaded subtitle in its own
  storage, which this app can't read."
`400` for a malformed item id.

### `POST /api/media/{rating_key}/reindex`
Re-index one already-known item without walking the library. Uses the same
indexing as a scan: a sidecar file beside the video, else an embedded text track,
else (**Plex only**) the subtitle Plex keeps itself. Plex stores subtitles it
downloaded or that were uploaded in its own database, not as files, so the item's
external text subtitle is read from `GET /library/streams/{id}` (English first,
never an image-based one) and indexed with its timestamps; nothing is written to
the media folder. An item that has no lines yet is always looked at again, even if
the video is unchanged; one that has lines is skipped unless the video or its
sidecar changed.
`200 { "status": "indexed" | "unchanged", "lines": 1204 }`: `unchanged` means
nothing new was found (the server may still be fetching, or Jellyfin saved the
file somewhere this app can't read). `404` for an unknown item.

With the `save_fetched_subtitles` setting on, the subtitle is also saved as a `.srt` beside
the video, and the item's stored fingerprint then includes that file, so the next scan
doesn't see a change.

A full scan reads Plex's own copy only for items that have a recorded subtitle
request (one API call each), so a scan doesn't make a call per item.

### `GET /api/wanted?q=&library=...&limit=50&offset=0`
Items with **no indexed subtitles whose file is reachable**, i.e. the ones a
subtitle would fix (Sonarr's "Wanted"). Ordered by show/season/episode.
`{ "items": [{ "rating_key", "kind", "title", "show", "season", "episode", "year", "library" }], "total": 2, "unreachable": 599 }`.
`unreachable` counts the matching items left out because their file can't be
reached (a path mapping to fix, not a subtitle). `q` matches title or show;
`library` limits the list to some libraries and may be repeated
(`&library=TV&library=Movies`; none = all, and `total` / `unreachable` follow the
selection); `limit` is 1-200.
