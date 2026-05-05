//! `datashuttle-playground-server` — standalone HTTP entrypoint.
//!
//! The full session-lifecycle handler suite (create, reset, end, execute
//! action) currently lives inside the OSS api crate because of its
//! coupling to private types. Phase 5.B will lift that coupling via a
//! public extension point and reintroduce the full surface here. Today
//! the server boots a minimal router (health, manifest read, prometheus
//! metrics) so deployments and CI can validate the binary end-to-end.

use std::sync::Arc;

use anyhow::Context;
use datashuttle_playground::manifest::Manifest;
use datashuttle_playground::metrics::PlaygroundMetrics;
use datashuttle_playground::quota::PlaygroundQuotaTracker;
use datashuttle_playground::sessions::SessionManager;
use prometheus::Registry;
use tracing::{info, warn};

mod config;
mod router;

use crate::router::{router, ServerState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = config::load().context("loading playground server config")?;
    info!(
        bind_addr = %cfg.bind_addr(),
        manifest = ?cfg.manifest_path,
        auth_required = cfg.auth_token.is_some(),
        "starting datashuttle-playground-server",
    );
    if cfg.auth_token.is_none() {
        warn!(
            "PLAYGROUND_TOKEN is unset — running in dev mode with no \
             authentication. Set PLAYGROUND_TOKEN before exposing this \
             server beyond a trusted network."
        );
    }

    let prom_registry = Arc::new(Registry::new());
    let metrics = Arc::new(
        PlaygroundMetrics::new(&prom_registry)
            .context("registering playground prometheus metrics")?,
    );

    let manifest = load_manifest(cfg.manifest_path.as_deref())?.map(Arc::new);

    let sessions = manifest
        .as_ref()
        .map(|m| SessionManager::new(m.clone(), true, cfg.session_ttl));

    let quota = Arc::new(PlaygroundQuotaTracker::with_limit(
        cfg.session_quota_per_day,
    ));

    let state = Arc::new(ServerState {
        config: cfg.clone(),
        manifest,
        sessions,
        quota,
        metrics,
        prom_registry,
    });

    let app = router(state);

    let listener = tokio::net::TcpListener::bind(cfg.bind_addr())
        .await
        .with_context(|| format!("binding {}", cfg.bind_addr()))?;
    info!(addr = %cfg.bind_addr(), "listening");
    axum::serve(listener, app)
        .await
        .context("axum server crashed")?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::filter::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .json()
        .init();
}

fn load_manifest(explicit: Option<&std::path::Path>) -> anyhow::Result<Option<Manifest>> {
    let candidates: Vec<std::path::PathBuf> = match explicit {
        Some(p) => vec![p.to_path_buf()],
        None => vec![
            std::path::PathBuf::from("/opt/datashuttle/examples/manifest.json"),
            std::path::PathBuf::from("examples/manifest.json"),
        ],
    };

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        match Manifest::load(path) {
            Ok(m) => {
                info!(path = %path.display(), scenarios = m.scenarios.len(), "manifest loaded");
                return Ok(Some(m));
            }
            Err(e) => {
                warn!(path = %path.display(), "manifest invalid — skipping: {e}");
            }
        }
    }
    Ok(None)
}
