# Troubleshooting and FAQ

Find the symptom, read the likely cause, try the fix. If you are setting up for the first
time, [Getting started](GETTING-STARTED.md) has the step-by-step version. Two places tell
you what the app itself thinks is wrong, and they are worth a look before anything else:

- **System → Health** lists the problems it has detected, most severe first, and each one
  says how to fix it. Every message is explained on [Health checks](HEALTH.md).
- **System → Logs** (and the **Activity** log) record what happened, including the exact
  error from ffmpeg or your media server.

## Contents

- [Installing and starting](#installing-and-starting)
- [Logging in](#logging-in)
- [Connecting to Plex or Jellyfin](#connecting-to-plex-or-jellyfin)
- [Indexing and search](#indexing-and-search)
- [The editor and exports](#the-editor-and-exports)
- [Missing subtitles](#missing-subtitles)
- [Speed](#speed)
- [Behind a reverse proxy](#behind-a-reverse-proxy)
- [Questions people ask](#questions-people-ask)
- [Still stuck](#still-stuck)

## Installing and starting

**`docker compose up` can't pull the image.**
Check the image name is exactly `ghcr.io/memearr/meme-arr:latest` and that the machine
can reach `ghcr.io`. If you see `unauthorized` or `denied`, the image is not yet public:
open an issue on the repository. Pulling a public image needs no login.

**`exec format error`, or the container restarts immediately on a Raspberry Pi or an Apple
Silicon NAS.**
The image is built for `linux/amd64` (Intel/AMD) only. There is no ARM image today.

**The container starts but the page doesn't load.**
Use the **host** port, not 8080 inside the container: the example maps `8089:8080`, so open
`http://<your-server>:8089`. Check `docker logs meme-arr` for a startup error. An invalid
`URL_BASE` stops startup with an explanation.

**"Permission denied" in the logs, or System → Health says a folder isn't writable.**
The app runs as `PUID:PGID` (default `1000:1000`). `/data` and `/output` are fixed up
automatically, but it must be able to *read* your media: set `PUID`/`PGID` to the owner of
your media files, or make them world-readable. `PUID=0` runs as root.

**I changed an environment variable and nothing happened.**
Most variables only matter on the first run; the Plex or Jellyfin address, token and path
mappings are changed on the **Settings** page afterwards. See
[Configuration](CONFIGURATION.md) for which is which.

## Logging in

**I forgot the admin password.**
Set `RESET_ADMIN_PASSWORD=<a new password>` in the container's environment and restart it.
Log in with the new password, then **remove the variable** (it is applied on every start
until you do). There is no email reset; see [Authentication](AUTHENTICATION.md).

**"Too many attempts" / a 429 after wrong passwords.**
After 5 failed logins from one address the app makes you wait: 30 seconds, doubling on each
further failure, up to 15 minutes. Wait it out, or restart the container to clear it.

**Behind a reverse proxy, everyone shares one lockout.**
List the proxy under **Settings → Trusted proxies** so the app can see real client addresses.
See [Behind a reverse proxy](#behind-a-reverse-proxy).

**Login seems to do nothing behind a proxy.**
Check that `URL_BASE` matches the path your proxy uses, and that the browser is allowed to keep
cookies for the site. Sessions last 30 days.

## Connecting to Plex or Jellyfin

| You see | Most likely | What to do |
|---|---|---|
| "Couldn't connect" on **Test** | wrong address, or `localhost` used | use the server's LAN IP and port (`32400` for Plex, `8096` for Jellyfin). Inside a container, `localhost` is the container itself |
| Connected, but the library list is empty | the token or API key can't see libraries | make a new one with an admin account |
| Plex says the token is invalid | the token expired or was revoked | get a fresh token and paste it again in Settings |
| `server_unreachable` in Health | the server was down or moved | check the address and that the container can reach it: `docker exec meme-arr wget -qO- http://<ip>:32400/identity` |

## Indexing and search

**Search finds nothing.**
Work down this list:

1. Has indexing run? **System** shows how many items and subtitle lines are indexed; the
   **Activity** log shows the last scan. Start one from **System → Tasks → Library scan →
   Run now**.
2. Does the item have subtitles the app can read? Only *text* subtitles can be searched.
   Image-based ones (PGS, VobSub) are skipped. The **Wanted** tab lists what has none.
3. Is the library mapped? A library with no path mapping is left out on purpose. Map it in
   **Settings → Library path mappings**.
4. Are you searching *Dialogue* when you meant *Title* (or the other way round)?
5. Is a filter chip or a **Search in** library choice narrowing it more than you meant?

**Items are listed as "unreachable".**
The path your media server reports isn't a path this container can read. Fix the mapping or
the mount: compare `docker exec -it meme-arr ls /media` with the mapping on **Settings**.
**System → Health** (`mapping_unreadable`) says which folder.

**Plex runs on Windows and reports `D:\...` paths.**
That works: map the `D:\` prefix to the folder where you mounted that drive. See
[Operations](OPERATIONS.md).

**A subtitle I downloaded isn't searchable yet.**
See [Missing subtitles](#missing-subtitles) below.

**I added new episodes and they don't show up.**
The **Library scan** task re-checks the library on its schedule (**System → Tasks**); run it
now to pick them up at once. Only changed or new files are processed.

**The first scan takes a long time.**
Every file is opened by ffmpeg to find and extract its subtitle track, so a big library on a
slow disk or network share takes a while, once. Later scans skip unchanged files. It keeps
running if you close the page.

**The Library tab says an item has "no subtitles" but I know it does.**
It may have only image-based subtitles, or subtitles in a file whose name doesn't start with
the video's name (the indexer ignores those). Rename the subtitle file to match the video and
re-scan.

## The editor and exports

**The preview shows a player but pressing play does nothing.**
Press **Reload preview**. If that doesn't help, **System → Tasks → Clear preview cache → Run
now**, then look in **System → Logs** for `preview render for … failed`: the message includes
ffmpeg's own error. The details are in [Operations](OPERATIONS.md#the-editor-preview-doesnt-play).

**The preview is slow to appear.**
It is rendered on demand with ffmpeg, at the size and frame rate you chose. Big, high-frame-rate
clips from a network share take longer. Choose a smaller size or lower fps while you trim.

**My MP4/WebM export has no sound.**
Tick **Include audio**. GIF and WebP can't carry audio. A clip whose source file has no audio
track exports silent.

**The captions are in the wrong place in time.**
Subtitle timing can drift from a particular release of a video. Use the sync buttons in the
editor (±100 / ±500 ms): the correction applies to the preview and the export.

**The caption font is not the one I wanted.**
The image ships DejaVu Sans. For the classic meme look, mount a folder containing an
`Impact.ttf` over `/app/fonts` (the commented line in the compose file); it is used when
present.

**An export fails.**
The job shows the error. The usual causes are an unreadable source file (check the path
mapping), a full disk (**System → Health** `low_disk_space`), or `/output` not being writable
(`dir_not_writable`). The Logs have ffmpeg's full message.

**The GIF is huge.**
GIF is an inefficient format. Lower the size, frame rate or clip length, or use WebP or MP4.
`gifsicle` (shipped in the image) shrinks GIFs further; if Health reports `gifsicle_missing`,
GIFs still work but are bigger.

**Does the exported file contain my file names or tags?**
No. Metadata from the original file is removed. The only thing a file may carry is a title
naming the movie or show/episode, which you can switch off. See [Using](USING.md).

**A sequence won't export.**
A sequence is limited to 20 clips and 3 minutes in total.

## Missing subtitles

**A request on the Wanted tab stays "Waiting".**
Asking for a subtitle only tells the media server to fetch it. The request becomes **Indexed**
once the app has re-read the item. Open **Wanted → Requests → Diagnose**: it says where the
server keeps the subtitle and what to do. The common cases and fixes are on
[Operations](OPERATIONS.md#a-downloaded-subtitle-stays-waiting).

**"Find subtitles online" doesn't work on Plex.**
Online subtitle search needs Plex Media Server 1.13.8.5395 or newer. Jellyfin needs a subtitle provider plugin (such as OpenSubtitles) installed
and configured.

**I want the downloaded subtitle saved as a file.**
On Plex, turn on **Settings → Subtitles Plex keeps → Also save it as a .srt file next to the
video**. It is off by default, needs the media folder mounted writable, and never overwrites a
file.

## Speed

**The Library tab, or a page full of results, loads slowly.**
Library and search pages read an index that lives in `/data`. Put `/data` on fast local storage
(an SSD) rather than a network share; SQLite is much slower over SMB/NFS and can even misbehave
there. If it is slow on local disk too, open an issue with the timing shown in your browser's
network tab.

**The container uses a lot of memory or disk.**
The database holds one row per subtitle line, so a large library is a large database.
**System** shows the breakdown, and **System → Tasks →
Optimize database** reclaims space after large changes. It needs about as much free space as
the database's size while it runs.

## Behind a reverse proxy

- At its own hostname (`https://memes.example.com`): proxy to port 8080 and add the proxy under
  **Settings → Trusted proxies**.
- Under a path (`https://home.example.com/meme-arr/`): set `URL_BASE=/meme-arr` and forward the
  prefix **unchanged**. The example for nginx is in [Deployment](DEPLOYMENT.md#behind-a-reverse-proxy).
- Large exports download slowly or stall: raise the proxy's read timeout and turn off response
  buffering for `/api/`.
- Login with Authentik or Plex fails with a redirect error: the address registered with the
  provider must match the address in your browser, including the `URL_BASE`. See
  [Authentication](AUTHENTICATION.md).

## Questions people ask

**Do I need Plex? Does it work with Jellyfin?**
Either one. You choose in the setup wizard. Emby and Kodi are not supported.

**Does it change my media or my media server?**
No. Your media is mounted read-only and the app only reads it. It talks to your media server's
API to list libraries and, if you ask, to search for and request subtitles. The one thing it
can write beside your videos is a `.srt` file, and only if you turn that option on.

**Does it send my library anywhere?**
No. Everything runs in your container. The only outside request the app makes on its own is the
check for a newer release on GitHub, and subtitle searches go through your own media server.

**Where are my memes?**
In the `/output` folder you mounted, and as a download in the browser.

**Can several people use it?**
There is a single admin login. Anyone who has it can use everything. Put it behind your own
access control (for example [Authentik](AUTHENTICATION.md)) if more people need it.

**Which languages' subtitles are searched?**
Whichever text subtitle track the indexer picks for each item. Search matches the words as they
appear in the subtitle text.

**How do I update?**
See [Upgrading](UPGRADING.md).

## Still stuck

Open an issue on the repository and include:

- the version (shown at the bottom of the sidebar),
- what you did and what you expected,
- the relevant lines from **System → Logs** (check them for addresses or file names you would
  rather not share before posting),
- the **System → Health** list.
