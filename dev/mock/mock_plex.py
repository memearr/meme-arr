#!/usr/bin/env python3
"""A pretend Plex Media Server serving the mock library (dev/mock/catalog.py).

    python dev/mock/mock_plex.py [--port 32400]

Enough of Plex's HTTP API for meme-arr: identity/version, libraries and items, an
item's streams, and the online subtitle search + download used by the Wanted tab.
Downloading a "fast" result makes Plex keep the subtitle in its (pretend) database,
which meme-arr then reads through /library/streams/<id>; a "slow" result is
accepted but never arrives, so its request stays Waiting (handy for the Requests
and Diagnose screens). Standard library only; state lives in memory.
"""
import argparse
import json
import re
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

sys.path.insert(0, str(Path(__file__).resolve().parent))
import catalog  # noqa: E402

VERSION = "1.41.3.9314-a0bfb8370"
ITEMS = {i["rating_key"]: i for i in catalog.items()}
LIBRARIES = [("1", "show", "TV Shows", "/data/TV Shows"), ("2", "show", "Cartoons", "/data/Cartoons"),
             ("3", "movie", "Movies", "/data/Movies")]
MEDIA_ROOT = Path(".")  # set from the manifest in main()
FILES = {}  # rating_key -> path Plex reports

STORED = {}  # rating_key -> subtitle text Plex "kept"
LOCK = threading.Lock()
PROVIDERS = [("OpenSubtitles", "opensubtitles"), ("Podnapisi", "podnapisi"), ("OpenSubtitles", "opensubtitles"),
             ("Subscene", "subscene"), ("Podnapisi", "podnapisi")]


def dotted(item):
    base = (item["show"] or item["title"]).replace(" ", ".").replace(",", "").replace("'", "")
    if item["kind"] == "episode":
        return f'{base}.S{item["season"]:02d}E{item["episode"]:02d}.{item["title"].replace(" ", ".").replace(",", "")}'
    return f'{base}.{item["year"]}'


def search_results(item, language):
    """Candidates an online search would show, best match first."""
    tags = [("1080p.WEB-DL.DDP5.1.x264-NTb", 4200, True), ("720p.HDTV.x264-LOL", 3100, False),
            ("1080p.BluRay.x264-SPARKS", 2600, False), ("WEBRip.x264-ION10", 1500, False),
            ("DVDRip.XviD-AC3", 700, False)]
    out = []
    for n, ((provider, tag), (release, score, perfect)) in enumerate(zip(PROVIDERS, tags)):
        speed = "fast" if n == 0 else "slow"  # only the first result ever arrives
        out.append({
            "key": f"/library/metadata/{item['rating_key']}/subtitles/{speed}-{n}",
            "id": str(7000 + n), "streamType": "3", "index": str(n),
            "title": f"{dotted(item)}.{release}", "displayTitle": f"{language.title()} (SRT)",
            "extendedDisplayTitle": f"{language.title()} (SRT) from {provider}",
            "language": "English" if language == "en" else "Spanish", "languageCode": "eng" if language == "en" else "spa",
            "languageTag": language, "format": "srt", "providerTitle": provider,
            "perfectMatch": "1" if perfect else "0", "score": str(score), "canAutoSync": "1" if n < 2 else "0",
            "sourceKey": f"{tag}:{item['rating_key']}{n}", "hearingImpaired": "1" if n == 3 else "0",
            "forced": "0", "userID": "1",
        })
    return out


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, body, code=200):
        data = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _text(self, text, code=200):
        data = text.encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        u = urlparse(self.path)
        q = parse_qs(u.query)
        path = u.path
        if path == "/identity":
            return self._json({"MediaContainer": {"claimed": True, "machineIdentifier": "mock-plex", "version": VERSION}})
        if path == "/library/sections":
            return self._json({"MediaContainer": {"Directory": [
                {"key": k, "type": kind, "title": t, "Location": [{"path": p}]} for k, kind, t, p in LIBRARIES]}})
        m = re.fullmatch(r"/library/sections/(\d+)/all", path)
        if m:
            return self._json({"MediaContainer": {"Metadata": [self.metadata(i) for i in ITEMS.values() if i["section"] == m.group(1)]}})
        m = re.fullmatch(r"/library/metadata/(\d+)/subtitles", path)
        if m and m.group(1) in ITEMS:
            lang = (q.get("language") or ["en"])[0]
            return self._json({"MediaContainer": {"Stream": search_results(ITEMS[m.group(1)], lang)}})
        m = re.fullmatch(r"/library/metadata/(\d+)", path)
        if m and m.group(1) in ITEMS:
            return self._json({"MediaContainer": {"Metadata": [self.detail(ITEMS[m.group(1)])]}})
        m = re.fullmatch(r"/library/streams/(\d+)", path)
        if m:
            key = str(int(m.group(1)) - 90000)
            with LOCK:
                text = STORED.get(key)
            if text is not None:
                return self._text(text)
        self.send_response(404)
        self.end_headers()

    def do_PUT(self):
        u = urlparse(self.path)
        q = parse_qs(u.query)
        m = re.fullmatch(r"/library/metadata/(\d+)/subtitles", u.path)
        if m and m.group(1) in ITEMS:
            key = (q.get("key") or [""])[0]
            rk = m.group(1)
            if "/fast-" in key:  # Plex "downloads" it and keeps it itself
                item = ITEMS[rk]
                lines = item["downloadable"] or ["(no dialogue)"]
                with LOCK:
                    STORED[rk] = catalog.srt(lines)
            return self._json({})
        self.send_response(404)
        self.end_headers()

    def metadata(self, item):
        md = {"ratingKey": item["rating_key"], "type": item["kind"], "title": item["title"], "year": item["year"],
              "duration": item["duration_ms"],
              "Media": [{"Part": [{"file": item["file"], "key": f"/library/parts/{item['rating_key']}/file.mp4"}]}]}
        if item["kind"] == "episode":
            md.update({"grandparentTitle": item["show"], "parentIndex": item["season"], "index": item["episode"]})
        return md

    def detail(self, item):
        streams = [{"streamType": 1, "codec": "h264"}, {"streamType": 2, "codec": "aac", "language": "English"}]
        with LOCK:
            kept = item["rating_key"] in STORED
        if kept:
            streams.append({"streamType": 3, "key": f"/library/streams/{90000 + int(item['rating_key'])}", "codec": "srt",
                            "format": "srt", "language": "English", "languageTag": "en",
                            "displayTitle": "English (SRT External)", "providerTitle": "OpenSubtitles"})
        return {"ratingKey": item["rating_key"],
                "Media": [{"Part": [{"file": item["file"], "Stream": streams}]}]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=32400)
    ap.add_argument("--manifest", default="dev/mock/media/manifest.json",
                    help="written by gen_media.py; only its file paths are used")
    args = ap.parse_args()
    manifest = {m["rating_key"]: m for m in json.loads(Path(args.manifest).read_text(encoding="utf-8"))}
    for rk, item in ITEMS.items():
        item["file"] = manifest[rk]["file"]
        item["duration_ms"] = manifest[rk]["duration_ms"]
    srv = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"mock Plex {VERSION} on http://127.0.0.1:{args.port} ({len(ITEMS)} items)", flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
