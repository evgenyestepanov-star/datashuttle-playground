FROM rust:1.85-bookworm AS builder
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
