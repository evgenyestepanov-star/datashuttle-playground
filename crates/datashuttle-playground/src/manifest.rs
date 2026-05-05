//! Playground manifest types and loader.
//!
//! The playground manifest (`examples/manifest.json`) is the single source of
//! truth for interactive scenarios surfaced in:
//!
//! - The web UI `/playground` page (gallery and runner)
//! - The `datashuttle playground` CLI subcommand (when wired)
//! - The mdbook docs under `docs/playground.md`
//!
//! The schema is pinned by `examples/manifest.schema.json` (JSON Schema draft 2020-12).
//! These Rust types are the authoritative view consumed by the playground server
//! to serve `GET /api/v1/playground/manifest` and to validate action IDs against
//! the whitelist for `POST /api/v1/playground/sessions/:id/actions/:action_id`.
//!
//! Vendored from `datashuttle-core::playground` during the Phase 5.A extraction
//! so the public playground crate has no dependency on private OSS crates.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Top-level manifest.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub scenarios: Vec<Scenario>,
}

/// A data source (database, queue, file store, etc.) backed by a free/OSS
/// docker container.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
    pub status: SourceStatus,
    pub free: bool,
    #[serde(default)]
    pub docker_service: Option<String>,
    #[serde(default)]
    pub docker_profile: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Cdc,
    Snapshot,
    Streaming,
    File,
    Rest,
    Kv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Stable,
    Beta,
    Excluded,
}

/// A scenario runs one shuttle against one source with a curated set of
/// actions the user can trigger from the UI.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scenario {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub description: String,
    pub difficulty: Difficulty,
    pub tier: u8,
    pub status: ScenarioStatus,
    #[serde(default)]
    pub tags: Vec<String>,
    pub prerequisites: Prerequisites,
    #[serde(default)]
    pub init_sql: Option<String>,
    #[serde(default)]
    pub shuttle_sql: Option<String>,
    #[serde(default)]
    pub teardown_sql: Option<String>,
    #[serde(default)]
    pub estimated_duration_s: Option<u32>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub break_scenarios: Vec<Action>,
    #[serde(default)]
    pub expected_outcomes: Vec<ExpectedOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Beginner,
    Intermediate,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioStatus {
    Stable,
    Beta,
    Hidden,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Prerequisites {
    #[serde(default)]
    pub deployment: Vec<Deployment>,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub min_memory_mb: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Deployment {
    Dev,
    SelfManaged,
    Cloud,
}

/// A discrete action a user can trigger from the UI during a scenario
/// (e.g. "Insert order", "Drop column", "10% packet loss").
///
/// Actions are the only server-side whitelist for arbitrary source
/// operations — free-form SQL from the playground UI is rejected. Each
/// action references a pre-reviewed SQL file or shell script shipped in
/// `examples/`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    pub kind: ActionKind,
    #[serde(default)]
    pub target: Option<ActionTarget>,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub sql_file: Option<String>,
    #[serde(default)]
    pub shell_cmd: Option<String>,
    #[serde(default)]
    pub http_request: Option<HttpRequest>,
    #[serde(default)]
    pub payload_file: Option<String>,
    #[serde(default)]
    pub repeat: Option<u32>,
    #[serde(default)]
    pub expected_dlq: Option<bool>,
    #[serde(default)]
    pub destructive: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    Sql,
    Shell,
    Http,
    ProduceKafka,
    UploadFile,
    Toxiproxy,
    ResetSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionTarget {
    Source,
    Target,
    Kafka,
    S3,
    Rest,
    Ops,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExpectedOutcome {
    pub metric: String,
    pub assertion: String,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("reading manifest at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing manifest at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("validation failed: {0}")]
    Validation(String),
}

impl Manifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|source| ManifestError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, ManifestError> {
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|source| ManifestError::Parse {
                path: PathBuf::from("<memory>"),
                source,
            })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != 1 {
            return Err(ManifestError::Validation(format!(
                "manifest version {} not supported (expected 1)",
                self.version
            )));
        }

        let mut source_ids: HashSet<&str> = HashSet::new();
        for s in &self.sources {
            if !source_ids.insert(&s.id) {
                return Err(ManifestError::Validation(format!(
                    "duplicate source id: {}",
                    s.id
                )));
            }
        }

        let mut scenario_ids: HashSet<&str> = HashSet::new();
        for sc in &self.scenarios {
            if !scenario_ids.insert(&sc.id) {
                return Err(ManifestError::Validation(format!(
                    "duplicate scenario id: {}",
                    sc.id
                )));
            }
            if !source_ids.contains(sc.source_id.as_str()) {
                return Err(ManifestError::Validation(format!(
                    "scenario {} references unknown source_id {}",
                    sc.id, sc.source_id
                )));
            }

            let mut action_ids: HashSet<&str> = HashSet::new();
            for a in sc.actions.iter().chain(sc.break_scenarios.iter()) {
                if !action_ids.insert(&a.id) {
                    return Err(ManifestError::Validation(format!(
                        "scenario {} has duplicate action id {}",
                        sc.id, a.id
                    )));
                }
                validate_action(&sc.id, a)?;
            }
        }

        Ok(())
    }

    pub fn scenario(&self, id: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.id == id)
    }

    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }
}

