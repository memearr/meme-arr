# Upgrading

Your settings and index live in the `/data` folder you mounted, so upgrading means replacing
the container and keeping that folder.

## Before you upgrade

1. **Read the release notes** for the version you are going to: the
   [releases page](https://github.com/memearr/meme-arr/releases) lists what changed and
   anything you need to do.
2. **Back up `/data`.** Stop the container, copy the whole folder (it holds `index.sqlite3`,
   your settings and the backups), and start it again. That is a full backup, including the
   index, so a rollback does not need a rescan. The in-app **System → Backups** only saves
   settings (and lives inside `/data` too).

## Upgrade with Docker Compose

```sh
docker compose pull
docker compose up -d
```

`pull` fetches the newest `:latest`; `up -d` recreates the container with it. To stay on a
specific version, change the image line to a tag such as
`ghcr.io/memearr/meme-arr:v0.25.0` and do the same.

## Upgrade with Portainer or Synology

- **Portainer:** open the stack and use **Pull and redeploy** (with *Re-pull image* on).
- **Synology Container Manager:** pull the new image (Registry, or update the project), then
  recreate the container from it with the same folders and settings. Your folders are not
  touched when a container is deleted and recreated.

## What happens on the first start of a new version

- The database is **migrated automatically**. Migrations only add things; they don't remove your
  data.
- Some versions fill in new data once at startup (for example, the per-item subtitle count that
  the Library views read). On a big library that can take a few seconds to a minute;
  the page may be slow to appear once.
- A restart does not rescan the library unconditionally: the scan runs when it is due.
- The sidebar shows the version. **System → Health** says `update_available` while a newer
  release exists.

## Changes worth knowing about

Newer versions, newest first. Nothing here needs a change to your compose file unless it says so.

- **0.32: reverse proxies and HTTPS.** The app now believes `X-Forwarded-Proto/-Host/-Port/-Prefix`
  only from a proxy on a private (or loopback) address or one on **Settings → Trusted proxies**;
  from a public address they are ignored. A proxy on your LAN or Docker network needs nothing; a
  proxy on a public address has to be listed (or set `PUBLIC_URL`). New optional settings:
  `PUBLIC_URL`, `HSTS_MAX_AGE`, `TLS_CERT` / `TLS_KEY` (the app serves HTTPS itself), `EXTRA_CA_CERTS`
  (trust a private CA for Plex, Jellyfin, Bazarr, Authentik) and `BROWSE_ROOTS`. The image's health
  check now tries HTTP and then HTTPS. The Sequence tab is now a set editor.
- **0.30–0.31: Bazarr, and subtitles in old encodings.** The first start after upgrading re-reads
  every item once (the indexer version went to 4): subtitle files in Windows-1252, UTF-16 and other
  legacy encodings are decoded instead of failing, and when a video has several subtitle files the
  indexer now prefers the language in Bazarr's profiles (English without Bazarr), skips `.forced`
  files unless they are all there is, and prefers a plain one over a hearing-impaired one. On a big
  library the rescan takes a while; searching works meanwhile.
- **0.27–0.29: the frame-accurate editor, and security fixes.** The editor gained a full-width player,
  a filmstrip timeline, exact In/Out and a cut check; the session cookie is marked `Secure` when you use
  HTTPS; downloads and the preview folder are confined to their folders.

## If something goes wrong

1. Look at **System → Health** and **System → Logs**, and `docker logs meme-arr`.
2. To go back: stop the container, put the image tag back to the previous version, restore the
   `/data` copy you made, and start it. Going back *without* restoring `/data` usually works too
   (new columns have defaults that older versions ignore), but the copy is the safe way.
3. Open an issue with the version you came from and the one you went to, plus the log lines.

## Moving to a new machine

1. Install on the new machine ([Getting started](GETTING-STARTED.md)) and copy your `/data`
   folder across (or mount a new one and use **System → Backups → Restore**, then run the
   library scan).
2. Keep the **media paths** the same inside the container (`/media/...`), so your mappings
   still match.
3. If the media server's address changed, update it in **Settings**.

## Updates and the version check

The app asks GitHub for the newest release tag and shows it in the sidebar. It never updates
itself. If you run a fork, set `GITHUB_REPO` to it (and `GITHUB_TOKEN` too if the fork is
private); see [Deployment](DEPLOYMENT.md#environment-variables).
