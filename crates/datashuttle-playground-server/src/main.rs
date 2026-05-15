//! `datashuttle-playground-server` — standalone HTTP entrypoint.
//!
//! Phase 5.C completed the playground extraction. The binary now boots
//! the full session-lifecycle handler suite (create, reset, end,
//! execute action) ported from OSS api-core, alongside the source-side
//! TCP dispatcher and the OSS api callback client.

use std::sync::Arc;

use anyhow::Context;
use datashuttle_playground::manifest::Manifest;
use datashuttle_playground::metrics::PlaygroundMetrics;
use datashuttle_playground::quota::PlaygroundQuotaTracker;
use datashuttle_playground::sessions::SessionManager;
use datashuttle_playground::tcp::PlaygroundDispatcher;
use prometheus::Registry;
use tracing::{info, warn};

use datashuttle_playground_server::api_client::ApiClient;
use datashuttle_playground_server::config;
use datashuttle_playground_server::dispatcher;
use datashuttle_playground_server::router::{router, ServerState};

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

    // Persist session map to <data_dir>/playground/sessions.json so the
    // TTL reaper survives a playground container restart. Without this
    // the in-memory map starts empty after boot and the api-side
    // artifacts those sessions created (shuttles, connections, Iceberg
    // namespaces, source-side schemas) orphan permanently — only an
    // explicit DELETE /sessions/:id from a live client cleans them.
    let sessions = if let Some(m) = manifest.as_ref() {
        Some(
            SessionManager::new_with_persistence(m.clone(), true, cfg.session_ttl, &cfg.data_dir)
                .await,
        )
    } else {
        None
    };

    let quota = Arc::new(PlaygroundQuotaTracker::with_limit(
        cfg.session_quota_per_day,
    ));

    // Source dispatcher (postgres + mysql TCP pools). Lazy-init
    // internally — unused pools pay zero connection cost at boot.
    let dispatcher: Arc<dyn PlaygroundDispatcher> = Arc::new(dispatcher::build_dispatcher());

    // OSS api callback client. Optional — when either env var is
    // missing we boot without it and handlers that need it return
    // 503. Lets a partial deploy still expose health/manifest.
    let api_client: Option<Arc<ApiClient>> =
        match (cfg.api_base_url.as_ref(), cfg.api_service_token.as_ref()) {
            (Some(base), Some(token)) => {
                match ApiClient::new(base.clone(), token.clone(), cfg.api_timeout) {
                    Ok(c) => {
                        info!(base_url = %base, "playground: api callback client configured");
                        Some(Arc::new(c))
                    }
                    Err(e) => {
                        warn!(error = %e, "playground: failed to build api callback client");
                        None
                    }
                }
            }
            _ => {
                warn!(
                    "PLAYGROUND_API_BASE_URL or PLAYGROUND_SERVICE_TOKEN is unset — \
                 session create / reset / SQL actions will return 503 until \
                 both are provided."
                );
                None
            }
        };

    let state = Arc::new(ServerState {
        config: cfg.clone(),
        manifest,
        sessions,
        quota,
        metrics,
        prom_registry,
        dispatcher,
        api_client,
    });

    // Reap TTL-expired sessions every 60s. Without this an expired
    // session sits in memory until the user (or admin) explicitly
    // ends it, and its catalog namespace + parquet files accumulate
    // in the warehouse forever. The interval is deliberately shorter
    // than the default session TTL so a session that expires at
    // T can be cleaned up by T+60s.
    datashuttle_playground_server::handlers::spawn_session_reaper(
        Arc::clone(&state),
        std::time::Duration::from_secs(60),
    );

    // One-shot orphan sweep against the api registry — catches any
    // shuttles/connections/namespaces left over from sessions that
    // existed before persistence was enabled OR whose sessions.json
    // was deleted (volume rebuild, etc.). Spawned in background so
    // server start isn't gated on the api being reachable yet.
    tokio::spawn(datashuttle_playground_server::handlers::sweep_api_orphans(
        Arc::clone(&state),
    ));

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
