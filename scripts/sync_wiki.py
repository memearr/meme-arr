#!/usr/bin/env python3
"""Generate the GitHub wiki from the in-repo docs (docs/ is the single source of truth).

    python scripts/sync_wiki.py <path-to-a-clone-of-the-wiki-repo>

Each doc becomes one wiki page; relative links between docs are rewritten to
wiki page links, and links to other repo files (source, dashboards, ...) become
absolute GitHub URLs. The script only touches the pages it owns, so it is safe
to re-run. Edit docs/, never the generated wiki pages.

The wiki is a separate git repository (<repo>.wiki.git). GitHub creates it
only after a first page exists, so the very first page has to be made once in
the web UI ("Create the first page"); after that this script does the rest.
"""
import posixpath
import re
import sys
from pathlib import Path

REPO_URL = "https://github.com/memearr/meme-arr"
REPO_ROOT = Path(__file__).resolve().parent.parent
BRANCH = "main"

# source file (repo-relative, forward slashes) -> wiki page name
PAGES = {
    "docs/README.md": "Home",
    "README.md": "Overview",
    "docs/GETTING-STARTED.md": "Getting-Started",
    "docs/USING.md": "Using",
    "docs/ARCHITECTURE.md": "Architecture",
    "docs/API.md": "API",
    "docs/DATA_MODEL.md": "Data-Model",
    "docs/AUTHENTICATION.md": "Authentication",
    "docs/DEPLOYMENT.md": "Deployment",
    "docs/DEVELOPMENT.md": "Development",
    "docs/OBSERVABILITY.md": "Observability",
    "docs/HEALTH.md": "Health-Checks",
    "docs/OPERATIONS.md": "Operations",
    "docs/TROUBLESHOOTING.md": "Troubleshooting",
    "docs/CONFIGURATION.md": "Configuration",
    "docs/SECURITY.md": "Security",
    "docs/UPGRADING.md": "Upgrading",
}

SIDEBAR_ORDER = ["Home", "Overview", "Getting-Started", "Using", "Troubleshooting", "Configuration", "Security",
                 "Upgrading", "Architecture", "API", "Data-Model", "Authentication", "Deployment", "Development",
                 "Observability", "Health-Checks", "Operations"]

# Screenshots live in docs/images/ and are copied into the wiki repository
# (a private repo's raw URLs wouldn't load for readers), so links to them become
# wiki-relative `images/<name>`.
IMAGES_DIR = "docs/images"
IMAGE_SUFFIXES = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"}

# [text](target) and ![alt](target); target has no spaces or parentheses.
LINK = re.compile(r"(!?\[[^\]\n]*\])\(([^)\s]+)\)")
FENCE = re.compile(r"^\s*(```|~~~)")


def rewrite_target(target: str, src: str) -> str:
    """The wiki-appropriate target for a link found in repo file `src`."""
    if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target) or target.startswith("#"):
        return target  # http(s), mailto, in-page anchor
    path, _, anchor = target.partition("#")
    resolved = posixpath.normpath(posixpath.join(posixpath.dirname(src), path)) if path else src
    suffix = f"#{anchor}" if anchor else ""
    if resolved.startswith(IMAGES_DIR + "/"):
        return "images/" + resolved[len(IMAGES_DIR) + 1:] + suffix
    if resolved in PAGES:
        return PAGES[resolved] + suffix
    # A file gets a /blob/ link and a folder a /tree/ link. Look at the repository to tell
    # (LICENSE has no extension), and fall back to the extension for paths that don't exist.
    on_disk = REPO_ROOT / resolved
    if on_disk.is_dir():
        kind = "tree"
    elif on_disk.is_file() or posixpath.splitext(resolved)[1]:
        kind = "blob"
    else:
        kind = "tree"
    return f"{REPO_URL}/{kind}/{BRANCH}/{resolved}{suffix}"


def rewrite_links(text: str, src: str) -> str:
    """Rewrite every link outside fenced code blocks."""
    out, in_fence = [], False
    for line in text.split("\n"):
        if FENCE.match(line):
            in_fence = not in_fence
        elif not in_fence:
            line = LINK.sub(lambda m: f"{m.group(1)}({rewrite_target(m.group(2), src)})", line)
        out.append(line)
    return "\n".join(out)


def page_text(text: str, src: str) -> str:
    body = rewrite_links(text.replace("\r\n", "\n").rstrip("\n"), src)
    note = (f"\n\n---\n_This page is generated from [`{src}`]({REPO_URL}/blob/{BRANCH}/{src}) in the repository. "
            f"Edit it there and run `scripts/sync_wiki.py`; changes made here are overwritten._\n")
    return body + note


def sidebar() -> str:
    lines = ["**meme-arr**", ""]
    lines += [f"- [{name.replace('-', ' ')}]({name})" for name in SIDEBAR_ORDER]
    lines += ["", f"[Repository]({REPO_URL}) · [Releases]({REPO_URL}/releases)", ""]
    return "\n".join(lines)


def sync(repo_root: Path, wiki_dir: Path) -> list[str]:
    """Write all pages into `wiki_dir`; returns the files written."""
    written = []
    for src, name in PAGES.items():
        text = (repo_root / src).read_text(encoding="utf-8")
        (wiki_dir / f"{name}.md").write_text(page_text(text, src), encoding="utf-8", newline="\n")
        written.append(f"{name}.md")
    (wiki_dir / "_Sidebar.md").write_text(sidebar(), encoding="utf-8", newline="\n")
    written.append("_Sidebar.md")
    src_images = repo_root / IMAGES_DIR
    if src_images.is_dir():
        out = wiki_dir / "images"
        out.mkdir(exist_ok=True)
        for img in sorted(src_images.iterdir()):
            if img.is_file() and img.suffix.lower() in IMAGE_SUFFIXES:
                (out / img.name).write_bytes(img.read_bytes())
                written.append(f"images/{img.name}")
    return written


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__)
        return 2
    wiki_dir = Path(argv[1])
    if not (wiki_dir / ".git").exists():
        print(f"{wiki_dir} is not a git clone of the wiki repository", file=sys.stderr)
        return 1
    root = Path(__file__).resolve().parent.parent
    for f in sync(root, wiki_dir):
        print("wrote", f)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
