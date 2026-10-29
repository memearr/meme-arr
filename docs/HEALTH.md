# Health checks

System → Health lists problems the app has detected, most severe first,
and the System item in the sidebar shows a badge while any warning or error
is open — the same idea as Sonarr/Radarr's System → Status → Health.

Checks run as the **Check health** scheduled task: on every startup, every
6 hours by default (System → Tasks), right after you save Plex settings or
path mappings, and whenever you click **Re-check now**. Results are kept in
memory (`GET /api/health`). The code is `src/health.rs`.

**Not reported on purpose:** a library with no path mapping. Mappings are opt-in:
leaving a library unmapped is how you keep it out of the index, so it is skipped
quietly (no health issue, no Activity-log warning). A mapping that *is* set but
points at a folder that doesn't exist is still reported (`mapping_unreadable`).

Each issue in the UI links to its section below. Levels: **error** (something
is broken now), **warning** (degraded, or will break), **info** (nothing
wrong — just worth knowing).

---

### `ffmpeg_missing`
**Error.** `ffmpeg` (or whatever `FFMPEG_BIN` points at) can't be run, so
previews and exports fail. The Docker image ships it; on a custom install,
put it on `PATH` or set `FFMPEG_BIN` to its full path.

### `ffprobe_missing`
**Error.** `ffprobe` (`FFPROBE_BIN`) can't be run, so the indexer can't read
media files' streams and nothing new gets indexed. Same fix as above.

### `gifsicle_missing`
**Warning.** `gifsicle` (`GIFSICLE_BIN`) can't be run. GIF exports still
work — they just skip the extra lossy compression pass, so they're bigger.

### `server_not_configured`
**Warning.** The selected media server (Plex or Jellyfin) has no URL or
credentials yet. Open Settings, enter them, and use **Test** before saving.

### `server_unreachable`
**Error.** The media server's API didn't answer (10-second timeout) or
returned an error. Check the URL (scheme, host, port — `32400` for Plex,
`8096` for Jellyfin), that the token / API key is still valid (Settings →
Test), and that the container can reach the server on the network. Library scans will fail until this is fixed.

### `mapping_unreadable`
**Error.** A path mapping's container folder doesn't exist or can't be
listed, so every item under it resolves as unreachable. Usually the volume
isn't mounted at that path (typo, or the CIFS mount failed), or it isn't
readable by the app's user (`PUID`/`PGID`). See `docs/OPERATIONS.md` for
CIFS and snap-Docker mount problems.

### `dir_not_writable`
**Error.** The output directory (finished exports) or the temp directory
(previews, export intermediates) can't be written. Fix the volume's
owner/permissions for the app's `PUID:PGID`, or set `PUID=0`.

### `low_disk_space`
**Warning.** The data or output volume has less than 2 GB free, or less
than 5% of its size — whichever is larger. Clear old exports or the preview
cache (System), or grow the volume.

### `vacuum_headroom`
**Warning.** The Optimize database task (`VACUUM`) writes a complete copy of
the database before swapping it in, so it needs about as much free space as
the database's current size. Free up space on the data volume, or the task
will fail.

### `update_check_failed`
**Warning.** The GitHub release check failed. Usually the container can't
reach `api.github.com` (no internet, a firewall, or GitHub rate-limiting your
address); it clears by itself once it can. Nothing else is affected. If you run
a private fork, it means `GITHUB_TOKEN` is missing or lacks access: GitHub
answers `404` (not `401`) to unauthenticated requests for private repos. Set
`GITHUB_TOKEN` in the **container's environment** to a token with read
access to the repo — classic PAT with `repo` scope, or fine-grained with
Contents: Read-only. A `read:packages` token used for `docker login
ghcr.io` is a different credential and won't work here.

### `update_available`
**Info.** A newer release tag exists. Pull the new image and recreate the
container.

### `last_scan_failed`
**Error.** The most recent library scan failed outright (e.g. Plex was
unreachable when it started). The Activity log has the details.

### `scan_item_errors`
**Warning.** The most recent library scan finished, but some items failed —
usually unreadable or corrupt files. Each failure's reason is in the
Activity log.

### `subtitle_search_unavailable`
**Info.** Plex is reachable but older than 1.13.8.5395, the first version with
online subtitle search, so "Find subtitles online" won't work. Nothing else is
affected, and it isn't counted in the nav badge. Update Plex Media Server to
use the feature. A version string that can't be read produces no note (the
search is simply attempted).
