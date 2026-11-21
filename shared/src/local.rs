//! Agent-local contracts: the status file for the panel (AG-14) and the
//! local API for editor extensions (AG-12, below).
//!
//! ## Status file (AG-14)
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
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
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
    pub git: GitStatus,
    pub local_api: LocalApiStatus,
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
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct StatusAccount {
    pub email: Option<String>,
    pub server_url: String,
    #[cfg_attr(feature = "codegen", ts(type = "number | null"))]
    pub device_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    pub last_ok_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    /// Short kind, e.g. `unreachable`, `HTTP 503`, `refused: device_revoked`.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct UploadStatus {
    /// Events waiting in the local buffer.
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub queued: u64,
    pub oldest_queued_at: Option<DateTime<Utc>>,
    pub last_upload_at: Option<DateTime<Utc>>,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub last_upload_events: u64,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub uploaded_since_start: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct HeartbeatStatus {
    pub last_at: Option<DateTime<Utc>>,
    /// Device clock minus server clock.
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub clock_offset_ms: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct PresenceStatus {
    /// `active`, `idle`, or `None` when the collector is unavailable.
    pub state: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub collector_available: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct GitStatus {
    /// Repositories found under the project roots and watched.
    pub repositories: usize,
    pub last_event_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct LocalApiStatus {
    pub listening: bool,
    /// Events accepted from extensions since the daemon started.
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub events_since_start: u64,
    pub last_event_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Local API (AG-12): how editor extensions and shell hooks hand events to the
// daemon.
//
// The daemon listens on `127.0.0.1` on a random free port and writes
// [`LocalApiFile`] to `<config>/local.json` (current user only). A client
// reads it, then calls the endpoints below with
// `Authorization: Bearer <its collector's token>`. Requests that carry an
// `Origin` header are refused, so web pages cannot reach the API.
// ---------------------------------------------------------------------------

/// File name inside the agent's config directory.
pub const LOCAL_API_FILE: &str = "local.json";
/// `POST`, body [`LocalEvents`], answer [`LocalEventsResponse`].
pub const LOCAL_EVENTS_PATH: &str = "/local/v1/events";
/// `GET`, answer [`LocalAgentState`].
pub const LOCAL_STATUS_PATH: &str = "/local/v1/status";
/// Largest request body, in bytes.
pub const MAX_LOCAL_BODY: usize = 1024 * 1024;
/// Most events per request.
pub const MAX_LOCAL_EVENTS: usize = 500;

/// Collector names with a token; each may send only its own event types
/// ([`collector_allows`]) with `source` = `<name>@<version>`.
pub const COLLECTORS: &[&str] = &["vscode"];

/// The event types `collector` may submit.
pub fn collector_allows(collector: &str, event_type: &str) -> bool {
    match collector {
        "vscode" => event_type.starts_with("editor."),
        _ => false,
    }
}

/// Contents of `local.json`. Rewritten at every daemon start (new port and
/// tokens) and removed on a clean stop.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct LocalApiFile {
    pub version: String,
    pub pid: u32,
    pub port: u16,
    /// Collector name to bearer token.
    pub tokens: std::collections::BTreeMap<String, String>,
    pub written_at: Option<DateTime<Utc>>,
}

/// `POST /local/v1/events`. Events use the shared envelope; the daemon
/// validates, filters and redacts them like its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
pub struct LocalEvents {
    #[cfg_attr(feature = "codegen", ts(type = "Event[]"))]
    pub events: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
pub struct LocalEventsResponse {
    /// Taken by the agent. Excluded or paused events count here too: the
    /// client must not retry them.
    pub accepted: u32,
    pub rejected: Vec<LocalRejection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
pub struct LocalRejection {
    /// Position in the request's `events`.
    pub index: u32,
    /// A `validate` reason code, `forbidden_event_type` or `source_mismatch`.
    pub reason: String,
}

/// `GET /local/v1/status`: what an extension shows in its status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(ts_rs::TS))]
#[serde(default)]
pub struct LocalAgentState {
    pub version: String,
    /// Inside a pause window: events are dropped.
    pub paused: bool,
    pub connection: ConnectionState,
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
    fn collectors_send_only_their_own_types() {
        assert!(collector_allows("vscode", "editor.save"));
        assert!(!collector_allows("vscode", "git.commit"));
        assert!(!collector_allows("vscode", "presence.active"));
        assert!(!collector_allows("unknown", "editor.save"));
        for name in COLLECTORS {
            assert!(
                crate::event::EVENT_TYPES
                    .iter()
                    .any(|t| collector_allows(name, t))
            );
        }
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
