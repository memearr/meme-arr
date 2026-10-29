# Fonts

Burned subtitles and meme text are drawn with a font chosen per clip in the editor. This folder
holds the fonts that come with the app; the Docker image copies it to `/app/fonts`
(the default `FONTS_DIR`) and they show up as *built in*.

| File | Font | License |
|---|---|---|
| `Anton-Regular.ttf` | Anton: a heavy condensed face, the closest free look to Impact | OFL 1.1 |
| `BebasNeue-Regular.ttf` | Bebas Neue: clean all-caps | OFL 1.1 |
| `Bangers-Regular.ttf` | Bangers: comic-book lettering | OFL 1.1 |
| `ComicNeue-Regular.ttf`, `ComicNeue-Bold.ttf` | Comic Neue: a friendlier Comic Sans | OFL 1.1 |
| `LuckiestGuy-Regular.ttf` | Luckiest Guy: playful and heavy | Apache 2.0 |

The license for each is in [`licenses/`](licenses/), and summarised in
[THIRD-PARTY-NOTICES.md](../../THIRD-PARTY-NOTICES.md). The image also installs **DejaVu Sans**
from Debian; it is the default font. Anything added here must be free to redistribute, come with
its license file in `licenses/`, and be listed in THIRD-PARTY-NOTICES.md (a test checks this).

## Adding your own

The easy way is **Settings -> Caption fonts** in the app: upload a `.ttf` or `.otf`, and it is kept
in your data folder. You can instead mount a folder over `/app/fonts`, but that *replaces* the
built-in set, DejaVu Sans included. If it contains an `Impact.ttf` that becomes the default.

Default font preference (first found wins): `Impact.ttf`, `DejaVuSans-Bold.ttf`, `DejaVuSans.ttf`.
