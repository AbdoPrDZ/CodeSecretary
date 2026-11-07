//! CodeSecretary warehouse: the device-facing entry point and system of record.
//!
//! It authenticates devices, ingests and stores events, archives raw batches,
//! serves agent config, enqueues engine jobs, and owns every Postgres
//! migration. It never calculates time.

pub mod config;
pub mod db;
pub mod http;
pub mod telemetry;
