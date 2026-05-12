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
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system playground \
    && useradd --system --gid playground --create-home --home-dir /var/lib/playground playground
WORKDIR /opt/datashuttle
COPY --from=builder /build/target/release/datashuttle-playground-server /usr/local/bin/datashuttle-playground-server
COPY examples /opt/datashuttle/examples
USER playground:playground
EXPOSE 8081
ENV PLAYGROUND_BIND_ADDR=0.0.0.0:8081 \
    RUST_LOG=info
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/datashuttle-playground-server"]
