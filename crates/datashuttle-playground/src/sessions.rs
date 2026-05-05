//! In-memory session store for the interactive playground (M20).
//!
//! Sessions are short-lived (default 2h TTL), one active per authenticated
//! user, and carry enough state to replay scenario actions and clean up
//! on expiry. State is intentionally held in-memory only — if the API
//! restarts, sessions expire cleanly and the background sweeper reaps any
//! orphan namespaces on the next boot.
//!
//! Isolation model: each session owns a namespace `playground_<uhash>_<sid>`
//! inside the configured Iceberg catalog. The shuttle created for the
//! session writes exclusively into that namespace; deletion tears the
//! namespace (and all its tables) down.

use crate::manifest::Manifest;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;

/// Default session lifetime. Admins can override globally via the
/// `DS_PLAYGROUND_TTL_SECS` environment variable consumed by `new_manager`.
pub const DEFAULT_TTL: Duration = Duration::from_secs(2 * 60 * 60);

/// Minimum allowed TTL — 5 minutes. Anything shorter is almost always a
/// user mistake.
pub const MIN_TTL: Duration = Duration::from_secs(5 * 60);

/// Maximum allowed TTL — 8 hours. Prevents accidental resource squatting.
pub const MAX_TTL: Duration = Duration::from_secs(8 * 60 * 60);

/// Cadence for the background sweep task.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// How frequently actions may be invoked on a single session (rough
/// rate-limit to keep abusive loops at bay; the UI honors this too).
pub const ACTION_COOLDOWN: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// Shuttle is being provisioned.
    Provisioning,
    /// Session is live — actions may be invoked.
    Active,
    /// Reset in progress (shuttle rewinding snapshot).
    Resetting,
    /// Session is being torn down.
    Terminating,
    /// Terminal state — session no longer accepts actions.
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub at: DateTime<Utc>,
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}

/// Live session state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub user_id: String,
    pub tenant_id: Option<String>,
    pub scenario_id: String,
    pub namespace: String,
    pub shuttle_name: String,
    pub connection_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// Monotonic clock anchor — not serialized (no point persisting a
    /// per-process clock origin). Re-anchored on hydrate to "now"
    /// minus elapsed-from-`created_at`, so `is_expired()` keeps
    /// honouring `expires_at` after restart.
    #[serde(skip, default = "default_instant_now")]
    pub created_monotonic: Instant,
    pub ttl: Duration,
    pub status: SessionStatus,
    /// Monotonic, like `created_monotonic`. After hydrate the field is
    /// `None`, which only affects rate-limiting on the next call —
    /// acceptable since the cooldown is 1s.
    #[serde(skip)]
    pub last_action_at: Option<Instant>,
    pub events: Vec<SessionEvent>,
}

fn default_instant_now() -> Instant {
    Instant::now()
}

/// Read sessions from a previously-persisted JSON file. Returns the
/// `(sessions, per_user)` pair the manager will hold; the file is
/// silently treated as empty if missing or corrupt — the caller can
/// always start with an empty map and the orphan sweeper's cleanup
/// pass will catch any drift.
async fn hydrate_from_disk(
    path: &std::path::Path,
) -> (HashMap<Uuid, Session>, HashMap<String, Uuid>) {
    let bytes = match tokio::fs::read(path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (HashMap::new(), HashMap::new());
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "playground SessionManager: hydrate read failed; starting fresh"
            );
            return (HashMap::new(), HashMap::new());
        }
    };
    let snapshot: Vec<Session> = match serde_json::from_slice(&bytes) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "playground SessionManager: hydrate parse failed; starting fresh"
            );
            return (HashMap::new(), HashMap::new());
        }
    };
    let now = Utc::now();
    let mut sessions: HashMap<Uuid, Session> = HashMap::new();
    let mut per_user: HashMap<String, Uuid> = HashMap::new();
    let mut dropped_expired = 0usize;
    for s in snapshot {
        if s.expires_at <= now || matches!(s.status, SessionStatus::Ended) {
            dropped_expired += 1;
            continue;
        }
        per_user.insert(s.user_id.clone(), s.id);
        sessions.insert(s.id, s);
    }
    if !sessions.is_empty() || dropped_expired > 0 {
        tracing::info!(
            hydrated = sessions.len(),
            dropped_expired,
            path = %path.display(),
            "playground SessionManager: hydrated from disk"
        );
    }
    (sessions, per_user)
}

