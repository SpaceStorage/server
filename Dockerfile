# syntax=docker/dockerfile:1
# Multi-stage build for the three-node Compose lab.
# Compose uses pull_policy: build so the image is built when the stack starts.

FROM rust:1.88-bookworm AS builder

WORKDIR /src

# Prefer Cargo.toml rust-version over an older rust-toolchain.toml pin.
ENV RUSTUP_TOOLCHAIN=1.88.0
RUN rustup toolchain install 1.88.0 && rustup default 1.88.0

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src ./src

RUN cargo build --release -p spacestoraged -p spacestorage \
  && strip target/release/spacestoraged target/release/spacestorage

FROM debian:bookworm-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/spacestoraged /usr/local/bin/spacestoraged
COPY --from=builder /src/target/release/spacestorage /usr/local/bin/spacestorage
COPY docker/three-node /etc/spacestorage/three-node
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh \
  && mkdir -p /etc/spacestorage/secrets /var/lib/spacestorage

VOLUME ["/etc/spacestorage/secrets", "/var/lib/spacestorage"]

ENTRYPOINT ["/entrypoint.sh"]
