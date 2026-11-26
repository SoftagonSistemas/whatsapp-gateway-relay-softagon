# syntax=docker/dockerfile:1.7

FROM oven/bun:1-alpine AS frontend

WORKDIR /app/console
COPY console/package.json console/package-lock.json* ./
RUN bun install --frozen-lockfile || bun install
COPY console ./
RUN bun run build

FROM rust:1.94-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    clang \
    libclang-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock* rust-toolchain.toml ./
COPY migrations ./migrations
COPY openapi ./openapi
COPY --from=frontend /app/console/dist ./console/dist
COPY src ./src

RUN cargo build --release --features whisper

FROM debian:bookworm-slim AS runtime

ARG VERSION=0.3.6
ARG REVISION=dev
ARG CREATED=unknown

LABEL org.opencontainers.image.title="Softagon Relay" \
      org.opencontainers.image.description="Open-source, self-hosted WhatsApp Web messaging gateway built with Rust, SQLite, S3 media storage, and local Whisper transcription." \
      org.opencontainers.image.url="https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon" \
      org.opencontainers.image.source="https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon" \
      org.opencontainers.image.documentation="https://github.com/SoftagonSistemas/whatsapp-gateway-relay-softagon#readme" \
      org.opencontainers.image.vendor="Softagon Sistemas" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.created="${CREATED}"

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libstdc++6 \
    libgomp1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin relay \
    && mkdir -p /data/media /data/models /data/tmp \
    && chown -R relay:relay /data

COPY --from=builder /app/target/release/softagon-relay /usr/local/bin/softagon-relay

USER relay
WORKDIR /data

ENV APP_HOST=0.0.0.0 \
    APP_PORT=8080 \
    DATA_DIR=/data

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/softagon-relay"]
