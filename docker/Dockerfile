# Phase 5.C bumped Rust from 1.85 → 1.94: post-port transitive deps
# (home@0.5.12, icu_*@2.2, time@0.3.47, idna_adapter@1.2) require
# rustc >= 1.86; the OSS Dockerfile is already on 1.94 and pinning
# to the same toolchain keeps a `cargo update --workspace` resolution
# consistent across the OSS + playground workspaces.
FROM rust:1.94-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY examples ./examples
RUN cargo build --release --bin datashuttle-playground-server
RUN strip target/release/datashuttle-playground-server || true

FROM debian:bookworm-slim
# Two third-party CLIs are bundled into the runtime image:
#
# * `rpk` — produce_kafka shells out to it for idempotent topic create
#   before the first POST to Pandaproxy. Pandaproxy's REST produce does
#   NOT auto-create topics (returns UNKNOWN_TOPIC_OR_PARTITION on first
#   call) and Redpanda's admin API has no topic-CRUD route either.
#
# * `mc` (MinIO client) — upload_file shells out to it to copy
#   scenario payloads into the cloud stack's MinIO. Direct sigv4 PUT
#   from reqwest would mean pulling in aws-sdk-s3 which adds minutes
#   to the playground build; `mc` is a single static binary.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini curl unzip \
    && ARCH="$(dpkg --print-architecture)" \
    && case "$ARCH" in \
         amd64) RPK_ARCH=amd64; MC_ARCH=amd64 ;; \
         arm64) RPK_ARCH=arm64; MC_ARCH=arm64 ;; \
         *) echo "unsupported arch $ARCH"; exit 1 ;; \
       esac \
    && curl -fsSL "https://github.com/redpanda-data/redpanda/releases/download/v24.2.7/rpk-linux-${RPK_ARCH}.zip" -o /tmp/rpk.zip \
    && unzip -q /tmp/rpk.zip -d /usr/local/bin \
    && rm /tmp/rpk.zip \
    && curl -fsSL "https://dl.min.io/client/mc/release/linux-${MC_ARCH}/mc" -o /usr/local/bin/mc \
    && chmod +x /usr/local/bin/mc \
    && apt-get purge -y --auto-remove unzip \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system playground \
    && useradd --system --gid playground --create-home --home-dir /var/lib/playground playground \
    && install -d -o playground -g playground /var/lib/datashuttle-playground
WORKDIR /opt/datashuttle
COPY --from=builder /build/target/release/datashuttle-playground-server /usr/local/bin/datashuttle-playground-server
COPY examples /opt/datashuttle/examples
USER playground:playground
EXPOSE 8081
ENV PLAYGROUND_BIND_ADDR=0.0.0.0:8081 \
    RUST_LOG=info
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/datashuttle-playground-server"]
