#!/usr/bin/env python3
"""Build the mock library on disk: tiny placeholder videos, .srt sidecars and a manifest.

    python dev/mock/gen_media.py --out dev/mock/media

Needs ffmpeg on PATH. Each video is a 36-second moving colour gradient (one
colour per show) at 640x360, a few hundred KB, so the whole library is small.
The manifest (manifest.json) lists every item with the path a media server would
report for it (`/data/<library>/...`), which mock_plex.py serves.
"""
import argparse
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import catalog  # noqa: E402

# One gradient per show/library so thumbnails and previews look different.
COLOURS = {
    "The Lantern Cove": ("0x1f6f8b", "0xf2c14e"),
    "Orbital Diner": ("0x2b1b4b", "0xff6f91"),
    "Detective Moss": ("0x3a3a3a", "0xc9b458"),
    "Pocket Dragons": ("0x7a1f3d", "0xff9f1c"),
    "Captain Quackers": ("0x0b6e4f", "0xfff275"),
    "Movies": ("0x14213d", "0xfca311"),
}


def safe(name):
    return re.sub(r'[<>:"/\\|?*]', "", name).strip()


def relative_path(item):
    """Where an item lives under the library, the way a media manager would name it."""
    if item["kind"] == "movie":
        return Path("Movies") / f'{safe(item["title"])} ({item["year"]}).mp4'
    show = safe(item["show"])
    name = f'{show} - S{item["season"]:02d}E{item["episode"]:02d} - {safe(item["title"])}.mp4'
    return Path(item["library"]) / show / f'Season {item["season"]:02d}' / name


def make_video(path, colours):
    c0, c1 = colours
    path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "ffmpeg", "-v", "error", "-y",
        "-f", "lavfi", "-i", f"gradients=s=640x360:r=12:c0={c0}:c1={c1}:speed=0.015:d={catalog.CLIP_SECONDS}",
        "-c:v", "libx264", "-preset", "veryfast", "-crf", "30", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
        str(path),
    ]
    subprocess.run(cmd, check=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="dev/mock/media")
    ap.add_argument("--clean", action="store_true", help="delete the output folder first")
    args = ap.parse_args()
    out = Path(args.out)
    if args.clean and out.exists():
        shutil.rmtree(out)
    if not shutil.which("ffmpeg"):
        sys.exit("ffmpeg is needed to build the placeholder videos")
    manifest = []
    for item in catalog.items():
        rel = relative_path(item)
        video = out / rel
        if not video.exists():
            make_video(video, COLOURS.get(item["show"] or "Movies", COLOURS["Movies"]))
        srt = video.with_suffix(".srt")
        if item["lines"]:
            srt.write_text(catalog.srt(item["lines"]), encoding="utf-8", newline="\n")
        elif srt.exists():
            srt.unlink()
        manifest.append({**{k: v for k, v in item.items() if k not in ("lines", "downloadable")},
                         "has_subtitles": bool(item["lines"]),
                         "duration_ms": catalog.CLIP_SECONDS * 1000,
                         "file": "/data/" + rel.as_posix(),
                         "local": rel.as_posix()})
    (out / "manifest.json").write_text(json.dumps(manifest, indent=1), encoding="utf-8")
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{len(manifest)} items in {out} ({size / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
