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
`ghcr.io/memearr/meme-arr:v0.24.1` and do the same.

## Upgrade with Portainer or Synology

- **Portainer:** open the stack and use **Pull and redeploy** (with *Re-pull image* on).
- **Synology Container Manager:** pull the new image (Registry, or update the project), then
  recreate the container from it with the same folders and settings. Your folders are not
  touched when a container is deleted and recreated.

## What happens on the first start of a new version

- The database is **migrated automatically**. Migrations only add things; they don't remove your
  data.
- Some versions fill in new data once at startup (for example, version 0.24.1 adds a per-item
  subtitle count and fills it in). On a big library that can take a few seconds to a minute;
  the page may be slow to appear once.
- A restart does not rescan the library unconditionally: the scan runs when it is due.
- The sidebar shows the version. **System → Health** says `update_available` while a newer
  release exists.

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
