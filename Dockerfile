# syntax=docker/dockerfile:1

# The ffmpeg that ships in the image: built once from docker/ffmpeg/Dockerfile and
# published by .github/workflows/ffmpeg-image.yml. Keep this equal to the
# FFMPEG_VERSION there (CI checks it).
ARG FFMPEG_VERSION=9.0.2
FROM ghcr.io/memearr/ffmpeg:${FFMPEG_VERSION} AS ffmpeg

# ---- 1. Build the SPA ----------------------------------------------------
FROM node:26-trixie-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json* ./
RUN npm install
COPY web/ ./
RUN npm run build

# ---- 2. Build the Rust binary -------------------------------------------
FROM rust:1-trixie AS build
WORKDIR /src
# Cache dependency compilation.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release 2>/dev/null || true
# Real sources.
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ---- 3. Runtime ----------------------------------------------------------
FROM debian:trixie-slim AS runtime
# ffmpeg is copied in below, not installed: it brings its own codec libraries and
# only needs glibc from here. fontconfig and a font are what drawtext/libass use.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        gifsicle \
        fontconfig \
        fonts-dejavu-core \
        ca-certificates \
        wget \
    && rm -rf /var/lib/apt/lists/*

COPY --from=ffmpeg /opt/ffmpeg /opt/ffmpeg
ENV PATH=/opt/ffmpeg/bin:$PATH

WORKDIR /app
COPY --from=build /src/target/release/meme-arr /app/meme-arr
COPY --from=web /web/dist /app/web
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod 0755 /app/docker-entrypoint.sh

# Fonts: the open-source caption fonts from assets/fonts (with their licenses), plus
# DejaVu Sans from Debian, which is the default. Users add more in Settings; mounting a
# folder over /app/fonts replaces this set (an Impact.ttf in it becomes the default).
COPY assets/fonts/ /app/fonts/
RUN mkdir -p /data /output \
    && cp /usr/share/fonts/truetype/dejavu/DejaVuSans*.ttf /app/fonts/ 2>/dev/null || true

# Build-time version (the release workflow passes the git tag here).
ARG APP_VERSION=dev
ENV APP_VERSION=$APP_VERSION

ENV BIND_ADDR=0.0.0.0:8080 \
    DATA_DIR=/data \
    OUTPUT_DIR=/output \
    WEB_DIR=/app/web \
    FONTS_DIR=/app/fonts \
    INDEX_ON_START=true \
    RUST_LOG=info

EXPOSE 8080
VOLUME ["/data", "/output"]

# Needs `wget` in this image (installed above): the slim base has neither wget nor
# curl, so this check used to fail on every container (reported as unhealthy).
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD ["/bin/sh", "-c", "wget -qO- http://localhost:8080/healthz || wget -qO- --no-check-certificate https://localhost:8080/healthz || exit 1"]

# Drops from root to PUID:PGID (default 1000:1000) after fixing volume
# ownership -- see docker-entrypoint.sh. PUID=0 keeps the old run-as-root
# behavior.
ENTRYPOINT ["/app/docker-entrypoint.sh"]
