# Data model

One SQLite database (`DATA_DIR/index.sqlite3`, default `/data/index.sqlite3`
in the container), WAL mode, bundled `rusqlite` (no system SQLite
dependency). The schema lives in `src/db/mod.rs` and the queries in the other files of
`src/db/` (one per area) — nothing else in the crate writes raw SQL against these tables.

**Migrations**: `migrate()` runs `CREATE TABLE IF NOT EXISTS` for everything
below on every startup (idempotent), plus `add_column_if_missing()` calls
for columns added after a table's initial release (so an existing DB from
an older version picks up new columns without a destructive migration
step). There's no migration framework or versioned migration files — it's
all additive, forward-only `IF NOT EXISTS`/`add_column_if_missing`.

## Catalog

### `media`
One row per indexed movie or episode.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | |
| `rating_key` | TEXT, UNIQUE | the media server's stable item ID (Plex ratingKey / Jellyfin Id) |
| `kind` | TEXT | `"movie"` \| `"episode"` |
| `title` | TEXT | |
| `show` | TEXT, nullable | episodes only |
| `season`, `episode` | INTEGER, nullable | episodes only |
| `year` | INTEGER, nullable | |
| `file_path` | TEXT | path as the *media server* reports it, before container rewriting |
| `part_key` | TEXT, nullable | Plex's `/library/parts/...` key (download fallback, not currently wired) |
| `duration_ms` | INTEGER | |
| `access_mode` | TEXT | `"direct"` (reachable on a mounted volume) or the legacy value `"plex-download"` (not reachable on a mounted volume; the name predates Jellyfin support and is kept so existing databases stay valid) |
| `library` | TEXT, nullable | the Plex library section title — added post-release via `add_column_if_missing` |
| `file_mtime`, `file_size` | INTEGER | the fingerprint used to skip re-indexing an unchanged file: the **video plus its sidecar subtitle** (newest mtime, summed size), so a subtitle added or replaced later is noticed. Rows indexed before this carry the video-only value, so items that have a sidecar are re-indexed once |
| `indexed_at` | INTEGER | unix seconds |
| `cue_count` | INTEGER | how many cues the item has, kept by `upsert_media` (reset to 0 on re-index) and `insert_cues`, and filled in once for databases from before it existed. The Library, stats and Wanted views read it instead of counting the cue table |
| `indexer_version` | INTEGER | added post-release; compared against `INDEXER_VERSION` in `index/` so a smarter indexer reprocesses already-seen files once instead of old rows staying stuck forever |

### `subtitle_cue`
One row per subtitle line, the thing dialogue search actually matches
against.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK | this *is* the row id the dialogue index (`compactindex/`) and FTS5 both key off |
| `media_id` | INTEGER | `REFERENCES media(id) ON DELETE CASCADE` |
| `start_ms`, `end_ms` | INTEGER | |
| `text` | TEXT | the real, untouched subtitle line |

Indexed on `media_id` (`idx_cue_media`) for the window queries behind
preview/export cue lookup.

### `subtitle_fts`
A **virtual table** — SQLite's FTS5 extension, in **external-content** mode
(`content='subtitle_cue', content_rowid='id'`), `unicode61` tokenizer. This
is the actual dialogue search index. External-content means FTS5 stores no
second copy of the subtitle text — it indexes `subtitle_cue.text` in place
and looks the real text back up by rowid when needed, which is also why
`cue_ai`/`cue_ad` triggers exist: FTS5's own index has to be told explicitly
on every insert/delete to `subtitle_cue`, it isn't automatic for an
external-content table.

`detail='full'` (the default, not explicitly set) means FTS5 stores
per-occurrence position data — what makes native phrase `MATCH` queries
work, and also the dominant cost in this index's size at scale (see the
compact-index research notes in `compactindex/mod.rs`'s module doc and
`docs/ARCHITECTURE.md`).

## App configuration

