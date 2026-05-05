//! Phase 10.C.1 — Prometheus metrics for the interactive playground.
//!
//! Registered against the main `MetricsRegistry`'s Prometheus
//! `Registry` so `/metrics` picks them up alongside everything else.
//! Separated into this module because the metrics.rs monolith is
//! already ~1000 lines and tightly coupled to shuttle observability;
//! playground metrics evolve on a different cadence.
//!
//! Metrics owned here:
//!
//! * `datashuttle_playground_session_started_total{scenario, outcome}`
//!   — counter, one tick per POST /sessions result.
//! * `datashuttle_playground_session_active{tenant}` — gauge,
//!   re-sampled from the SessionManager by the metrics cron. Labels
//!   kept tenant-only to bound cardinality.
//! * `datashuttle_playground_action_duration_seconds{scenario, action, outcome}`
//!   — histogram, observed in `run_action` at the handler boundary.
//! * `datashuttle_playground_action_error_total{scenario, action, error_kind}`
//!   — counter, incremented alongside the histogram on the error path.
//! * `datashuttle_playground_teardown_duration_seconds{kind}` —
//!   histogram, observed in `teardown_session` (kind = "session") and
//!   the orphan sweeper (kind = "orphan").
//! * `datashuttle_playground_orphan_resources_reaped_total{protocol}`
//!   — counter, incremented once per orphan drop.

use prometheus::{HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry};

pub struct PlaygroundMetrics {
    session_started_total: IntCounterVec,
    session_active: IntGaugeVec,
    action_duration_seconds: HistogramVec,
    action_error_total: IntCounterVec,
    teardown_duration_seconds: HistogramVec,
    orphan_resources_reaped_total: IntCounterVec,
    /// Phase 10.C.3 — smoke-cron outcome counter. One tick per
    /// scenario per cron run, labeled by result (ok|error). Alert
    /// on `increase(...{result="error"}[30m]) > 0`.
    smoke_run_total: IntCounterVec,
}

impl std::fmt::Debug for PlaygroundMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaygroundMetrics").finish()
    }
}

impl PlaygroundMetrics {
    pub fn new(registry: &Registry) -> prometheus::Result<Self> {
        let session_started_total = IntCounterVec::new(
            Opts::new(
                "datashuttle_playground_session_started_total",
                "Playground session creation attempts, labeled by scenario and outcome (\"ok\" | \"denied\")",
            ),
            &["scenario", "outcome"],
        )?;
        let session_active = IntGaugeVec::new(
            Opts::new(
                "datashuttle_playground_session_active",
                "Currently-live playground sessions per tenant (sampled by the metrics cron)",
            ),
            &["tenant"],
        )?;
        // Action latency buckets cover 10ms–30s — playground actions
        // are typically sub-second SQL but shuttle provisioning can
        // stretch to tens of seconds on cold Iceberg commits.
        let action_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "datashuttle_playground_action_duration_seconds",
                "Duration of a playground action invocation in seconds",
            )
            .buckets(vec![
                0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
            ]),
            &["scenario", "action", "outcome"],
        )?;
        let action_error_total = IntCounterVec::new(
            Opts::new(
                "datashuttle_playground_action_error_total",
                "Playground action errors, labeled by scenario, action, and error_kind",
            ),
            &["scenario", "action", "error_kind"],
        )?;
        let teardown_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "datashuttle_playground_teardown_duration_seconds",
                "Time taken to tear down a session or reap an orphan, in seconds",
            )
            .buckets(vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]),
            &["kind"],
        )?;
        let orphan_resources_reaped_total = IntCounterVec::new(
            Opts::new(
                "datashuttle_playground_orphan_resources_reaped_total",
                "Orphan playground resources reaped by the hourly sweeper, labeled by protocol (postgres|mysql)",
            ),
            &["protocol"],
        )?;
        let smoke_run_total = IntCounterVec::new(
            Opts::new(
                "datashuttle_playground_smoke_run_total",
                "Playground smoke-cron runs per scenario, labeled by result (ok|error)",
            ),
            &["scenario", "result"],
        )?;

        registry.register(Box::new(session_started_total.clone()))?;
        registry.register(Box::new(session_active.clone()))?;
        registry.register(Box::new(action_duration_seconds.clone()))?;
        registry.register(Box::new(action_error_total.clone()))?;
        registry.register(Box::new(teardown_duration_seconds.clone()))?;
        registry.register(Box::new(orphan_resources_reaped_total.clone()))?;
        registry.register(Box::new(smoke_run_total.clone()))?;

        Ok(Self {
            session_started_total,
            session_active,
            action_duration_seconds,
            action_error_total,
            teardown_duration_seconds,
            orphan_resources_reaped_total,
            smoke_run_total,
        })
    }

    pub fn record_smoke_run(&self, scenario: &str, ok: bool) {
        let result = if ok { "ok" } else { "error" };
        self.smoke_run_total
            .with_label_values(&[scenario, result])
            .inc();
    }

    pub fn record_session_start(&self, scenario: &str, outcome: SessionStartOutcome) {
        self.session_started_total
            .with_label_values(&[scenario, outcome.as_str()])
            .inc();
    }

    pub fn set_session_active(&self, tenant: &str, value: i64) {
        self.session_active.with_label_values(&[tenant]).set(value);
    }

    pub fn observe_action_duration(
        &self,
        scenario: &str,
        action: &str,
        outcome: ActionOutcomeKind,
        duration: std::time::Duration,
    ) {
        self.action_duration_seconds
            .with_label_values(&[scenario, action, outcome.as_str()])
            .observe(duration.as_secs_f64());
    }

    pub fn record_action_error(&self, scenario: &str, action: &str, error_kind: ActionErrorKind) {
        self.action_error_total
            .with_label_values(&[scenario, action, error_kind.as_str()])
            .inc();
    }

    pub fn observe_teardown(&self, kind: TeardownKind, duration: std::time::Duration) {
        self.teardown_duration_seconds
            .with_label_values(&[kind.as_str()])
            .observe(duration.as_secs_f64());
    }

    pub fn record_orphan_reaped(&self, protocol: OrphanProtocol) {
        self.orphan_resources_reaped_total
            .with_label_values(&[protocol.as_str()])
            .inc();
    }
}

