# Configuration

Almost everything is configured **in the app**, on the **Settings** and **System** pages, and
saved in the database in your `/data` folder. The compose file only needs to say where your
data and media live. This page says what each setting does and where it is.

## What goes where

| It is set... | Examples | Changing it |
|---|---|---|
| In the **compose file** (always) | the ports, the folders you mount (`/data`, `/output`, `/media`), `PUID`/`PGID`, `URL_BASE` | edit the file, recreate the container |
| In **Settings** (the app) | the Plex or Jellyfin address and token, which folder each library is mounted at, logins, trusted proxies, the API key | change it in the app; it takes effect at once |
| In **System → Tasks** | when the library scan, backups and clean-up run | change the schedule in the app |
| By a **first-run variable** | `PLEX_URL`, `PLEX_TOKEN`, `PATH_MAP` | only read the first time; afterwards use Settings |

The full list of environment variables is in [Deployment](DEPLOYMENT.md#environment-variables).
A normal install sets none of them except `PUID`, `PGID` and sometimes `URL_BASE`.

## The folders you mount

| Folder in the container | What it is | Notes |
|---|---|---|
| `/data` | settings, the subtitle index, backups, temporary files | **Keep this.** Put it on local storage (an SSD if you can), not a network share |
| `/output` | finished memes | also offered as a browser download |
| `/media` (or anything you choose) | your videos | mount **read-only** (`:ro`). It must be the same files your media server sees |

## Settings

### Media server connection

Choose **Plex** or **Jellyfin**, give the address (`http://<ip>:32400` or `http://<ip>:8096`)
and the token (Plex) or API key (Jellyfin), and press **Test**. The token is stored in the
app's database and is never sent back to the browser: once saved, the page only shows that one
is set. Leave the field empty when saving to keep the saved one.

### Library path mappings

Your media server reports file paths as *it* sees them (`/volume1/video/TV/...`). This app
reads the same files at a different place (`/media/TV/...`). Each mapping says "turn this
server folder into this container folder". The wizard suggests them by matching folder names.

- A library with **no mapping is skipped** on purpose. That is how you keep a library out of
  the index.
- A mapping whose container folder doesn't exist is reported in **System → Health**
  (`mapping_unreadable`).
- Plex on Windows reports `D:\...` paths; map the `D:\` prefix like any other.

### Subtitles Plex keeps (Plex only)

Plex stores subtitles it downloaded in its own database. The app reads them through Plex's API
and indexes them. Optionally it can also **save each one as a `.srt` file next to the video**.
That is off by default; it needs the media folder mounted writable, never overwrites an
existing file, and Plex may then list the subtitle twice.

### Trusted proxies

If you run the app behind a reverse proxy, list the proxy's address (or network, such as
`172.18.0.0/16`) here. Then the app believes the `X-Forwarded-For` header **only** when the
connection comes from one of those addresses, so login lockouts and logs see the real client.
Leave it empty if you do not use a proxy.

### API key

An optional key for dashboards and scripts. It is read-only and limited to a short list of
status endpoints (health, tasks, index status, library stats, storage); it can't read settings
or change anything. It is shown once when created. See [Authentication](AUTHENTICATION.md).

### Plex login, Authentik login, admin password

Three ways to prove you are the admin; all can be on together. See
[Authentication](AUTHENTICATION.md).

## System

### Tasks

Scheduled jobs, each with **Run now**:

| Task | What it does |
|---|---|
| Library scan | walks the mapped libraries and indexes new or changed files' subtitles |
| Optimize database | compacts the database (needs free space about equal to its size while it runs) |
| Clear preview cache | deletes rendered previews |
| Trim activity log | keeps the Activity log from growing without limit |
| Check health | refreshes the Health list |
| Back up settings | writes a settings snapshot to `/data/backups` (14 are kept) |
| Re-check waiting subtitle requests | re-reads items whose requested subtitle is still Waiting, for three days |

### Backups

The **Back up settings** task saves your configuration (not the subtitle index) daily. To move
to a new container: mount a fresh `/data`, open the app, **System → Backups → Restore**, then
run the library scan. Backup files contain your server token and login secrets, and they live
in `/data`, so copy them elsewhere if you want protection from losing that folder. See
[Upgrading](UPGRADING.md) for backing up the whole of `/data`.

### Logs and health

**Logs** is the app's log with adjustable detail; **Activity** is a readable history of scans,
requests and logins. **Health** lists problems with how to fix them; see
[Health checks](HEALTH.md).

## Other options

- **A different font for captions.** Put an `Impact.ttf` in a folder, mount it over
  `/app/fonts` (the commented line in the compose file), and it is used when present.
- **Run under a sub-path** (`https://home.example.com/meme-arr/`): set `URL_BASE=/meme-arr`
  and see [Deployment](DEPLOYMENT.md#behind-a-reverse-proxy).
- **A different port**: change the left side of `8089:8080`. Leave the right side alone.
- **Don't scan on first start**: set `INDEX_ON_START=false`; after that the schedule is under
  **System → Tasks**.