### `subtitle_request`
Subtitle downloads the app asked the media server for (the Wanted tab's Requests list):
`id`, `rating_key`, `server` (`plex`|`jellyfin`), `language`, `subtitle` (the result's
title), `provider`, `requested_at`. No foreign key to `media`: a scan can delete and
recreate an item's row and the request should survive that. Only the newest 500 are kept.
Whether a request has become searchable is **not stored**: it's worked out when listing, from
the item's current cues and `media.indexed_at`.

### `settings`
Plain key/value store. `media_server` (`plex` | `jellyfin`; absent = `plex`),
`plex_url`, `plex_token`, `jellyfin_url`, `jellyfin_api_key`, `admin_password_hash`,
`authentik_issuer_url`/`authentik_client_id`/`authentik_client_secret`,
`plex_client_identifier`, `plex_login_account_uuid`/`plex_login_username` —
every piece of app config that isn't a path mapping lives here, not in env
vars (env vars only *seed* these on first run — see `docs/DEPLOYMENT.md`).

### `path_mapping`
Ordered list of Plex-path → container-path rewrites (`paths.rs` walks these
in `position` order, first match wins).

| Column | Type |
|---|---|
| `id` | INTEGER PK |
| `position` | INTEGER |
| `plex_prefix` | TEXT |
| `container_path` | TEXT |
| `label` | TEXT, nullable |

## Observability

### `activity_log`
The Activity tab's persistent log. Trimmed to the most recent 5000 rows on every
insert (`DELETE ... WHERE id <= (SELECT MAX(id) - 5000 ...)`, a no-op once
under the cap).

| Column | Type |
|---|---|
| `id` | INTEGER PK |
| `ts` | INTEGER (unix seconds) |
| `level` | TEXT — `"info"` \| `"warn"` \| `"error"` |
| `message` | TEXT |

## Authentication

See `docs/AUTHENTICATION.md` for how these are actually used.

### `scheduled_task`
One row per scheduled task (`tasks.rs`): its schedule and the outcome of its
most recent run, so "last run" and "next run" survive restarts.

| Column | Type | Notes |
|---|---|---|
| `name` | TEXT PK | task id, e.g. `library_scan` |
| `enabled`, `interval_secs` | INTEGER | the user's schedule; never overwritten once the row exists |
| `created_at` | INTEGER, nullable | a never-run task is first due one interval after this; NULL (library scan) means due immediately — so upgrading doesn't trigger an instant VACUUM |
| `last_started`, `last_finished` | INTEGER, nullable | unix seconds; `next_run = last_started (or created_at) + interval` |
| `last_duration_ms` | INTEGER, nullable | |
| `last_error` / `last_summary` | TEXT, nullable | one or the other; a skipped run is a summary (`Skipped — …`), not an error |

### `sessions`
| Column | Type | Notes |
|---|---|---|
| `token_hash` | TEXT PK | **SHA-256 of the cookie token**, never the raw value |
| `created_at`, `expires_at` | INTEGER | fixed 30-day TTL from login, no sliding refresh |

### `oidc_flows`
Transient, one-time-use state for an in-progress Authentik login, bridging
the redirect → callback gap.

| Column | Type |
|---|---|
| `state` | TEXT PK — the CSRF token Authentik echoes back |
| `nonce` | TEXT |
| `pkce_verifier` | TEXT |
| `created_at`, `expires_at` | INTEGER — 10-minute TTL |

### `plex_flows`
Same shape/purpose for the Plex PIN login flow.

| Column | Type |
|---|---|
| `pin_id` | INTEGER PK |
| `code` | TEXT |
| `purpose` | TEXT — `"login"` or `"link"` (see AUTHENTICATION.md) |
| `created_at`, `expires_at` | INTEGER — 10-minute TTL |

## What isn't stored in SQLite

The research compact dialogue index (`compactindex/`) is **not** a SQLite
table at all — it's a standalone binary file
(`DATA_DIR/compact_index.bin`), built/loaded by `compact_rebuild.rs`. It's a
rebuildable derived cache, never authoritative: `subtitle_cue`/`subtitle_fts`
stay the real source of truth, and the compact index can always be thrown
away and rebuilt from them. See ARCHITECTURE.md for why it exists and what
state it is in (shadow mode, not live).