impl Session {
    pub fn new(
        user_id: String,
        tenant_id: Option<String>,
        scenario_id: String,
        ttl: Duration,
    ) -> Self {
        let id = Uuid::new_v4();
        let namespace = derive_namespace(&user_id, &id);
        let shuttle_name = format!("pg_{}_{}", short_user(&user_id), short_sid(&id));
        let connection_name = format!("{shuttle_name}_src");
        let now = Utc::now();
        let expires_at =
            now + chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::hours(2));
        Self {
            id,
            user_id,
            tenant_id,
            scenario_id,
            namespace,
            shuttle_name,
            connection_name,
            created_at: now,
            expires_at,
            created_monotonic: Instant::now(),
            ttl,
            status: SessionStatus::Provisioning,
            last_action_at: None,
            events: Vec::new(),
        }
    }

    pub fn is_expired(&self) -> bool {
        // Cross-check both clocks: monotonic catches "ttl elapsed in
        // this process", wall clock catches "we hydrated a session
        // whose original deadline already passed". Both must agree
        // the session is alive — if either says expired, expire it.
        self.created_monotonic.elapsed() > self.ttl || Utc::now() >= self.expires_at
    }

    pub fn record(
        &mut self,
        kind: &str,
        message: String,
        action_id: Option<String>,
        success: Option<bool>,
    ) {
        self.events.push(SessionEvent {
            at: Utc::now(),
            kind: kind.to_string(),
            message,
            action_id,
            success,
        });
        const MAX_EVENTS: usize = 500;
        if self.events.len() > MAX_EVENTS {
            let drop = self.events.len() - MAX_EVENTS;
            self.events.drain(0..drop);
        }
    }

    pub fn extend(&mut self, extra: Duration) -> Result<(), &'static str> {
        let remaining = self
            .ttl
            .checked_sub(self.created_monotonic.elapsed())
            .unwrap_or(Duration::ZERO);
        let new_total = remaining + extra;
        if new_total > MAX_TTL {
            return Err("extension would exceed max TTL");
        }
        self.ttl = self.created_monotonic.elapsed() + new_total;
        self.expires_at = Utc::now()
            + chrono::Duration::from_std(new_total).unwrap_or(chrono::Duration::hours(1));
        Ok(())
    }
}

/// Public projection — the shape returned by the HTTP API.
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: Uuid,
    pub scenario_id: String,
    pub status: SessionStatus,
    pub namespace: String,
    pub shuttle_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub ttl_seconds: u64,
    pub remaining_seconds: u64,
    pub events: Vec<SessionEvent>,
}

impl From<&Session> for SessionView {
    fn from(s: &Session) -> Self {
        let remaining = s
            .ttl
            .checked_sub(s.created_monotonic.elapsed())
            .unwrap_or(Duration::ZERO)
            .as_secs();
        SessionView {
            id: s.id,
            scenario_id: s.scenario_id.clone(),
            status: s.status,
            namespace: s.namespace.clone(),
            shuttle_name: s.shuttle_name.clone(),
            created_at: s.created_at,
            expires_at: s.expires_at,
            ttl_seconds: s.ttl.as_secs(),
            remaining_seconds: remaining,
            events: s.events.clone(),
        }
    }
}

// --------------------------------------------------------------------- manager

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session limit reached for user (already holds {0})")]
    UserLimit(Uuid),
    #[error("session not found: {0}")]
    NotFound(Uuid),
    #[error("session is owned by a different user")]
    Forbidden,
    #[error("action cooldown active — retry in {0:?}")]
    Cooldown(Duration),
    #[error("invalid TTL: {0}")]
    InvalidTtl(String),
    #[error("playground is disabled by config")]
    Disabled,
    #[error("unknown scenario: {0}")]
    UnknownScenario(String),
}

