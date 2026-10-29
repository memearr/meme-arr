# Third-party notices

meme-arr itself is released under the [MIT license](LICENSE). The software and fonts below are
included in the project or its Docker image under their own licenses.

## Fonts bundled in `assets/fonts/`

These are copied into the Docker image (`/app/fonts`) and offered as built-in caption fonts. They
are unmodified. Each license text is in [`assets/fonts/licenses/`](assets/fonts/licenses/).

| Font | Copyright | License | Source |
|---|---|---|---|
| Anton | 2020 The Anton Project Authors | SIL Open Font License 1.1 | https://github.com/googlefonts/AntonFont |
| Bebas Neue | 2019 The Bebas Neue Project Authors (Dharma Type) | SIL Open Font License 1.1 | https://github.com/dharmatype/Bebas-Neue |
| Bangers | 2010 The Bangers Project Authors | SIL Open Font License 1.1 | https://github.com/googlefonts/bangers |
| Comic Neue (Regular, Bold) | 2014 The Comic Neue Project Authors | SIL Open Font License 1.1 | https://github.com/crozynski/comicneue |
| Luckiest Guy | 2010 Brian J. Bonislawsky DBA Astigmatic (AOETI) | Apache License 2.0 | https://fonts.google.com/specimen/Luckiest+Guy |

The SIL Open Font License allows these fonts to be used, copied and redistributed, including
inside other projects, provided the copyright notice and license come with them and the fonts are
not sold on their own.

## Installed in the Docker image from Debian

The image installs **DejaVu Sans** (Bitstream Vera and public-domain license), **ffmpeg** and
**gifsicle** from Debian's packages; they keep their own licenses, are run as separate programs
and are not linked into meme-arr.

## Software dependencies

The Rust and JavaScript libraries meme-arr is built from are listed in `Cargo.lock` and
`web/package-lock.json`; all are under permissive licenses (MIT, Apache-2.0, BSD, ISC and similar,
and MPL-2.0 for some build-time tooling).
