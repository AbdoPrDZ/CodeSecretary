//! Agent-local status (AG-14): what the daemon tells the status panel.
//!
//! The daemon rewrites `<data>/status.json` ([`STATUS_FILE`]) every
//! [`STATUS_EVERY_SECS`] seconds and once on shutdown (`running = false`),
//! atomically (temp file + rename). The panel only reads it, so the daemon
//! needs no server or port, and a closed panel costs nothing.
//!
//! Contents are states, counts and times; never event metadata (titles,
//! paths, URLs). Every field has a default, so panel and daemon versions may
//! differ.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// File name inside the agent's data directory.
pub const STATUS_FILE: &str = "status.json";
/// How often the daemon rewrites it.
pub const STATUS_EVERY_SECS: u64 = 5;
/// A status older than this means the daemon is not running (or hung).
pub const STALE_AFTER_SECS: i64 = 20;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentStatus {
    pub version: String,
    pub pid: u32,
    pub started_at: Option<DateTime<Utc>>,
    pub written_at: Option<DateTime<Utc>>,
    /// False in the last write before a clean shutdown.
    pub running: bool,
    pub account: Option<StatusAccount>,
    pub connection: ConnectionStatus,
    pub upload: UploadStatus,
    pub heartbeat: HeartbeatStatus,
    pub presence: PresenceStatus,
    pub config: ConfigStatus,
}

impl AgentStatus {
    /// Running and written recently.
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.running
            && self
                .written_at
                .is_some_and(|at| (now - at).num_seconds() <= STALE_AFTER_SECS)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusAccount {
    pub email: Option<String>,
    pub server_url: String,
    pub device_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// No request made yet.
    #[default]
    Unknown,
    /// The last warehouse request succeeded.
    Online,
    /// The last request failed (network, 5xx); the agent keeps retrying.
    Offline,
    /// Not signed in, or the session/device was revoked: run `login`.
    SignedOut,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    /// Short kind, e.g. `unreachable`, `HTTP 503`, `refused: device_revoked`.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UploadStatus {
    /// Events waiting in the local buffer.
    pub queued: u64,
    pub oldest_queued_at: Option<DateTime<Utc>>,
    pub last_upload_at: Option<DateTime<Utc>>,
    pub last_upload_events: u64,
    pub uploaded_since_start: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeartbeatStatus {
    pub last_at: Option<DateTime<Utc>>,
    /// Device clock minus server clock.
    pub clock_offset_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PresenceStatus {
    /// `active`, `idle`, or `None` when the collector is unavailable.
    pub state: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub collector_available: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigStatus {
    pub fetched_at: Option<DateTime<Utc>>,
    pub path_exclusions: usize,
    pub app_exclusions: usize,
    pub domain_exclusions: usize,
    pub pause_windows: usize,
    /// Inside a pause window now.
    pub paused: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tolerant_parsing() {
        let now: DateTime<Utc> = "2026-11-20T12:00:00Z".parse().unwrap();
        let status = AgentStatus {
            version: "0.1.0".into(),
            running: true,
            written_at: Some(now),
            connection: ConnectionStatus {
                state: ConnectionState::Online,
                ..Default::default()
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(serde_json::from_str::<AgentStatus>(&json).unwrap(), status);
        assert!(json.contains(r#""state":"online""#));
        // Older/newer daemons: unknown fields ignored, missing ones default.
        let partial: AgentStatus = serde_json::from_str(r#"{"running":true,"future":1}"#).unwrap();
        assert_eq!(partial.connection.state, ConnectionState::Unknown);
    }

    #[test]
    fn liveness_needs_a_recent_write() {
        let now: DateTime<Utc> = "2026-11-20T12:00:00Z".parse().unwrap();
        let mut status = AgentStatus {
            running: true,
            written_at: Some(now - chrono::Duration::seconds(5)),
            ..Default::default()
        };
        assert!(status.is_live(now));
        status.written_at = Some(now - chrono::Duration::seconds(STALE_AFTER_SECS + 1));
        assert!(!status.is_live(now));
        status.written_at = Some(now);
        status.running = false;
        assert!(!status.is_live(now));
    }
}