pub struct SessionManager {
    sessions: RwLock<HashMap<Uuid, Session>>,
    per_user: RwLock<HashMap<String, Uuid>>,
    manifest: Arc<Manifest>,
    enabled: bool,
    default_ttl: Duration,
    /// JSON file under `<data_dir>/playground/sessions.json` where the
    /// in-memory map is mirrored after every mutation. Without this
    /// the orphan-sweeper reaps schemas of any session that survived
    /// an api restart, since the in-memory map starts empty. `None`
    /// in tests / OSS deployments without playground enabled.
    persistence_path: Option<std::path::PathBuf>,
}

impl SessionManager {
    pub fn new(manifest: Arc<Manifest>, enabled: bool, default_ttl: Duration) -> Arc<Self> {
        let ttl = clamp_ttl(default_ttl).unwrap_or(DEFAULT_TTL);
        Arc::new(Self {
            sessions: RwLock::new(HashMap::new()),
            per_user: RwLock::new(HashMap::new()),
            manifest,
            enabled,
            default_ttl: ttl,
            persistence_path: None,
        })
    }

    /// Like [`Self::new`] but hydrates the session map from
    /// `<data_dir>/playground/sessions.json` first and mirrors every
    /// subsequent mutation back to the same file. Already-expired
    /// rows on disk are dropped during hydrate.
    pub async fn new_with_persistence(
        manifest: Arc<Manifest>,
        enabled: bool,
        default_ttl: Duration,
        data_dir: impl AsRef<std::path::Path>,
    ) -> Arc<Self> {
        let ttl = clamp_ttl(default_ttl).unwrap_or(DEFAULT_TTL);
        let path = data_dir.as_ref().join("playground").join("sessions.json");
        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let (sessions, per_user) = hydrate_from_disk(&path).await;
        Arc::new(Self {
            sessions: RwLock::new(sessions),
            per_user: RwLock::new(per_user),
            manifest,
            enabled,
            default_ttl: ttl,
            persistence_path: Some(path),
        })
    }

