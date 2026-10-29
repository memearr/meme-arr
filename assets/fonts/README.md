# Fonts

Meme text and burned subtitles are rendered with a bundled font. The Docker
image installs **DejaVu Sans** (`fonts-dejavu-core`) and copies it into
`/app/fonts`, which is the default `FONTS_DIR`.

## Using an Impact-style meme font

The classic meme look uses Impact, which is proprietary and not shipped here.
To use it (or any other font), mount a directory containing the `.ttf` over
`/app/fonts`:

```yaml
volumes:
  - ./fonts:/app/fonts:ro
```

Font preference order (first found wins): `Impact.ttf`, `DejaVuSans-Bold.ttf`,
`DejaVuSans.ttf`. So dropping an `Impact.ttf` into the mounted folder makes it
the default automatically.

This folder in the repo is intentionally empty of binaries — fonts are provided
at runtime by the image or your mount.
