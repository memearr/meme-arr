# meme-arr documentation

**meme-arr** turns any moment in your Plex or Jellyfin library into a meme: search for a line
of dialogue, jump to that moment, trim it, caption it, and export a GIF, WebP, MP4 or WebM.
It runs as one Docker container on your own server.

New here? Read **[Getting started](GETTING-STARTED.md)**: it takes you from nothing to your
first meme, with a screenshot of every step. Something not working? Go to
**[Troubleshooting](TROUBLESHOOTING.md)**.

## Using it

| Page | What's in it |
|---|---|
| [Getting started](GETTING-STARTED.md) | Install with Docker, connect Plex or Jellyfin, map your libraries, index, and make a first meme |
| [Using meme-arr](USING.md) | A tour of every tab: searching (operators, filters), the editor, sequences, finding missing subtitles, System |
| [Troubleshooting and FAQ](TROUBLESHOOTING.md) | Symptoms, causes and fixes; common questions |
| [Health checks](HEALTH.md) | Every message on System → Health: what it means and how to fix it |

## Running it

| Page | What's in it |
|---|---|
| [Configuration](CONFIGURATION.md) | What every setting does, and whether it lives in the compose file or in the app |
| [Security](SECURITY.md) | What is protected, what is stored, and how to reach it from outside safely |
| [Upgrading](UPGRADING.md) | Updating, backing up, rolling back, moving to a new machine |
| [Deployment](DEPLOYMENT.md) | The image, compose stack, every environment variable, reverse proxies |
| [Authentication](AUTHENTICATION.md) | Password, Authentik and Plex login; sessions; recovery |
| [Observability](OBSERVABILITY.md) | `/healthz`, `/status`, `/metrics`, the Activity log, a Grafana dashboard |
| [Operations](OPERATIONS.md) | Deploy gotchas (network shares, Windows paths) and deeper fixes |

## How it works and how to change it

| Page | What's in it |
|---|---|
| [Architecture](ARCHITECTURE.md) | System design, every module's purpose, request lifecycle, frontend structure |
| [API](API.md) | Every HTTP endpoint: method, auth requirement, request/response shape |
| [Data model](DATA_MODEL.md) | The SQLite schema and migration conventions |
| [Development](DEVELOPMENT.md) | Local setup, tests, the mock library, PR conventions |

## 30-second orientation (for contributors)

- **Stack**: Rust (axum/tokio) backend, shells out to `ffmpeg`/`ffprobe` for
  all media work, SQLite (FTS5) for the dialogue index, a React+Vite SPA built
  to static files and served by the same binary. One container, one port.
- **The one thing that makes this app exist**: dialogue search. The app walks
  a Plex or Jellyfin library, extracts subtitle text with ffmpeg, and indexes every line
  in SQLite FTS5, so "search a line you remember, jump straight to that
  moment" actually works — Frinkiac, but for your own Plex or Jellyfin library.
- **Where things live**: `src/` is the whole Rust crate (binary = `src/main.rs`,
  everything else lives in the library at `src/lib.rs` so integration tests
  can reach it); `web/` is the frontend (the shell in `App.jsx`, one component file per area, small
  unit-tested logic modules, `api.js` — a thin fetch wrapper — and `styles.css`); `tests/` has an end-to-end pipeline
  test and the compact-index research benchmark.
- **What's authoritative vs. in-progress**: `subtitle_cue`/`subtitle_fts`
  (SQLite FTS5) is the real, live dialogue search path today. There's a
  from-scratch alternative inverted index (`src/compactindex/`) running in
  **shadow mode** (built/compared in the background, never affects what a
  user sees) — see ARCHITECTURE.md's note on it before assuming it's live.
