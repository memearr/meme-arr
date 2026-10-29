# Development

## Local setup

Prerequisites: Rust (stable, edition 2021, `rust-version = "1.75"` floor),
Node 22.12+ (CI and the image use Node 24), `ffmpeg`/`ffprobe` on `PATH`.

```bash
# Backend, from the repo root
cargo build
cargo test

# Frontend, from web/
npm install
npm run dev      # Vite dev server on :5173, proxies /api and /healthz to :8080
npm test         # Vitest unit tests
npm run build    # production build to web/dist
```

Run the real backend locally (needs `ffmpeg`/`ffprobe` on `PATH`, and a
`DATA_DIR`/`OUTPUT_DIR` that exist):

```bash
DATA_DIR=./data OUTPUT_DIR=./output WEB_DIR=./web/dist INDEX_ON_START=false \
  cargo run
```

First run has no admin password set — it'll show the setup screen. Without
`PLEX_URL`/`PLEX_TOKEN` set, Plex features are unavailable until you
configure them on the Settings page (the app still runs fine without Plex
configured — auth, the UI shell, and anything not Plex-dependent all work).

## Testing

```bash
cargo fmt --all -- --check          # formatting
cargo clippy --all-targets -- -D warnings   # lint, warnings are errors
cargo test                          # unit tests + the pipeline integration tests
cargo audit                         # known-vulnerable crates (CI gate; exceptions in .cargo/audit.toml)
npm test                            # frontend unit tests (Vitest), from web/
npm run build                       # frontend build, from web/
npm audit --audit-level=high        # frontend CI gate, from web/
```

- **Unit tests** live inline in each module (`#[cfg(test)] mod tests`).
  Pure logic (SRT/VTT parsing, path rewriting, ffmpeg arg-building, export
  filtergraph construction, auth crypto, SQL query functions against an
  in-memory SQLite DB via a `mem_pool()` test helper) is thoroughly tested;
  thin orchestration glue that mostly calls already-tested functions
  (`index::run`, the `compact_rebuild` spawn points, API handlers that are
  mostly `spawn_blocking` + a DB call) deliberately isn't — that's a
  consistent convention in this codebase, not an oversight.
- **`tests/pipeline.rs`**: an end-to-end integration test — generates a tiny
  sample video with embedded subtitles via ffmpeg, runs
  extract → index → search → export with **no live Plex server**, proving
  the real pipeline works without needing a Plex instance in CI.
- **`tests/compact_index_benchmark.rs`**: `#[ignore]`d by default (it's a
  benchmark, not a correctness gate). Run explicitly:
  ```bash
  cargo test --release --test compact_index_benchmark -- --ignored --nocapture
  ```
  Two variants: a synthetic-corpus run, and a real-data run that reads
  `real_sample.txt` from the repo root if present (gitignored — real
  copyrighted dialogue text, never committed; export a sample from your own
  library per the comment at the top of that test file if you want to
  re-run it against real data).

**Frontend tests** (Vitest, `web/src/*.test.js`, plain Node environment —
no DOM) cover the logic that lives outside React components: display
formatters (`format.js`), the subtitle browser's find/next/previous logic
(`cueFind.js`), and `api.js`'s error handling (backend error messages
surface, a 401 fires `memearr:unauthorized` except on `/api/auth/*`). The
convention mirrors the backend's: pull non-trivial logic out of `App.jsx`
into a plain module and test it there, rather than rendering components.
Component behavior (layout, scrolling, form wiring) still isn't covered —
verify it by exercising the feature in a browser before calling it done.

## Theming

Dark is the default; Light and Auto (follow the OS via `prefers-color-scheme`)
are in the sidebar's "Theme" toggle, remembered in `localStorage` and applied
by a tiny script in `web/index.html` before first paint (no dark flash for
light users). All colors are CSS variables in `styles.css`; the light palette
appears twice (`:root[data-theme="light"]` and the Auto media query) because
CSS can't share a block — keep the two identical. Don't hardcode colors in new
rules: use or add a variable, and check text pairs against WCAG AA (4.5:1).

## Mock library and screenshots

`dev/mock/` builds a **made-up media library** and serves it from a pretend Plex
server, for demos, manual testing and the documentation screenshots. Everything in
it is invented (shows such as *The Lantern Cove*, *Orbital Diner* and *Detective
Moss*, with original dialogue), so it is safe to show.