fn validate_action(scenario_id: &str, a: &Action) -> Result<(), ManifestError> {
    let missing = match a.kind {
        ActionKind::Sql => a.sql.is_none() && a.sql_file.is_none(),
        ActionKind::Http => a.http_request.is_none(),
        ActionKind::Shell | ActionKind::Toxiproxy | ActionKind::ResetSnapshot => {
            a.shell_cmd.is_none()
        }
        ActionKind::ProduceKafka | ActionKind::UploadFile => a.payload_file.is_none(),
    };
    if missing {
        return Err(ManifestError::Validation(format!(
            "scenario {} action {} is missing the payload required for kind {:?}",
            scenario_id, a.id, a.kind
        )));
    }
    Ok(())
}

impl Scenario {
    pub fn allowed_action_ids(&self) -> HashSet<&str> {
        self.actions
            .iter()
            .chain(self.break_scenarios.iter())
            .map(|a| a.id.as_str())
            .collect()
    }

    pub fn action(&self, id: &str) -> Option<&Action> {
        self.actions
            .iter()
            .chain(self.break_scenarios.iter())
            .find(|a| a.id == id)
    }

    pub fn allowed_in(&self, deployment: Deployment) -> bool {
        self.prerequisites.deployment.is_empty()
            || self.prerequisites.deployment.contains(&deployment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> Manifest {
        serde_json::from_value(serde_json::json!({
            "version": 1,
            "sources": [{
                "id": "postgres",
                "name": "PostgreSQL",
                "kind": "cdc",
                "status": "stable",
                "free": true
            }],
            "scenarios": [{
                "id": "s1",
                "source_id": "postgres",
                "title": "t",
                "description": "d",
                "difficulty": "beginner",
                "tier": 1,
                "status": "stable",
                "prerequisites": { "deployment": ["dev"] },
                "actions": [{
                    "id": "a1",
                    "label": "A1",
                    "kind": "sql",
                    "sql": "SELECT 1"
                }]
            }]
        }))
        .unwrap()
    }

    #[test]
    fn validates_happy_path() {
        let m = valid_manifest();
        m.validate().unwrap();
        assert_eq!(m.scenario("s1").unwrap().allowed_action_ids().len(), 1);
    }

    #[test]
    fn rejects_unknown_source_ref() {
        let mut m = valid_manifest();
        m.scenarios[0].source_id = "nope".into();
        let e = m.validate().unwrap_err();
        assert!(matches!(e, ManifestError::Validation(_)));
    }

    #[test]
    fn rejects_duplicate_scenario_id() {
        let mut m = valid_manifest();
        m.scenarios.push(m.scenarios[0].clone());
        let e = m.validate().unwrap_err();
        assert!(matches!(e, ManifestError::Validation(_)));
    }

    #[test]
    fn rejects_sql_action_without_body() {
        let mut m = valid_manifest();
        m.scenarios[0].actions[0].sql = None;
        m.scenarios[0].actions[0].sql_file = None;
        let e = m.validate().unwrap_err();
        assert!(matches!(e, ManifestError::Validation(_)));
    }

    #[test]
    fn deployment_gate() {
        let m = valid_manifest();
        let s = m.scenario("s1").unwrap();
        assert!(s.allowed_in(Deployment::Dev));
        assert!(!s.allowed_in(Deployment::Cloud));
    }

    #[test]
    fn parses_real_manifest() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/manifest.json");
        let bytes = std::fs::read(&path).expect("read examples/manifest.json");
        let m = Manifest::parse(&bytes).expect("parse manifest");
        assert!(!m.sources.is_empty());
        assert!(!m.scenarios.is_empty());
        for sc in &m.scenarios {
            assert!(m.source(&sc.source_id).is_some(), "{}", sc.source_id);
        }
    }
}
