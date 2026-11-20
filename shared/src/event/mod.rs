//! Event envelope (blueprint §9.2) and the Phase 1 event catalog (Appendix B).
//!
//! Wire format (JSON), one object per event:
//!
//! ```json
//! {
//!   "event_id": "019a5e1c-…",          // UUIDv7, from the collector
//!   "schema_version": 1,
//!   "timestamp_start": "2026-11-07T09:00:00Z",
//!   "timestamp_end": "2026-11-07T09:05:00Z",   // null for point events
//!   "source": "agent@0.1.0",
//!   "event_type": "presence.active",
//!   "project_hint": null,
//!   "repository": null,                  // {"root_path": …, "remote_url": …}
//!   "working_directory": null,
//!   "metadata": {}
//! }
//! ```
//!
//! `user_id` and `device_id` are deliberately absent: the warehouse takes
//! them from the access token (`sub` → user, `sid` → device), never from the
//! payload.

mod metadata;

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use metadata::*;

use crate::ids::EventId;

/// Version of the envelope and metadata shapes. The warehouse accepts N and
/// N-1 once a second version exists (SH-22).
pub const SCHEMA_VERSION: u16 = 1;

/// One activity event, already redacted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event_id: EventId,
    pub schema_version: u16,
    pub timestamp_start: DateTime<Utc>,
    #[serde(default)]
    pub timestamp_end: Option<DateTime<Utc>>,
    pub source: Source,
    #[serde(default)]
    pub project_hint: Option<String>,
    #[serde(default)]
    pub repository: Option<RepositoryRef>,
    #[serde(default)]
    pub working_directory: Option<String>,
    /// `event_type` + `metadata`.
    #[serde(flatten)]
    pub payload: Payload,
}

impl Event {
    /// A new event with a fresh UUIDv7 and no project/repository context.
    pub fn new(
        source: Source,
        timestamp_start: DateTime<Utc>,
        timestamp_end: Option<DateTime<Utc>>,
        payload: Payload,
    ) -> Self {
        Self {
            event_id: EventId::new(),
            schema_version: SCHEMA_VERSION,
            timestamp_start,
            timestamp_end,
            source,
            project_hint: None,
            repository: None,
            working_directory: None,
            payload,
        }
    }

    pub fn event_type(&self) -> &'static str {
        self.payload.event_type()
    }
}

/// Collector name and version, serialized as `name@version` (e.g. `vscode@0.3.1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub version: String,
}

impl Source {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

impl FromStr for Source {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('@') {
            Some((name, version)) if !name.is_empty() && !version.is_empty() => {
                Ok(Self::new(name, version))
            }
            _ => Err(format!("source must be name@version, got {s:?}")),
        }
    }
}

impl Serialize for Source {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Repository context: local root and/or normalized remote (`github.com/acme/api`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRef {
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default)]
    pub remote_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn envelope_wire_format_is_flat() {
        let event = Event::new(
            Source::new("agent", "0.1.0"),
            ts("2026-11-07T09:00:00Z"),
            Some(ts("2026-11-07T09:05:00Z")),
            Payload::PresenceActive(Empty {}),
        );
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["event_type"], "presence.active");
        assert_eq!(value["metadata"], serde_json::json!({}));
        assert_eq!(value["source"], "agent@0.1.0");
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["timestamp_start"], "2026-11-07T09:00:00Z");
        assert!(value.get("user_id").is_none() && value.get("device_id").is_none());
        let back: Event = serde_json::from_value(value).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn optional_fields_may_be_missing() {
        let json = serde_json::json!({
            "event_id": "019a5e1c-0000-7000-8000-000000000001",
            "schema_version": 1,
            "timestamp_start": "2026-11-07T09:00:00Z",
            "source": "vscode@0.3.1",
            "event_type": "editor.save",
            "metadata": {"file_path": "src/main.rs"}
        });
        let event: Event = serde_json::from_value(json).unwrap();
        assert_eq!(event.timestamp_end, None);
        assert_eq!(event.event_type(), "editor.save");
    }

    #[test]
    fn unknown_event_type_is_rejected() {
        let json = serde_json::json!({
            "event_id": "019a5e1c-0000-7000-8000-000000000001",
            "schema_version": 1,
            "timestamp_start": "2026-11-07T09:00:00Z",
            "source": "agent@0.1.0",
            "event_type": "keyboard.keystroke",
            "metadata": {}
        });
        assert!(serde_json::from_value::<Event>(json).is_err());
    }

    #[test]
    fn source_requires_name_and_version() {
        assert_eq!(
            "vscode@0.3.1".parse::<Source>().unwrap(),
            Source::new("vscode", "0.3.1")
        );
        for bad in ["vscode", "@1", "vscode@", ""] {
            assert!(bad.parse::<Source>().is_err(), "{bad:?}");
        }
    }
}