    /// Snapshot the current sessions map and write it to the
    /// persistence file (if configured). Best-effort: a write failure
    /// logs and is dropped — losing one snapshot is recoverable on
    /// next mutation.
    async fn persist(&self) {
        let Some(path) = self.persistence_path.as_ref() else {
            return;
        };
        let snapshot: Vec<Session> = {
            let map = self.sessions.read().await;
            map.values().cloned().collect()
        };
        let json = match serde_json::to_vec_pretty(&snapshot) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "playground SessionManager: serialize failed; skipping persist");
                return;
            }
        };
        // Atomic-ish write: temp + rename so a crash mid-write
        // doesn't leave a half-truncated file we'd then refuse to
        // parse on next boot.
        let tmp = path.with_extension("json.tmp");
        if let Err(e) = tokio::fs::write(&tmp, &json).await {
            tracing::warn!(error = %e, path = %path.display(), "playground SessionManager: tmp write failed");
            return;
        }
        if let Err(e) = tokio::fs::rename(&tmp, path).await {
            tracing::warn!(error = %e, path = %path.display(), "playground SessionManager: rename failed");
        }
    }

    pub fn manifest(&self) -> &Arc<Manifest> {
        &self.manifest
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn default_ttl(&self) -> Duration {
        self.default_ttl
    }

    /// Create a session for `user_id` running `scenario_id`. Rejects when
    /// the user already holds an active session, when the scenario is
    /// unknown, or when the playground is globally disabled.
    pub async fn create(
        &self,
        user_id: &str,
        tenant_id: Option<String>,
        scenario_id: &str,
        ttl_override: Option<Duration>,
    ) -> Result<Session, SessionError> {
        if !self.enabled {
            return Err(SessionError::Disabled);
        }
        if self.manifest.scenario(scenario_id).is_none() {
            return Err(SessionError::UnknownScenario(scenario_id.into()));
        }
        let ttl = match ttl_override {
            Some(t) => clamp_ttl(t)?,
            None => self.default_ttl,
        };
        // Lock order: sessions → per_user. Keep it consistent with
        // `end()` and `sweep_expired()` so no future third-lock
        // refactor can slip into a deadlock.
        let mut sessions = self.sessions.write().await;
        let mut per_user = self.per_user.write().await;
        if let Some(existing) = per_user.get(user_id).copied() {
            // Verify the recorded session is still alive; clean up the
            // pointer if it has silently expired between sweeps.
            if let Some(sess) = sessions.get(&existing) {
                if !matches!(sess.status, SessionStatus::Ended) && !sess.is_expired() {
                    return Err(SessionError::UserLimit(existing));
                }
            }
            per_user.remove(user_id);
        }
        let session = Session::new(user_id.to_string(), tenant_id, scenario_id.to_string(), ttl);
        let id = session.id;
        per_user.insert(user_id.to_string(), id);
        sessions.insert(id, session.clone());
        drop(per_user);
        drop(sessions);
        self.persist().await;
        Ok(session)
    }

    pub async fn get(&self, id: Uuid, user_id: &str) -> Result<Session, SessionError> {
        let sessions = self.sessions.read().await;
        let s = sessions.get(&id).ok_or(SessionError::NotFound(id))?;
        if s.user_id != user_id {
            return Err(SessionError::Forbidden);
        }
        Ok(s.clone())
    }

    /// Mutate a session with a closure under the write lock.
    pub async fn update<F, R>(&self, id: Uuid, user_id: &str, f: F) -> Result<R, SessionError>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let result = {
            let mut sessions = self.sessions.write().await;
            let s = sessions.get_mut(&id).ok_or(SessionError::NotFound(id))?;
            if s.user_id != user_id {
                return Err(SessionError::Forbidden);
            }
            f(s)
        };
        self.persist().await;
        Ok(result)
    }

    /// Mark as terminating, remove from maps, return the removed session
    /// so the caller can run teardown work outside the lock.
    pub async fn end(&self, id: Uuid, user_id: &str) -> Result<Session, SessionError> {
        let mut sessions = self.sessions.write().await;
        let mut per_user = self.per_user.write().await;
        let s = sessions.get(&id).ok_or(SessionError::NotFound(id))?;
        if s.user_id != user_id {
            return Err(SessionError::Forbidden);
        }
        let uid = s.user_id.clone();
        if per_user.get(&uid) == Some(&id) {
            per_user.remove(&uid);
        }
        let mut s = sessions.remove(&id).unwrap();
        s.status = SessionStatus::Ended;
        drop(per_user);
        drop(sessions);
        self.persist().await;
        Ok(s)
    }

    /// Throttle action invocations per-session.
    pub async fn touch_action(&self, id: Uuid, user_id: &str) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        let s = sessions.get_mut(&id).ok_or(SessionError::NotFound(id))?;
        if s.user_id != user_id {
            return Err(SessionError::Forbidden);
        }
        if let Some(last) = s.last_action_at {
            let since = last.elapsed();
            if since < ACTION_COOLDOWN {
                return Err(SessionError::Cooldown(ACTION_COOLDOWN - since));
            }
        }
        s.last_action_at = Some(Instant::now());
        Ok(())
    }

    /// Sweep expired sessions. Returns the ids of sessions removed so the
    /// caller can run teardown work (drop namespace, delete shuttle).
    pub async fn sweep_expired(&self) -> Vec<Session> {
        let mut expired = Vec::new();
        let mut sessions = self.sessions.write().await;
        let mut per_user = self.per_user.write().await;
        let ids: Vec<Uuid> = sessions
            .iter()
            .filter(|(_, s)| s.is_expired())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(mut s) = sessions.remove(&id) {
                if per_user.get(&s.user_id) == Some(&id) {
                    per_user.remove(&s.user_id);
                }
                s.status = SessionStatus::Ended;
                expired.push(s);
            }
        }
        let removed_anything = !expired.is_empty();
        drop(per_user);
        drop(sessions);
        if removed_anything {
            self.persist().await;
        }
        expired
    }

    /// Active session count. Used by the /usage endpoint and tests.
    pub async fn active_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Best-effort lookup across all users — admin use only.
    pub async fn admin_get(&self, id: Uuid) -> Option<Session> {
        self.sessions.read().await.get(&id).cloned()
    }

    /// Snapshot every live session's `namespace`. Used by the orphan
    /// sweeper (10.B.3) to cross-reference against schemas/databases
    /// sitting on the playground sidecars.
    pub async fn live_namespaces(&self) -> std::collections::HashSet<String> {
        self.sessions
            .read()
            .await
            .values()
            .map(|s| s.namespace.clone())
            .collect()
    }

    /// Phase 10.B.5 — snapshot every live session's `shuttle_name`.
    /// The orphan sweeper uses this to derive the set of currently-
    /// valid publication + replication-slot names so it can reap
    /// the stragglers without touching live sessions' artifacts.
    pub async fn live_shuttles(&self) -> std::collections::HashSet<String> {
        self.sessions
            .read()
            .await
            .values()
            .map(|s| s.shuttle_name.clone())
            .collect()
    }

    /// Look up the live session for `user_id` if any.
    pub async fn get_user_session(&self, user_id: &str) -> Option<Session> {
        let per_user = self.per_user.read().await;
        let id = per_user.get(user_id).copied()?;
        drop(per_user);
        self.sessions.read().await.get(&id).cloned()
    }
}

