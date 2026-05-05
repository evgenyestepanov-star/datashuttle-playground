# datashuttle-playground

Standalone runtime, scenario manifest, and HTTP server for the DataShuttle
interactive playground.

The playground lets users pick a curated scenario (Postgres CDC, MySQL,
Kafka, etc.), spin up an isolated session, and trigger pre-reviewed
actions against a sidecar source database — without ever touching free-form
SQL. It ships separately from the core control plane because:

* **Different deployment cadence.** Manifest scenarios ship independently
  of core releases.
* **Different scaling shape.** Bursty demo traffic vs. steady pipeline
  workload.

## Layout

```
crates/
  datashuttle-playground/         foundation library: sessions, TCP
                                  dispatcher, quota tracker, prometheus
                                  metrics, manifest schema
  datashuttle-playground-server/  binary entrypoint
docker/                           Dockerfile + compose for local dev
helm/                             Helm chart for k8s deployment
examples/                         manifest.json + manifest.schema.json
```

## Quick start

### Local — Cargo

```sh
cargo run --bin datashuttle-playground-server
# listens on 0.0.0.0:8081
curl -s http://localhost:8081/health
curl -s http://localhost:8081/api/v1/playground/manifest | jq .
```

### Local — Docker Compose

```sh
docker compose -f docker/docker-compose.yml up --build
```

The compose file mounts `examples/manifest.json` into the image at
`/opt/datashuttle/examples/manifest.json` so the container picks it up
without extra config.

### Configuration

All settings come from environment variables:

| Variable                  | Default                          | Notes |
| ------------------------- | -------------------------------- | ----- |
| `PLAYGROUND_BIND_ADDR`    | `0.0.0.0:8081`                   | HTTP listen address |
| `PLAYGROUND_TOKEN`        | _unset_ (dev mode, no auth)      | When set, every non-probe request must include `Authorization: Bearer $PLAYGROUND_TOKEN` |
| `PLAYGROUND_MANIFEST`     | first match of `/opt/datashuttle/examples/manifest.json`, then `examples/manifest.json` | Path to the scenario manifest |
| `PLAYGROUND_TTL_SECS`     | `7200` (2 h)                     | Per-session TTL |
| `PLAYGROUND_QUOTA_PER_DAY`| `20`                             | Max session creations per tenant per UTC day |
| `RUST_LOG`                | `info`                           | tracing-subscriber filter |

`PLAYGROUND_TOKEN` unset means **no authentication**. That mode is for
local dev only — never expose an unauthenticated playground server to a
network the public can reach.

## HTTP surface

| Method | Path                              | Notes |
| ------ | --------------------------------- | ----- |
| `GET`  | `/health`                         | Liveness probe (no auth) |
| `GET`  | `/metrics`                        | Prometheus exposition (no auth) |
| `GET`  | `/api/v1/playground/manifest`     | Validated scenario manifest |
| `GET`  | `/api/v1/playground/health`       | Authenticated liveness check |

The full session-lifecycle surface (`POST /sessions`, `POST
/sessions/:id/actions/:action_id`, etc.) currently lives inside DataShuttle
api-core and depends on private types. Phase 5.B of the architecture
simplification plan introduces a public extension point in OSS so those
handlers can be ported into this binary without pulling in private
internals. Track Phase 5.B in the OSS `SIMPLIFICATION_PLAN.md`.

## Integrating with DataShuttle api

When Phase 5.B lands, DataShuttle's api will reverse-proxy
`/api/v1/playground/*` to a `datashuttle-playground-server` instance and
forward `Authorization: Bearer <PLAYGROUND_TOKEN>`. Until then api-core
serves the playground surface in-process; running this binary today is
useful for:

* Validating the standalone binary builds and runs.
* Exercising the manifest-load path.
* Setting up infrastructure (Helm, image registry) ahead of the cutover.

## Releases

Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`:

* `cargo publish -p datashuttle-playground` to crates.io (requires
  `CRATES_IO_TOKEN`).
* `docker buildx build --push` to `datashuttle/playground:<tag>` and
  `:latest` (requires `DOCKERHUB_USERNAME` / `DOCKERHUB_TOKEN`).

## Development

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## License

See [LICENSE](LICENSE).
