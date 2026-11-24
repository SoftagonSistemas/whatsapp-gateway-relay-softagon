# syntax=docker/dockerfile:1.7

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
COPY src ./src

RUN cargo build --release --features whisper

FROM debian:bookworm-slim AS runtime

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
