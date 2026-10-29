# Security

meme-arr is built for your home network. This page says what it protects, what it does not,
and what to do if you want to reach it from outside your home.

## The short version

- Set a strong admin password. Without one anyone who can reach the page can use the app.
- **Do not put it on the open internet without HTTPS** (a reverse proxy that terminates TLS).
  The login cookie is not marked `Secure` so that it works over plain HTTP on a LAN; over the
  internet, the password and session would travel in clear text.
- Mount your media **read-only**.
- Keep it updated ([Upgrading](UPGRADING.md)).

## Who can do what

There is one admin account and no sign-up. Anyone who can log in can use everything, including
changing the media server settings and creating exports. There are no per-user permissions.

If more than one person needs it, or you want single sign-on, use
[Authentik](AUTHENTICATION.md) in front of or alongside it.

## Logging in

- The password is stored as an **Argon2id** hash. It cannot be read back, only replaced.
- Sessions are random 244-bit tokens in an `HttpOnly`, `SameSite=Lax` cookie. Only a hash of
  the token is stored, and sessions last 30 days.
- After five wrong passwords from one address, that address is made to wait (30 seconds,
  doubling to 15 minutes). The limit is per address, so behind a proxy you must list the proxy
  under **Settings → Trusted proxies**; otherwise everyone shares one limit. The header with
  the client address is believed *only* from trusted proxies, so it can't be forged to dodge the
  limit.
- Plex login only works for the Plex account you linked. Anyone can make a Plex account, so a
  Plex login alone proves nothing; the link does.
- Authentik login accepts anyone Authentik lets into the application, so restrict the
  application to the right users there.
- Forgot the password? Use `RESET_ADMIN_PASSWORD` (see
  [Troubleshooting](TROUBLESHOOTING.md#logging-in)). That means whoever controls the
  container's environment controls the app, as with any container.

Details: [Authentication](AUTHENTICATION.md).

## What is reachable without logging in

Only the login page and its files, the login endpoints, and four small status endpoints:

| Endpoint | Shows |
|---|---|
| `/healthz` | that the app is running |
| `/status` | the version, uptime and whether an index pass is running |
| `/metrics` | counts for monitoring: items and subtitle lines indexed, database sizes, export job counts, the version |
| `/api/version` | the version |

None of them shows library titles, file paths, settings or secrets. If you don't want even
that visible from outside, block those paths at your reverse proxy and allow them only from
your monitoring system.

## What it stores

- The **media server token / API key**, the **Authentik client secret** and the **API key hash**
  are stored in the database in `/data`. The token and secret are **not encrypted at rest**.
  The app never sends them back to the browser (Settings only shows that one is set). Anyone who
  can read your `/data` folder (or a settings backup) can read them, so treat that folder like
  a password file: don't share it, don't put it in a public cloud folder.
- The **subtitle text** of your library is in the index. It is derived from your subtitle
  files and stays in `/data`.
- **Fonts** you upload are kept in `/data/fonts`. Only the logged-in admin can upload or delete
  them. An upload is accepted only if its name is a plain `.ttf`/`.otf` file name (any folder in it
  is dropped), it is at most 16 MB, and it parses as a font; nothing is ever run, and fonts are not
  served back to the browser. They are read by ffmpeg when a caption uses them.
- **Memes** are written to `/output`. They carry none of the source file's metadata. The only
  thing a file may carry is a title naming the movie or show/episode; you can switch that off
  per export.

## What the app does on the network

- It connects to **your** Plex or Jellyfin server.
- It asks GitHub whether a newer release exists (the sidebar shows the result).
- If you use them: Authentik (your server) and plex.tv for Plex login.

It sends nothing else out, has no telemetry, and the web page loads no scripts or fonts from
other sites.

## What the container can touch

It runs as a normal user (`PUID`/`PGID`, not root) after fixing the ownership of `/data` and
`/output`. It reads your media from the read-only mount. It writes only to `/data` and
`/output` (and next to your videos, if you turn on the optional `.srt` saving). It runs
`ffmpeg` on your own files to make previews and exports.

## Reaching it from outside

1. Put it behind a reverse proxy that serves **HTTPS** with a real certificate.
2. List the proxy under **Settings → Trusted proxies**.
3. Prefer single sign-on ([Authentik](AUTHENTICATION.md)) or a VPN over exposing the login page.
4. Use a long, unique admin password.
5. Keep the container updated.

See [Deployment](DEPLOYMENT.md#behind-a-reverse-proxy) for proxy examples.

## Reporting a problem

If you find a security problem, please do **not** post the details in a public issue. Use the
repository's **Security** tab and choose **Report a vulnerability** to send it privately.
Include the version and the steps to reproduce when you can. The policy is in
[SECURITY.md](https://github.com/memearr/meme-arr/blob/main/SECURITY.md).
