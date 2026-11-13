# syntax=docker/dockerfile:1

# ffmpeg for the meme-arr image, built once and published as
#   ghcr.io/memearr/ffmpeg:<version>
# by .github/workflows/ffmpeg-image.yml. The main Dockerfile copies the result
# in, so a normal image build never compiles ffmpeg. To change the version,
# edit the two ARGs below (checksum: `sha256sum ffmpeg-<version>.tar.xz`).
#
# Only the codecs and filters meme-arr uses are built: libx264, libvpx (VP9),
# libopus, libwebp, libass (burned subtitles) and drawtext (meme text); GIF,
# AAC and palettegen are part of ffmpeg itself. The ffmpeg libraries are linked
# in statically; the codec libraries are copied next to the binaries
# (/opt/ffmpeg/lib, found through an rpath), so the result only needs glibc
# from the image it is copied into (debian:trixie).

FROM debian:trixie-slim AS build
ARG FFMPEG_VERSION=9.0.2
ARG FFMPEG_SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential nasm pkg-config curl xz-utils ca-certificates \
      libx264-dev libvpx-dev libopus-dev libwebp-dev libass-dev \
      libfreetype-dev libfontconfig-dev libfribidi-dev libharfbuzz-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN curl -fsSLO "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz" \
 && echo "${FFMPEG_SHA256}  ffmpeg-${FFMPEG_VERSION}.tar.xz" | sha256sum -c - \
 && tar xf "ffmpeg-${FFMPEG_VERSION}.tar.xz"
WORKDIR /src/ffmpeg-${FFMPEG_VERSION}
RUN ./configure \
      --prefix=/opt/ffmpeg \
      --enable-gpl --enable-version3 \
      --enable-libx264 --enable-libvpx --enable-libopus --enable-libwebp \
      --enable-libass --enable-libfreetype --enable-libfontconfig \
      --enable-libfribidi --enable-libharfbuzz \
      --disable-doc --disable-debug --disable-ffplay \
      --extra-ldflags="-Wl,--disable-new-dtags -Wl,-rpath,/opt/ffmpeg/lib" \
 && make -j"$(nproc)" \
 && make install
# Copy every shared library the two binaries need, except glibc's (which the
# image they run in provides), so they run without any codec packages installed.
RUN mkdir -p /opt/ffmpeg/lib \
 && for bin in ffmpeg ffprobe; do ldd "/opt/ffmpeg/bin/$bin"; done \
    | awk '/=> \//{print $3}' | sort -u \
    | grep -Ev '/(libc|libm|libdl|libpthread|librt|libresolv|libutil)\.so' \
    | xargs -r -I{} cp -L {} /opt/ffmpeg/lib/ \
 && rm -rf /opt/ffmpeg/share /opt/ffmpeg/include /opt/ffmpeg/lib/pkgconfig

# Used by CI to prove the build works before it is published: starts from a
# bare image with no codec packages and runs what meme-arr runs.
FROM debian:trixie-slim AS check
RUN apt-get update \
 && apt-get install -y --no-install-recommends fontconfig fonts-dejavu-core \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /opt/ffmpeg /opt/ffmpeg
ENV PATH=/opt/ffmpeg/bin:$PATH
RUN set -eux; \
    ffmpeg -version | head -1 | tee /dev/stderr | grep -F "ffmpeg version 9."; \
    ffprobe -version | head -1; \
    ! ldd /opt/ffmpeg/bin/ffmpeg | grep "not found"; \
    enc="$(ffmpeg -hide_banner -encoders)"; \
    for e in libx264 libvpx-vp9 libopus libwebp aac gif; do echo "$enc" | grep -qw "$e"; done; \
    flt="$(ffmpeg -hide_banner -filters)"; \
    for f in subtitles drawtext palettegen paletteuse; do echo "$flt" | grep -qw "$f"; done; \
    cd /tmp; \
    printf '1\n00:00:00,000 --> 00:00:01,000\nhello\n' > t.srt; \
    ffmpeg -v error -f lavfi -i testsrc=duration=1:size=320x240 \
      -vf "subtitles=t.srt:force_style='Outline=2'" -c:v libx264 -pix_fmt yuv420p -y a.mp4; \
    ffmpeg -v error -f lavfi -i testsrc=duration=1:size=320x240 -f lavfi -i sine=duration=1 \
      -vf "drawtext=font=DejaVu Sans:text=hi:fontcolor=white" -c:v libvpx-vp9 \
      -c:a libopus -shortest -y b.webm; \
    ffmpeg -v error -f lavfi -i testsrc=duration=1:size=160x120 -c:v libwebp -y c.webp; \
    ffmpeg -v error -f lavfi -i testsrc=duration=1:size=160x120 \
      -vf "split[a][b];[a]palettegen[p];[b][p]paletteuse" -y d.gif; \
    for f in a.mp4 b.webm c.webp d.gif; do ffprobe -v error "$f"; done

# What gets published: just the files, for `COPY --from=`.
FROM scratch AS artifact
COPY --from=build /opt/ffmpeg /opt/ffmpeg
