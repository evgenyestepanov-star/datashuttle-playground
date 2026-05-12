//! Library re-export of the playground server modules so integration
//! tests (under `tests/`) can construct a `ServerState` and `Router`
//! without going through the binary entrypoint.
//!
//! The binary still drives `main.rs` directly; this lib target adds
//! no runtime surface beyond what's already public.

pub mod api_client;
pub mod config;
pub mod dispatcher;
pub mod handlers;
pub mod identity;
pub mod router;
