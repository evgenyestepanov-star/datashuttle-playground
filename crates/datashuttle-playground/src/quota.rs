//! Phase 10.B.4 — playground abuse-prevention quotas.
//!
//! Caps a tenant's playground session *creations* to
//! `MAX_SESSIONS_PER_TENANT_PER_DAY` in a rolling UTC-day window. This
//! is the single knob that stops a bad actor from looping
//! session-create → session-delete to burn sidecar resources; the
//! per-user "one active session" rule in `SessionManager::create`
//! handles the live-concurrency case.
//!
//! Backed by a per-pod in-memory map. Acceptable because:
//!   * Quota is per-tenant-per-day — worst case a tenant can create
//!     `MAX * pods` sessions if traffic fan-outs across pods, which
//!     still caps the blast radius well below what's abusive.
//!   * The `kv` distributed-KV migration tracked for 9.4 sudo/write
//!     counters will cover this store too.
//!
//! Anonymous / no-tenant requests use a dedicated slot (`""`) which
//! falls under the same cap. On deployments without multi-tenancy
//! enabled that's effectively a single global cap — the right
//! behavior.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, NaiveDate, Utc};

/// Per-tenant daily cap on session creations. Sized so a demo user
/// can iterate freely (each session ~2-5 minutes of work) while a
/// script-driven abuser gets blocked after roughly one shuttle-hour.
pub const MAX_SESSIONS_PER_TENANT_PER_DAY: u32 = 20;

/// Sliding-window is too heavy for a quota that resets at UTC
/// midnight; a simple `HashMap<(tenant, date) -> count>` does the job.
/// Entries older than the current UTC date are pruned on every check
/// so memory doesn't grow unboundedly across days.
pub struct PlaygroundQuotaTracker {
    state: Mutex<HashMap<String, DailyRecord>>,
    /// Override for tests so a single UTC day doesn't cap the whole
    /// test run. `None` in production means "use MAX_SESSIONS..."
    max_per_day: u32,
}

#[derive(Debug, Clone, Copy)]
struct DailyRecord {
    day: NaiveDate,
    count: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum QuotaError {
    #[error(
        "playground daily quota exhausted for tenant ({tenant}): {count}/{limit} — resets at UTC midnight"
    )]
    DailyLimit {
        tenant: String,
        count: u32,
        limit: u32,
    },
}

impl PlaygroundQuotaTracker {
    pub fn new() -> Self {
        Self::with_limit(MAX_SESSIONS_PER_TENANT_PER_DAY)
    }

    /// Used by tests (and the `DS_PLAYGROUND_MAX_SESSIONS_PER_DAY`
    /// env override the state constructor applies at boot).
    pub fn with_limit(max_per_day: u32) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            max_per_day,
        }
    }

    pub fn max_per_day(&self) -> u32 {
        self.max_per_day
    }

    /// Charge one session against the (tenant, today) slot. Returns
    /// the post-increment count on success, `QuotaError::DailyLimit`
    /// when exhausted. Inspects & mutates under a single mutex —
    /// the contention envelope is `O(create-session RPS)` which is
    /// tiny in practice.
    pub fn try_consume(&self, tenant: Option<&str>) -> Result<u32, QuotaError> {
        self.try_consume_at(tenant, Utc::now())
    }

    /// Time-injected variant for tests so we can simulate day
    /// rollover without waiting.
    pub fn try_consume_at(
        &self,
        tenant: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<u32, QuotaError> {
        let today = now.date_naive();
        let key = tenant.unwrap_or("").to_string();
        let mut map = self.state.lock().unwrap();
        // Sweep stale entries from other days. Cheap even at a few
        // thousand tenants — runs on every create, which is already
        // rate-limited by the session flow itself.
        map.retain(|_, rec| rec.day == today);
        let entry = map.entry(key.clone()).or_insert(DailyRecord {
            day: today,
            count: 0,
        });
        if entry.count >= self.max_per_day {
            return Err(QuotaError::DailyLimit {
                tenant: key,
                count: entry.count,
                limit: self.max_per_day,
            });
        }
        entry.count += 1;
        Ok(entry.count)
    }

    /// Count for (tenant, today) without mutation — used by the
    /// admin /playground/quotas readout.
    pub fn peek(&self, tenant: Option<&str>) -> u32 {
        let today = Utc::now().date_naive();
        let key = tenant.unwrap_or("");
        let map = self.state.lock().unwrap();
        map.get(key)
            .filter(|rec| rec.day == today)
            .map(|rec| rec.count)
            .unwrap_or(0)
    }
}

impl Default for PlaygroundQuotaTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn first_creation_is_1() {
        let q = PlaygroundQuotaTracker::new();
        assert_eq!(q.try_consume(Some("tenant-a")).unwrap(), 1);
    }

    #[test]
    fn separate_tenants_have_separate_buckets() {
        let q = PlaygroundQuotaTracker::with_limit(2);
        assert_eq!(q.try_consume(Some("a")).unwrap(), 1);
        assert_eq!(q.try_consume(Some("a")).unwrap(), 2);
        // a is now at cap but b is untouched
        assert!(q.try_consume(Some("a")).is_err());
        assert_eq!(q.try_consume(Some("b")).unwrap(), 1);
    }

    #[test]
    fn anonymous_uses_empty_string_slot() {
        let q = PlaygroundQuotaTracker::with_limit(1);
        assert_eq!(q.try_consume(None).unwrap(), 1);
        assert!(q.try_consume(None).is_err());
        // Named tenant is independent
        assert_eq!(q.try_consume(Some("named")).unwrap(), 1);
    }

    #[test]
    fn day_rollover_resets_count() {
        let q = PlaygroundQuotaTracker::with_limit(1);
        let day1 = Utc.with_ymd_and_hms(2026, 4, 18, 23, 30, 0).unwrap();
        let day2 = Utc.with_ymd_and_hms(2026, 4, 19, 0, 30, 0).unwrap();
        assert_eq!(q.try_consume_at(Some("t"), day1).unwrap(), 1);
        assert!(q.try_consume_at(Some("t"), day1).is_err());
        // Next day: count reset
        assert_eq!(q.try_consume_at(Some("t"), day2).unwrap(), 1);
    }

    #[test]
    fn peek_reflects_current_day_count() {
        let q = PlaygroundQuotaTracker::with_limit(5);
        assert_eq!(q.peek(Some("t")), 0);
        q.try_consume(Some("t")).unwrap();
        q.try_consume(Some("t")).unwrap();
        assert_eq!(q.peek(Some("t")), 2);
    }

    #[test]
    fn error_carries_limit_context() {
        let q = PlaygroundQuotaTracker::with_limit(1);
        q.try_consume(Some("tenant-x")).unwrap();
        let err = q.try_consume(Some("tenant-x")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("tenant-x"));
        assert!(msg.contains("1/1"));
        assert!(msg.contains("UTC midnight"));
    }
}
