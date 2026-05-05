//! Playground runtime — sessions, TCP dispatcher, quota, metrics, and
//! manifest types for DataShuttle's interactive sandbox.
//!
//! Extracted from the OSS `datashuttle-playground` crate during Phase 5.A
//! of the architecture simplification (#999). This crate is the
//! foundation library: it owns the self-contained pieces (session manager,
//! dispatcher trait, quota tracker, prometheus metrics, manifest schema)
//! that have no coupling to private OSS internals.
//!
//! HTTP handlers and an api-side runtime adapter remain in OSS api-core
//! pending Phase 5.B, which will add a public extension point and let
//! the `datashuttle-playground-server` binary serve the full surface
//! standalone.

pub mod manifest;
pub mod metrics;
pub mod quota;
pub mod sessions;
pub mod tcp;

pub use manifest::Manifest;
