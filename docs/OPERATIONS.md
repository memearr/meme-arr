# Operations

Real gotchas hit (and solved) running this app against a real Synology/Docker
deployment and during real development on this codebase — not hypothetical
risks. Read this before touching deploy config or local dev tooling.

## Deploy gotchas (Synology / Docker / Portainer)

### CIFS/SMB share paths with `$` in them
Windows admin shares (`//host/D$`) or any SMB share with a literal `$` in
its name must have that `$` **doubled** in the compose file's `device:`
(`//host/D$$`). A single `$` is treated as a compose variable reference and
silently resolves to an empty string, which then mounts the wrong thing (or
fails) with no error pointing at the real cause. Quoting does *not* help —
compose interpolates inside quoted strings too.

### Named-volume `driver_opts` get cached by the Docker daemon
If you change a named volume's `driver_opts` (e.g. switching a CIFS mount's
`addr=`/`uid=`/`username=` options) in `docker-compose.yml`, Docker does
**not** pick up the change on a plain `docker compose up`. The daemon caches
the volume definition from when it was first created. You have to
explicitly remove the stale volume (`docker volume rm <name>`, after
stopping anything using it) before `up` will actually recreate it with the
new options. This has caused "I changed the compose file but nothing
happened" confusion more than once — always suspect volume caching first
when a mount config edit doesn't take effect.

