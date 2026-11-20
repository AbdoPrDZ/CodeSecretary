//! Warehouse device API bodies (blueprint Appendix C, WAREHOUSE plan §4).
//!
//! Every `/v1/...` call except [`AuthConfig`] carries a Keycloak access token
//! (`Authorization: Bearer …`). Errors are `{"error": "<code>"}`
//! ([`ApiError`]) with the codes in [`error_code`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::event::Event;
use crate::ids::EventId;

/// At most this many events per batch.
pub const MAX_BATCH_EVENTS: usize = 1000;
/// At most this many bytes per batch body, as sent (gzip-compressed).
pub const MAX_BATCH_BYTES: usize = 1024 * 1024;

/// `GET /v1/auth/config`: where the agent signs in (D14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Keycloak realm issuer, e.g. `https://id.example.com/realms/secretary`.
    pub issuer: String,
    /// OIDC client the agent uses (`secretary-agent`).
    pub client_id: String,
}

/// `POST /v1/devices`: register this device for the token's Keycloak session.
/// Idempotent per session; re-sending updates name/OS/version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterDevice {
    pub name: String,
    pub os: String,
    pub agent_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRegistered {
    pub device_id: i64,
}

/// `POST /v1/events:batch` body (gzip JSON, `Content-Encoding: gzip`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchRequest {
    /// Random per upload attempt; names the raw archive object (WH-6).
    pub batch_id: Uuid,
    /// Device clock minus server clock, in milliseconds (see [`HeartbeatResponse`]).
    pub clock_offset_ms: i64,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchResponse {
    /// Stored now.
    pub accepted: Vec<EventId>,
    /// Already stored earlier; safe to forget.
    pub duplicates: Vec<EventId>,
    /// Invalid; will never be accepted, so the agent drops them too.
    pub rejected: Vec<Rejection>,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rejection {
    /// As sent; may be absent or malformed when that is the reason.
    #[serde(default)]
    pub event_id: Option<String>,
    /// Short machine-readable reason, e.g. `invalid_timestamps`.
    pub reason: String,
}

/// `POST /v1/agent/heartbeat`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub agent_version: String,
    pub queue_depth: u64,
    pub clock_offset_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    /// Server clock when the request was handled; the agent derives its
    /// clock offset from this and the request round trip.
    pub server_time: DateTime<Utc>,
}

/// Error body of every non-2xx `/v1/...` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

/// Values of [`ApiError::error`].
pub mod error_code {
    /// 401: token missing, malformed, expired, forged, or for another audience/client.
    pub const INVALID_TOKEN: &str = "invalid_token";
    /// 401: the device was revoked from the dashboard; sign in again.
    pub const DEVICE_REVOKED: &str = "device_revoked";
    /// 403: valid token, but `POST /v1/devices` has not been called for its session.
    pub const DEVICE_NOT_REGISTERED: &str = "device_not_registered";
    /// 413: more than [`MAX_BATCH_EVENTS`](super::MAX_BATCH_EVENTS) events or bytes.
    pub const BATCH_TOO_LARGE: &str = "batch_too_large";
    /// 400: body is not (gzip) JSON of the expected shape.
    pub const INVALID_BODY: &str = "invalid_body";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Empty, Payload, Source};

    #[test]
    fn batch_round_trip() {
        let request = BatchRequest {
            batch_id: Uuid::now_v7(),
            clock_offset_ms: 250,
            events: vec![Event::new(
                Source::new("agent", "0.1.0"),
                "2026-11-07T09:00:00Z".parse().unwrap(),
                Some("2026-11-07T09:01:00Z".parse().unwrap()),
                Payload::PresenceIdle(Empty {}),
            )],
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<BatchRequest>(&json).unwrap(),
            request
        );
    }

    #[test]
    fn batch_response_parses_warehouse_shape() {
        let json = r#"{"accepted":["019a5e1c-0000-7000-8000-000000000001"],"duplicates":[],
            "rejected":[{"event_id":null,"reason":"invalid_body"}],
            "server_time":"2026-11-07T09:00:00.123456Z"}"#;
        let response: BatchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.accepted.len(), 1);
        assert_eq!(response.rejected[0].reason, "invalid_body");
    }
}
