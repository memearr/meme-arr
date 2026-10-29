# Observability

For *configuration* problems (Plex unreachable, unmapped libraries, missing
tools, low disk, update check failing) see the Health panel and
[HEALTH.md](HEALTH.md). The endpoints below are for monitoring systems.

Three public endpoints, three different questions — don't conflate them:

| Endpoint | Question it answers | Touches SQLite? | Auth |
|---|---|---|---|
| `GET /healthz` | "Is the process up at all?" | no | public |
| `GET /status` | "Up, which version, how long, is an index pass running?" | no | public |
| `GET /metrics` | "Give me numbers to graph over time." | **yes** | public |

All three are on the public-path allowlist (`is_public_api_path` in
`api/mod.rs`) deliberately — a monitoring system shouldn't need a session
cookie, and `/healthz` backs the Docker `HEALTHCHECK`, so it must never
depend on auth state.

## `/healthz`

Liveness only: `200 {"ok":true}` as long as the server is handling
requests at all. No DB query, no Plex call. If this fails, the process is
wedged or dead.

## `/status`

A slightly richer liveness check for uptime monitors / status pages:

```json
{ "status": "ok", "version": "v0.25.0", "uptime_seconds": 8123, "index_running": false }
```

Deliberately **in-memory only, no DB query** — so it stays fast even if
SQLite were stuck. That also means it can't tell you the database is
healthy: a locked or corrupted DB still reports `"status": "ok"` here. It
doesn't report Plex reachability either.

## `/metrics`

Hand-rolled Prometheus text format (no `prometheus` crate — a handful of
gauges didn't justify a metrics registry). Every metric is a gauge,
prefixed `meme_arr_`:

| Metric | Labels | Meaning |
|---|---|---|
| `build_info` | `version` | always `1`; the version is the label |
| `media_indexed_total` | | indexed movies + episodes |
| `subtitle_lines_total` | | indexed subtitle cues |
| `index_running` | | `1` while an index pass is running |
| `index_last_run_done` / `_skipped` / `_errors` | | counts from the most recent pass |
| `index_last_run_timestamp_seconds` | | when that pass finished (absent until one has) |
| `db_bytes` | `component`: `file`, `wal`, `subtitle_text`, `media_text`, `index_overhead` | database disk usage breakdown |
| `export_jobs` | `status`: `queued`, `running`, `done`, `failed` | export jobs known to *this process* (in-memory — resets on restart) |
| `compact_index_rows` / `_terms` / `_bytes` | | the shadow-mode compact dialogue index (absent until it's loaded) |

There are no request counters or duration histograms. Because it runs real
DB queries (`db::stats`, `db::db_usage`), `/metrics` is the one public
endpoint whose success actually proves SQLite is answering.

A starter dashboard lives at `grafana/meme-arr-dashboard.json` (see
`grafana/README.md`) — import it and point it at your Prometheus datasource.

## Activity log

A persistent, human-readable event log (`activity_log` table, see
`docs/DATA_MODEL.md`), shown on the UI's Activity tab and served by
`GET /api/activity/log?limit=N` (default 200, max 500, newest first) and
cleared by `DELETE /api/activity/log`. Capped at the most recent 5,000 rows.

What writes to it today:
- **Indexing** (`index/`): pass started/finished with counts, per-item
  failures (the first 25 per pass, then a "further errors suppressed" line),
  sidecar-subtitle and embedded-track extraction failures, skipped unmapped
  libraries, pruned items.
- **Compact index rebuilds** (`compact_rebuild.rs`).
- **Auth** (`api/mod.rs`): every failed password login (with its IP) and each lockout, admin password set/changed, logins via Authentik
  or Plex (successful password logins aren't logged), a rejected Plex login from an
  unlinked account, Authentik settings updated/removed, Plex login
  linked/unlinked.
- **Maintenance**: database vacuum & optimize (start, finish, space freed),
  clearing exported memes or the preview cache, and any shadow-check
  disagreement between the compact dialogue index and FTS5.
- **Scheduled task failures** (`Scheduled task '<label>' failed: …`);
  skipped runs aren't logged, they're recorded on the task itself.
- **Failed exports** (single-clip and sequence), with ffmpeg's actual error
  text — also logged via `tracing`. The job's own `error` field
  (`GET /api/jobs/:id`) is in-memory and lost on restart, so this is the
  only lasting record. Successful exports aren't logged here.

## Logs in the UI

System → Logs shows the last 2,000 server log lines in the browser, with a
minimum-level filter, text filter, pause, download, and a **Server level**
control (error … trace) that takes effect immediately and is remembered
across restarts (it overrides `RUST_LOG`). Debug/trace raise only this app's
own verbosity. Lines are scrubbed of credentials before being kept (see
`logs.rs`). It's an in-memory tail — gone on restart — so for history use
`docker logs` or the persistent Activity log.

## Logging

Standard `tracing`/`tracing-subscriber`, controlled by `RUST_LOG` (e.g.
`info`, `debug`, `meme_arr=debug,tower_http=info`). Goes to stdout —
`docker logs` / Portainer's log viewer is where it lands, not a file. Separate
from the Activity log: `tracing` output is for debugging the app itself; the
Activity log is a curated subset for someone watching the UI.