// --------------------------------------------------------------------- helpers

fn clamp_ttl(t: Duration) -> Result<Duration, SessionError> {
    if t < MIN_TTL {
        return Err(SessionError::InvalidTtl(format!(
            "must be at least {}s",
            MIN_TTL.as_secs()
        )));
    }
    if t > MAX_TTL {
        return Err(SessionError::InvalidTtl(format!(
            "must be at most {}s",
            MAX_TTL.as_secs()
        )));
    }
    Ok(t)
}

fn short_user(user_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(user_id.as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..4])
}

fn short_sid(id: &Uuid) -> String {
    // First 4 bytes of the UUID as 8 hex chars.
    let bytes = id.as_bytes();
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3]
    )
}

/// Derive the Iceberg namespace used for a session. Deterministic so
/// background sweeps on API restart can reconcile orphan namespaces.
pub fn derive_namespace(user_id: &str, session_id: &Uuid) -> String {
    format!(
        "playground_{}_{}",
        short_user(user_id),
        short_sid(session_id)
    )
}

// hex encoding helper (avoid adding hex crate — tiny and stable).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        const HEX: &[u8] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;

    fn manifest_with_scenario() -> Arc<Manifest> {
        let m: Manifest = serde_json::from_value(serde_json::json!({
            "version": 1,
            "sources": [{ "id": "postgres", "name": "p", "kind": "cdc", "status": "stable", "free": true }],
            "scenarios": [{
                "id": "s1",
                "source_id": "postgres",
                "title": "t",
                "description": "d",
                "difficulty": "beginner",
                "tier": 1,
                "status": "stable",
                "prerequisites": { "deployment": ["dev"] },
                "actions": [{ "id": "a1", "label": "A1", "kind": "sql", "sql": "SELECT 1" }]
            }]
        })).unwrap();
        Arc::new(m)
    }

    #[tokio::test]
    async fn rejects_when_disabled() {
        let mgr = SessionManager::new(manifest_with_scenario(), false, DEFAULT_TTL);
        let e = mgr.create("u1", None, "s1", None).await.unwrap_err();
        assert!(matches!(e, SessionError::Disabled));
    }

    #[tokio::test]
    async fn one_session_per_user() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
        let first = mgr.create("u1", None, "s1", None).await.unwrap();
        let err = mgr.create("u1", None, "s1", None).await.unwrap_err();
        assert!(matches!(err, SessionError::UserLimit(id) if id == first.id));
    }

    #[tokio::test]
    async fn second_user_succeeds() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
        mgr.create("u1", None, "s1", None).await.unwrap();
        mgr.create("u2", None, "s1", None).await.unwrap();
        assert_eq!(mgr.active_count().await, 2);
    }

    #[tokio::test]
    async fn rejects_unknown_scenario() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
        let err = mgr.create("u1", None, "nope", None).await.unwrap_err();
        assert!(matches!(err, SessionError::UnknownScenario(_)));
    }

    #[tokio::test]
    async fn get_enforces_ownership() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
        let s = mgr.create("u1", None, "s1", None).await.unwrap();
        let err = mgr.get(s.id, "other").await.unwrap_err();
        assert!(matches!(err, SessionError::Forbidden));
    }

    #[tokio::test]
    async fn sweep_removes_expired() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, MIN_TTL);
        let s = mgr.create("u1", None, "s1", None).await.unwrap();
        // Forcibly expire by rewriting TTL
        mgr.update(s.id, "u1", |s| s.ttl = Duration::from_millis(1))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let swept = mgr.sweep_expired().await;
        assert_eq!(swept.len(), 1);
        assert_eq!(mgr.active_count().await, 0);
    }

    /// #71 — sessions survive a SessionManager rebuild (simulates an
    /// api restart) when persistence is enabled. The hydrated map
    /// drops sessions whose `expires_at` already passed and keeps the
    /// rest, so the orphan sweeper's namespace cross-check sees the
    /// same set the previous process held.
    #[tokio::test]
    async fn persist_then_hydrate_round_trips_active_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = SessionManager::new_with_persistence(
            manifest_with_scenario(),
            true,
            DEFAULT_TTL,
            dir.path(),
        )
        .await;
        let s1 = mgr.create("u1", None, "s1", None).await.unwrap();
        let s2 = mgr.create("u2", None, "s1", None).await.unwrap();
        assert_eq!(mgr.active_count().await, 2);

        // Drop the old manager and rebuild from the same directory.
        drop(mgr);
        let mgr2 = SessionManager::new_with_persistence(
            manifest_with_scenario(),
            true,
            DEFAULT_TTL,
            dir.path(),
        )
        .await;
        assert_eq!(mgr2.active_count().await, 2);
        let live: std::collections::HashSet<String> = mgr2.live_namespaces().await;
        assert!(live.contains(&s1.namespace));
        assert!(live.contains(&s2.namespace));

        // End one session — the file mirrors the new state.
        mgr2.end(s1.id, "u1").await.unwrap();
        let mgr3 = SessionManager::new_with_persistence(
            manifest_with_scenario(),
            true,
            DEFAULT_TTL,
            dir.path(),
        )
        .await;
        assert_eq!(mgr3.active_count().await, 1);
        assert!(mgr3.live_namespaces().await.contains(&s2.namespace));
    }

    /// Already-expired rows on disk are dropped during hydrate so
    /// the next orphan sweeper pass cleans up their stranded
    /// schemas like usual.
    #[tokio::test]
    async fn hydrate_drops_expired_entries() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = SessionManager::new_with_persistence(
            manifest_with_scenario(),
            true,
            DEFAULT_TTL,
            dir.path(),
        )
        .await;
        let s = mgr.create("u1", None, "s1", None).await.unwrap();
        // Backdate `expires_at` directly (skipping the persist path
        // a real session never would, but the hydrate filter has to
        // cope) and persist.
        mgr.update(s.id, "u1", |s| {
            s.expires_at = Utc::now() - chrono::Duration::seconds(1);
        })
        .await
        .unwrap();
        drop(mgr);
        let mgr2 = SessionManager::new_with_persistence(
            manifest_with_scenario(),
            true,
            DEFAULT_TTL,
            dir.path(),
        )
        .await;
        assert_eq!(mgr2.active_count().await, 0);
    }

    #[tokio::test]
    async fn namespace_is_deterministic() {
        let uid = "alice@example.org";
        let sid = Uuid::parse_str("ffffeeee-dddd-cccc-bbbb-aaaaaaaaaaaa").unwrap();
        let ns1 = derive_namespace(uid, &sid);
        let ns2 = derive_namespace(uid, &sid);
        assert_eq!(ns1, ns2);
        assert!(ns1.starts_with("playground_"));
    }

    #[tokio::test]
    async fn ttl_clamping() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
        let err = mgr
            .create("u1", None, "s1", Some(Duration::from_secs(10)))
            .await
            .unwrap_err();
        assert!(matches!(err, SessionError::InvalidTtl(_)));
    }

    #[tokio::test]
    async fn action_cooldown() {
        let mgr = SessionManager::new(manifest_with_scenario(), true, DEFAULT_TTL);
        let s = mgr.create("u1", None, "s1", None).await.unwrap();
        mgr.touch_action(s.id, "u1").await.unwrap();
        let err = mgr.touch_action(s.id, "u1").await.unwrap_err();
        assert!(matches!(err, SessionError::Cooldown(_)));
    }
}
