# syntax=docker/dockerfile:1.7

FROM node:25-bookworm AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN corepack enable && corepack prepare pnpm@10.31.0 --activate && pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:1.86-bookworm AS backend-builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg mediainfo \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=backend-builder /app/target/release/media-manager /usr/local/bin/media-manager
COPY --from=frontend-builder /app/frontend/build /app/frontend/build

ENV MM_HOST=0.0.0.0
ENV MM_PORT=8080
ENV MM_STATE_DIR=/app/.mm-state

EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/media-manager"]
