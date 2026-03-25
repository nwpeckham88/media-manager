# syntax=docker/dockerfile:1.7

FROM node:25-bookworm AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN npm install -g pnpm@10.31.0 && pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:1.86-bookworm AS backend-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
ARG TARGETARCH
COPY ffmpeg/jellyfin-ffmpeg-rk3588.env /tmp/jellyfin-ffmpeg-rk3588.env
RUN apt-get update \
        && apt-get install -y --no-install-recommends ca-certificates curl mediainfo \
        && if [ "$TARGETARCH" = "arm64" ]; then \
                . /tmp/jellyfin-ffmpeg-rk3588.env; \
                curl -fL "$JELLYFIN_FFMPEG_URL" -o /tmp/jellyfin-ffmpeg.deb; \
                echo "$JELLYFIN_FFMPEG_SHA256  /tmp/jellyfin-ffmpeg.deb" | sha256sum -c -; \
                apt-get install -y --no-install-recommends /tmp/jellyfin-ffmpeg.deb; \
                ln -sf /usr/lib/jellyfin-ffmpeg/ffmpeg /usr/local/bin/ffmpeg; \
                ln -sf /usr/lib/jellyfin-ffmpeg/ffprobe /usr/local/bin/ffprobe; \
            else \
                apt-get install -y --no-install-recommends ffmpeg; \
                ln -sf /usr/bin/ffmpeg /usr/local/bin/ffmpeg; \
                ln -sf /usr/bin/ffprobe /usr/local/bin/ffprobe; \
            fi \
        && rm -f /tmp/jellyfin-ffmpeg.deb /tmp/jellyfin-ffmpeg-rk3588.env \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=backend-builder /app/target/release/media-manager /usr/local/bin/media-manager
COPY --from=frontend-builder /app/frontend/build /app/frontend/build

ENV MM_HOST=0.0.0.0
ENV MM_PORT=8080
ENV MM_STATE_DIR=/app/.mm-state
ENV MM_FFMPEG_PATH=/usr/local/bin/ffmpeg
ENV MM_FFPROBE_PATH=/usr/local/bin/ffprobe
ENV MM_MEDIAINFO_PATH=/usr/bin/mediainfo

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/media-manager"]