/// Typed label values — the string→label conversions live here so a
/// typo at a call site fails to compile instead of silently creating
/// a new Prometheus series.
#[derive(Debug, Clone, Copy)]
pub enum SessionStartOutcome {
    Ok,
    Denied,
}

impl SessionStartOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Denied => "denied",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ActionOutcomeKind {
    Ok,
    Err,
}

impl ActionOutcomeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Err => "err",
        }
    }
}

/// Stable error-kind labels for the action error counter. Each call
/// site must pick exactly one of these so alerts can partition
/// failures (e.g. a spike in `duplicate_key` is a scenario-design
/// problem; a spike in `connect` is a sidecar outage).
#[derive(Debug, Clone, Copy)]
pub enum ActionErrorKind {
    Connect,
    Auth,
    Timeout,
    DuplicateKey,
    SchemaMismatch,
    Protocol,
    Config,
    Other,
}

impl ActionErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Auth => "auth",
            Self::Timeout => "timeout",
            Self::DuplicateKey => "duplicate_key",
            Self::SchemaMismatch => "schema_mismatch",
            Self::Protocol => "protocol",
            Self::Config => "config",
            Self::Other => "other",
        }
    }

    /// Map the dispatcher's own error variants into a metric label.
    /// Keeps the mapping in one place; handler sites call this and
    /// pass the result to `record_action_error`. Always compiled now
    /// that `DispatchError` lives unconditionally in api (#829).
    pub fn from_dispatch_error(e: &crate::tcp::DispatchError) -> Self {
        use crate::tcp::DispatchError;
        match e {
            DispatchError::Config(_) => Self::Config,
            DispatchError::Connect(_) => Self::Connect,
            DispatchError::Auth(_) => Self::Auth,
            DispatchError::SchemaMismatch(_) => Self::SchemaMismatch,
            DispatchError::DuplicateKey(_) => Self::DuplicateKey,
            DispatchError::Timeout(_) => Self::Timeout,
            DispatchError::Protocol(_) => Self::Protocol,
            DispatchError::Unavailable => Self::Config,
        }
    }

    /// Best-effort mapping from a free-form error string. Used at the
    /// run_action boundary where most shell/HTTP paths return `String`
    /// rather than a typed error. Matches on lowercase substrings of
    /// the message so the label vocabulary stays stable across
    /// backends.
    pub fn from_message(msg: &str) -> Self {
        let m = msg.to_lowercase();
        if m.contains("duplicate") || m.contains("23505") || m.contains("1062") {
            Self::DuplicateKey
        } else if m.contains("timeout") || m.contains("57014") || m.contains("timed out") {
            Self::Timeout
        } else if m.contains("authentication")
            || m.contains("access denied")
            || m.contains("permission")
        {
            Self::Auth
        } else if m.contains("connect") || m.contains("refused") || m.contains("unreachable") {
            Self::Connect
        } else if m.contains("schema")
            || m.contains("undefined")
            || m.contains("no such")
            || m.contains("42p01")
            || m.contains("42703")
        {
            Self::SchemaMismatch
        } else if m.contains("config") || m.contains("misconfig") {
            Self::Config
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TeardownKind {
    Session,
    Orphan,
}

impl TeardownKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Orphan => "orphan",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OrphanProtocol {
    Postgres,
    Mysql,
}

impl OrphanProtocol {
    fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_mapping_is_stable() {
        assert_eq!(
            ActionErrorKind::from_message("duplicate key violation").as_str(),
            "duplicate_key"
        );
        assert_eq!(
            ActionErrorKind::from_message("connection refused").as_str(),
            "connect"
        );
        assert_eq!(
            ActionErrorKind::from_message("query timed out after 30s").as_str(),
            "timeout"
        );
        assert_eq!(
            ActionErrorKind::from_message("authentication failed").as_str(),
            "auth"
        );
        assert_eq!(
            ActionErrorKind::from_message("undefined column x").as_str(),
            "schema_mismatch"
        );
        assert_eq!(
            ActionErrorKind::from_message("random error").as_str(),
            "other"
        );
    }

    #[test]
    fn registration_does_not_double_register() {
        let reg = Registry::new();
        let m1 = PlaygroundMetrics::new(&reg).unwrap();
        // Second registration should FAIL on a shared registry — that's
        // the safeguard that catches an accidental double-wire.
        assert!(PlaygroundMetrics::new(&reg).is_err());
        // First handle still usable.
        m1.record_session_start("test", SessionStartOutcome::Ok);
    }
}