### snap-installed Docker + CIFS/AppArmor flakiness
On a host where Docker was installed via snap (`/var/snap/docker/...` — the
some NAS-style hosts are), snap's AppArmor confinement often blocks CIFS *volume*
mounts in ways that look like a networking or credentials problem but
aren't. Don't keep fiddling with the volume's `driver_opts`; the robust fix
is to mount the shares on the **host** (`cifs-utils` + `/etc/fstab`), run
`sudo snap connect docker:removable-media` so snap Docker can see `/mnt`,
and bind-mount them (`- /mnt/media/d:/media/d:ro`) instead of using CIFS
volumes. Exact commands: [Getting started → Media on a network share](GETTING-STARTED.md#media-on-a-network-share).

### Plex on Windows reports `D:\...` paths
If the Plex server runs on Windows, every file path it reports looks like
`D:\Movies\Dune.mkv`. Map the drive in Settings → path mappings (`D:\` or
`D:` → `/media/d`); `paths.rs` normalizes `\` to `/` and matches the drive
letter case-insensitively. Without that, Windows paths would match no mapping, every item
would resolve as unreachable, and the UI would show titles indexed but "0 lines indexed". If you
see that pattern, check the mapping's prefix against the raw path Plex reports first.

### Line endings and separators when editing from Windows
Editing `docker-compose.yml`/`.env` from a Windows machine (as this
project's own development happens) risks introducing backslash paths or
CRLF line endings into files that are interpreted on a Linux host (the
Synology, or any Linux Docker host). Keep path separators as forward
slashes in any path that's actually consumed inside the container or by the
Linux-side Docker daemon — only Windows-local tooling paths (if any) should
ever use backslashes. Shell scripts are pinned to LF by `.gitattributes`
(a CRLF `docker-entrypoint.sh` fails with `/bin/sh^M: not found`).

### Permission errors after upgrading to the non-root image
The app now runs as `PUID:PGID` (default `1000:1000`) instead of root. The
entrypoint chowns `/data` and `/output` on startup, but it can't chown
everything — e.g. an `/output` that's itself a CIFS mount ignores chown. If
the logs show `couldn't chown everything under ...` and exports or the
database then fail with permission errors, either fix that mount's
permissions/owner (CIFS: `uid=1000,gid=1000` or world-writable modes), or set
`PUID=0` to run as root like before. Media mounts only need to be readable,
which the example `file_mode=0444,dir_mode=0555` already guarantees.

## A downloaded subtitle stays "Waiting"

Requesting a subtitle from the Wanted tab only asks the media server to fetch it. The request
turns **Indexed** once the item has subtitle lines from a re-index. Where the text comes from
depends on the server:

- **Plex** keeps a subtitle it downloaded (or that was uploaded) in its own database, not as a
  file. **Re-index** reads it through Plex's API (`/library/streams/{id}`) and indexes it with
  its timestamps; nothing is written to your media folder unless you turn on **Settings →
  Subtitles Plex keeps → Also save it as a .srt file next to the video** (off by default; needs
  the media folder mounted writable, never overwrites a file, and Plex may then list the
  subtitle twice). A request stays Waiting only if the
  download hasn't finished, never arrived, or is an image-based subtitle (which can't be searched
  as text).
- **Jellyfin** can save a downloaded subtitle beside the video (library option "Save subtitles
  with media"); this app indexes that file. If Jellyfin keeps it in its own storage instead, it
  can't be read and the request stays Waiting.

0. You don't have to keep pressing Re-index: the scheduled task **Re-check waiting subtitle
   requests** (System → Tasks, hourly by default) re-indexes Waiting requests for three days.
   After that a request needs a manual Re-index.
1. Open **Wanted → Requests → Diagnose** on the request. It states the likely cause and shows
   the facts: where the server says the video is, where this app looks for it, the subtitle files
   in that folder, and what the server itself lists for the item (Jellyfin also gives the path of
   an external subtitle, which shows where it saved it).
2. Typical findings and fixes:
   - *"Plex lists N external subtitle(s) ... Plex keeps these in its own database"*: normal.
     Press Re-index. If it still finds no lines, the Activity log says whether Plex refused to
     serve the subtitle or it is image-based.
   - *"Jellyfin lists N external subtitle(s) ... but none is a file beside the video"*: Jellyfin
     stored it in its own storage. Turn on its library option "Save subtitles with media" and
     make sure Jellyfin is allowed to write to the media folder. Then request it again.
   - *"doesn't list any downloaded subtitle"*: the download didn't complete. Check the server's own
     log, the subtitle provider's daily download limit (OpenSubtitles limits free accounts), and
     write permission on the media folder.
   - *"can't find the video file"*: the path mapping doesn't turn the server's path into a path this
     container can read (Settings → path mappings; System → Health `mapping_unreadable`).
   - *"subtitle files whose names don't start with the video's name"*: rename them to match the
     video; the indexer ignores files that don't.
3. The same facts are in the logs, so they're there after the fact: the **Activity** tab and the
   server log record, for every request, what the server answered (HTTP status, time, any body),
   and for every re-index, *why* nothing changed (the video, the subtitle files beside it and
   which was used, indexed lines, and the stored vs current fingerprint). A failed request is
   logged as a warning too. Turn the log level up (System → Logs) to see more detail.

## The editor preview doesn't play

The preview is a short clip the server renders with ffmpeg, then the browser plays it. A render
goes to a temporary file and only becomes the cache entry once it is a complete MP4. Identical
requests wait for a single render, an abandoned request stops its ffmpeg and removes its temporary
file, and a cached file is checked before it is served. Preview audio is stereo, because a 5.1/7.1
track can stop a browser starting playback. If a preview still doesn't start, the editor says so
after a few seconds; then:

1. Press **Reload preview**.
2. **System → Tasks → Clear preview cache → Run now** removes every cached preview.
3. Look at **System → Logs** for `preview render for … failed` (the message includes
   ffmpeg's own error, for example a missing font or an unreadable file).
4. Check the file plays at all in your media server; an unreadable or truncated source
   file fails every preview of it.

## Local development gotchas

### `RUSTC` environment variable can be broken on a given Windows machine
On some Windows development machines a persisted
`RUSTC` environment variable points at a directory (`C:\Users\<you>\.cargo\bin`)
instead of `rustc.exe`, which breaks every `cargo` invocation with a
confusing error that has nothing to do with the toolchain itself:

```
error: could not execute process `C:\Users\<you>\.cargo\bin -vV` (never executed)
Caused by:
  Access is denied. (os error 5)
```

If you see that, check `echo $env:RUSTC` (PowerShell) before assuming the
toolchain is broken — override it inline for the one command
(`$env:RUSTC = 'C:\path\to\cargo\bin\rustc.exe'; cargo build`) rather than
chasing a toolchain reinstall.

### Stuck ports during manual smoke-testing
A previously-started server process that appears to have exited (doesn't
show up in a process list) can still leave its bound port unreachable for
a while afterward. If a freshly-started instance logs "listening" but
`curl`/`Invoke-WebRequest` against it hangs or refuses, don't assume the new
code is broken — try a different port first to rule out a stuck port left
over from an earlier run before debugging the app itself.

## Git/PR workflow gotcha

The stacked-PR merge trap (merging into a feature-branch base instead of
`main`) is an easy mistake to make: see `docs/DEVELOPMENT.md`'s "Branch / PR workflow"
section for the explanation and the `git merge-base --is-ancestor` check that catches it
before it causes real divergence between `main` and what GitHub's UI claims is merged.

## Dependency-bump gotcha (frontend)

A major-version Dependabot bump to one package can break the build via an
*un-bumped peer dependency* that Dependabot didn't also touch — e.g. a Vite
major bump leaving `@vitejs/plugin-react` on a version whose peer range
doesn't reach the new Vite major. `npm install` locally reproduces the
`ERESOLVE` conflict immediately; the fix is bumping the peer package to a
version that supports the new major, checked against what that peer version
actually requires before taking it (a same-major-number bump isn't always
the minimal safe fix — check its own peer deps too, since skipping straight
to the newest major can pull in machinery you don't want, as happened here
with a plugin-react major that added unwanted Rolldown/React-Compiler
dependencies this app doesn't need). Push the fix directly onto the
Dependabot-created branch so its own PR goes green, rather than opening a
competing PR.
