# Grafana dashboard

`meme-arr-dashboard.json` is an example dashboard for the `/metrics`
endpoint (Prometheus text format, no auth required — see `api::metrics` in
`src/api/mod.rs`).

## Setup

1. Point Prometheus at this app: add a scrape job targeting
   `<host>:8080` (or wherever it's exposed) with `metrics_path: /metrics`.
2. In Grafana: **Dashboards → Import**, upload `meme-arr-dashboard.json`
   (or paste its contents), and pick your Prometheus datasource when prompted.

## What's on it

- **Build** — current version (from `meme_arr_build_info`).
- **Media indexed / Subtitle lines indexed** — current totals.
- **Indexing now?** — whether a library pass is running.
- **Last index pass** — done/skipped/errors from the most recently completed
  pass.
- **Database size by component** (pie) and **over time** (stacked area) —
  breaks down the DB file into subtitle text, media text, and search-index
  overhead (see `db::db_usage`).
- **Export jobs by status** — queued/running/done/failed, scoped to the
  currently running process (in-memory, resets on restart).
- **Compact dialogue index (research, shadow mode)** — row/term/byte counts
  for the experimental compact index described in the project notes. Not
  wired into live search; this panel is purely observability on the
  background-rebuilt shadow copy.

All panels use the `${DS_PROMETHEUS}` template variable for the datasource,
so re-importing onto a different Prometheus instance just needs that one
picker, not editing every panel.