| File | What it is |
|---|---|
| `catalog.py` | the library: 3 libraries, 46 items, a few lines that repeat across shows (so `show:` / `-show:` have something to narrow), and 6 items with no subtitles (for **Wanted**) |
| `gen_media.py` | builds tiny placeholder videos (ffmpeg gradients, about 3 MB in all) and `.srt` sidecars under `dev/mock/media/` (git-ignored) plus a `manifest.json` |
| `mock_plex.py` | a pretend Plex (standard library only): libraries and items, an item's streams, and the online subtitle search and download. The first result of a search "downloads" (Plex keeps the subtitle itself, which the app then reads and indexes); the others are accepted but never arrive, so their requests stay **Waiting** |
| `screenshots.cjs` | builds the library, starts the mock server and the app on empty data, indexes, and takes the screenshots with Playwright into `docs/images/` |

To use the mock library by hand:

```bash
python dev/mock/gen_media.py --out dev/mock/media        # needs ffmpeg on PATH
python dev/mock/mock_plex.py                              # http://127.0.0.1:32400
# then run the app and, in Settings, use Plex at http://127.0.0.1:32400 (any token) with
# mappings /data/TV Shows, /data/Cartoons, /data/Movies -> dev/mock/media/<same name>
```

To regenerate the screenshots (after a UI change):

```bash
cargo build && (cd web && npm run build)      # the app and the frontend
npm i --no-save playwright-core               # once; any folder on NODE_PATH works
BROWSER="C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" node dev/mock/screenshots.cjs
```

`BROWSER` is any Chromium-based browser (Edge, Chrome, Chromium); `APP_BIN`, `WEB_DIR`,
`APP_PORT` and `MOCK_PORT` override the defaults (see the top of the script). The script
uses the newest of `target/release` and `target/debug`, and shows Docker-style paths
(`/media/...`) instead of where the mock media is on your disk. It also leaves out the two
health warnings that describe a developer machine (the GitHub update check and a missing
`gifsicle`) so the pictures show a healthy install; this is done in the browser only.
Commit the PNGs in `docs/images/` with the change that altered the UI, and
[publish the wiki](#the-wiki) so it picks them up.

## The wiki

The GitHub wiki is **generated from `docs/` and the top-level `README.md`**, so
there is one source of truth: edit the docs in the repo (in a normal PR), never
the wiki pages directly (they say so at the bottom and are overwritten).

To publish after docs changes land on `main`:

```bash
git clone https://github.com/memearr/meme-arr.wiki.git ../meme-arr.wiki
python scripts/sync_wiki.py ../meme-arr.wiki      # rewrites links between docs into wiki links
cd ../meme-arr.wiki && git add -A && git commit -m "Sync from docs" && git push
```

Notes:
- Screenshots in `docs/images/` are copied into the wiki repository (under `images/`)
  and links to them are rewritten, so link to them with a plain relative path
  that starts with `images/` (from a page in `docs/`) or `docs/images/` (from the
  root README).
- A new file in `docs/` must be added to `PAGES` (and `SIDEBAR_ORDER`) in
  `scripts/sync_wiki.py`. The `docs` CI job (`python3 -m unittest discover -s scripts`)
  fails if a doc is unmapped or any relative link in the docs points nowhere.
- GitHub creates a wiki's git repository only once its first page exists, and
  there's no API for that: if `…wiki.git` says "not found", open the repo's
  **Wiki** tab and click **Create the first page** (any text), then clone.

## Branch / PR workflow

Nothing here is enforced by tooling, but this is the workflow the project follows:

1. One branch per logical change, off `main`.
2. Full local verification (the four commands above) before pushing.
3. A pull request with a real summary and a test plan, including any manual or smoke
   testing done beyond the automated suite, and what was *not* verified.
4. CI (`.github/workflows/ci.yml`) must be green before merging.
5. After a merge, sync local `main`, re-verify, and for anything meant to ship cut a
   `vX.Y.Z` tag (the release workflow builds and publishes the image).

**One sharp edge**: if a PR's base is another *unmerged* feature branch (a "stacked" PR)
rather than `main`, GitHub's merge button merges into *that* base, not `main`, even after
the underlying branch's own PR has merged. Check with
`git merge-base --is-ancestor <branch> origin/main` before trusting a "Merged" label, and
prefer not to stack PRs across a review/merge boundary at all: retarget to `main` the
moment the base lands.

## Code style notes specific to this repo

- Comments explain *why*, not *what* — identifiers are expected to carry
  the "what" on their own. A comment that would be made redundant by
  deleting it isn't kept.
- No premature abstraction — three similar lines are preferred over a
  speculative shared helper built for a hypothetical second use case that
  hasn't shown up yet.
- `anyhow::Error` end-to-end for application errors; `AppError` in
  `api/mod.rs` is the one place that turns an `anyhow::Error` into an HTTP
  response.
- Blocking work (SQLite via `rusqlite`, filesystem) is wrapped in
  `tokio::task::spawn_blocking` from async handlers — `rusqlite` has no
  async story, so this is the load-bearing pattern keeping the executor
  unblocked.
