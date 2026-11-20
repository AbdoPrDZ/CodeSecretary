//! CodeSecretary contracts shared by the agent and the engine (`secretary-shared`).
//!
//! The single source of truth for what crosses a service boundary:
//!
//! - [`ids`]: newtype IDs; collectors generate UUIDv7 [`EventId`]s.
//! - [`event`]: the event envelope and the Phase 1 event type catalog
//!   (blueprint Appendix B).
//! - [`validate`]: envelope, timestamp, size and per-type rules (SH-4).
//! - [`redact`]: secret masking, URL stripping, remote normalization (SH-5, SH-6).
//! - [`config`]: `AgentConfig` (exclusions, pause windows) and the Appendix A defaults (SH-8).
//! - [`api`]: request/response bodies of the warehouse device API
//!   (blueprint Appendix C).
//!
//! The Python warehouse mirrors these types by hand until SH-9 generates a
//! JSON Schema from this crate; change both together.

pub mod api;
pub mod config;
pub mod event;
pub mod ids;
pub mod redact;
pub mod validate;

pub use event::{Event, Payload, RepositoryRef, SCHEMA_VERSION, Source};
pub use ids::EventId;
