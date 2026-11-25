//! CodeSecretary engine (`secretary-engine`): the only component that computes time.
//!
//! A background worker claims (user, local day) jobs from `processing_jobs`,
//! loads that day's events and the user's rules, correlates events to
//! projects, builds work sessions, and replaces the day's derived rows in one
//! transaction.
//!
//! - [`model`], [`correlate`], [`calculate`]: the pure, deterministic core.
//! - [`queue`], [`load`], [`write`], [`worker`]: Postgres I/O.
//! - [`health`], [`config`]: process plumbing.

pub mod calculate;
pub mod config;
pub mod correlate;
pub mod health;
pub mod load;
pub mod model;
pub mod queue;
pub mod worker;
pub mod write;

/// Bumped whenever the output for the same input changes (with golden
/// updates). Stored on every session and assignment; on startup the engine
/// recalculates the last 30 days that lack sessions of this version.
/// 2: user adjustments (EN-10) and manual sessions.
pub const ALGORITHM_VERSION: i32 = 2;
