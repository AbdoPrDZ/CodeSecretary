//! Event validation (SHARED plan §2, SH-4).
//!
//! Two entry points:
//!
//! - [`parse`]: untrusted JSON (local API, a corrupt buffer row) → [`Event`],
//!   with a precise [`Invalid`] reason instead of a serde message.
//! - [`validate`]: rules a well-typed [`Event`] can still break (timestamps,
//!   sizes, per-type rules). The agent runs it before buffering (AG-6).
//!
//! The reason codes ([`Invalid::code`]) are the `reason` values the warehouse
//! reports in `BatchResponse::rejected`. The warehouse gets the constants
//! and the event shape from the generated contract (`codegen`, SH-9) but
//! re-implements these rules in `ingest.py`. Change both together.
//!
//! Error values never carry event contents, so they are safe to log.

use std::fmt;

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;

use crate::event::{EVENT_TYPES, Event, Payload, SCHEMA_VERSION, Source};

/// Events may not end more than this after "now" (on the same clock).
pub const MAX_FUTURE_SKEW: TimeDelta = TimeDelta::minutes(5);
/// `metadata`, serialized as compact JSON (UTF-8), at most this many bytes.
pub const MAX_METADATA_BYTES: usize = 8 * 1024;
/// Any single text field, in characters.
pub const MAX_TEXT: usize = 2048;
/// `source` (`name@version`), in characters.
pub const MAX_SOURCE: usize = 128;
/// `project_hint`, in characters.
pub const MAX_PROJECT_HINT: usize = 255;

/// Envelope fields every event must have.
pub const REQUIRED_FIELDS: &[&str] = &[
    "event_id",
    "schema_version",
    "timestamp_start",
    "source",
    "event_type",
    "metadata",
];

/// Why an event is invalid. [`Invalid::code`] is the wire reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    /// Not a JSON object.
    NotAnObject,
    /// A field of [`REQUIRED_FIELDS`] is absent or `null`.
    MissingField,
    /// `event_id` is not a UUIDv7.
    InvalidEventId,
    /// `schema_version` is not one this build understands.
    UnsupportedSchemaVersion,
    /// `event_type` is not in the catalog.
    UnknownEventType,
    /// `source` is not `name@version` of at most [`MAX_SOURCE`] characters.
    InvalidSource,
    /// Unparseable timestamps, end before start, or the wrong shape for the
    /// type (interval types need an end; point types must not span time).
    InvalidTimestamps,
    /// Ends more than [`MAX_FUTURE_SKEW`] after now.
    TimestampInFuture,
    /// `metadata` does not match the event type.
    InvalidMetadata,
    /// `metadata` is larger than [`MAX_METADATA_BYTES`].
    MetadataTooLarge,
    /// A text field is empty where a value is required, or too long.
    InvalidField,
}

impl Invalid {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAnObject => "invalid_event",
            Self::MissingField => "missing_field",
            Self::InvalidEventId => "invalid_event_id",
            Self::UnsupportedSchemaVersion => "unsupported_schema_version",
            Self::UnknownEventType => "unknown_event_type",
            Self::InvalidSource => "invalid_source",
            Self::InvalidTimestamps => "invalid_timestamps",
            Self::TimestampInFuture => "timestamp_in_future",
            Self::InvalidMetadata => "invalid_metadata",
            Self::MetadataTooLarge => "metadata_too_large",
            Self::InvalidField => "invalid_field",
        }
    }
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Invalid {}

/// Interval event types (blueprint Appendix B, "Kind"); all others are points.
pub fn is_interval(event_type: &str) -> bool {
    matches!(
        event_type,
        "presence.active"
            | "presence.idle"
            | "window.focus"
            | "editor.file_focus"
            | "editor.edit_burst"
            | "editor.debug_session"
    )
}

/// Parse untrusted JSON into an event and [`validate`] it.
pub fn parse(value: Value, now: DateTime<Utc>) -> Result<Event, Invalid> {
    let Value::Object(fields) = &value else {
        return Err(Invalid::NotAnObject);
    };
    for name in REQUIRED_FIELDS {
        if fields.get(*name).is_none_or(Value::is_null) {
            return Err(Invalid::MissingField);
        }
    }
    let event_id = fields["event_id"]
        .as_str()
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or(Invalid::InvalidEventId)?;
    if event_id.get_version_num() != 7 {
        return Err(Invalid::InvalidEventId);
    }
    if fields["schema_version"].as_u64() != Some(u64::from(SCHEMA_VERSION)) {
        return Err(Invalid::UnsupportedSchemaVersion);
    }
    let event_type = fields["event_type"].as_str().unwrap_or_default();
    if !EVENT_TYPES.contains(&event_type) {
        return Err(Invalid::UnknownEventType);
    }
    if !fields["source"].as_str().is_some_and(valid_source) {
        return Err(Invalid::InvalidSource);
    }
    for name in ["timestamp_start", "timestamp_end"] {
        match fields.get(name) {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s.parse::<DateTime<Utc>>().is_ok() => {}
            Some(_) => return Err(Invalid::InvalidTimestamps),
        }
    }
    if !fields["metadata"].is_object() {
        return Err(Invalid::InvalidMetadata);
    }
    // The envelope is sound, so what is left for serde to refuse is either
    // metadata of the wrong shape or a bad optional envelope field.
    let event: Event = serde_json::from_value(value).map_err(|_| Invalid::InvalidMetadata)?;
    validate(&event, now)?;
    Ok(event)
}

/// Check a typed event. `now` must be on the same clock as the event's
/// timestamps (the agent: device clock; the warehouse adjusts first).
pub fn validate(event: &Event, now: DateTime<Utc>) -> Result<(), Invalid> {
    if event.event_id.as_uuid().get_version_num() != 7 {
        return Err(Invalid::InvalidEventId);
    }
    if event.schema_version != SCHEMA_VERSION {
        return Err(Invalid::UnsupportedSchemaVersion);
    }
    if !valid_source(&event.source.to_string()) || !valid_source_parts(&event.source) {
        return Err(Invalid::InvalidSource);
    }

    let start = event.timestamp_start;
    match event.timestamp_end {
        Some(end) if end < start => return Err(Invalid::InvalidTimestamps),
        None if is_interval(event.event_type()) => return Err(Invalid::InvalidTimestamps),
        Some(end) if end != start && !is_interval(event.event_type()) => {
            return Err(Invalid::InvalidTimestamps);
        }
        _ => {}
    }
    if event.timestamp_end.unwrap_or(start) > now + MAX_FUTURE_SKEW {
        return Err(Invalid::TimestampInFuture);
    }

    text(event.project_hint.as_deref(), MAX_PROJECT_HINT)?;
    text(event.working_directory.as_deref(), MAX_TEXT)?;
    if let Some(repository) = &event.repository {
        text(repository.root_path.as_deref(), MAX_TEXT)?;
        text(repository.remote_url.as_deref(), MAX_TEXT)?;
    }

    if metadata_len(&event.payload) > MAX_METADATA_BYTES {
        return Err(Invalid::MetadataTooLarge);
    }
    validate_payload(&event.payload)
}

/// Compact JSON size of the event's `metadata` object, in bytes.
pub fn metadata_len(payload: &Payload) -> usize {
    let value = serde_json::to_value(payload).expect("payload serializes");
    serde_json::to_vec(&value["metadata"])
        .expect("metadata serializes")
        .len()
}

fn validate_payload(payload: &Payload) -> Result<(), Invalid> {
    match payload {
        Payload::PresenceActive(_)
        | Payload::PresenceIdle(_)
        | Payload::AgentPause(_)
        | Payload::AgentResume(_)
        | Payload::AgentBufferOverflow(_) => Ok(()),
        Payload::WindowFocus(m) => {
            text(m.app.as_deref(), MAX_TEXT)?;
            text(m.title.as_deref(), MAX_TEXT)
        }
        Payload::EditorWorkspaceOpen(m) => {
            required(&m.workspace_path)?;
            text(m.remote_url.as_deref(), MAX_TEXT)
        }
        Payload::EditorFileFocus(m) => {
            relative_path(&m.file_path)?;
            text(m.language.as_deref(), 64)
        }
        Payload::EditorEditBurst(m) => relative_path(&m.file_path),
        Payload::EditorSave(m) => relative_path(&m.file_path),
        Payload::EditorDebugSession(m) => text(m.configuration_name.as_deref(), MAX_TEXT),
        Payload::GitCommit(m) => {
            commit_hash(&m.hash)?;
            text(m.branch.as_deref(), MAX_TEXT)?;
            text(m.remote_url.as_deref(), MAX_TEXT)
        }
        Payload::GitCheckout(m) => {
            text(m.from_branch.as_deref(), MAX_TEXT)?;
            text(m.to_branch.as_deref(), MAX_TEXT)
        }
        Payload::GitMerge(m) => text(m.branch.as_deref(), MAX_TEXT),
        Payload::GitBackfillCommit(m) => {
            commit_hash(&m.hash)?;
            text(m.branch.as_deref(), MAX_TEXT)?;
            // Backfilled time is always an estimate.
            if m.estimated {
                Ok(())
            } else {
                Err(Invalid::InvalidMetadata)
            }
        }
        Payload::AgentHeartbeat(m) => {
            required(&m.agent_version).and(text(Some(&m.agent_version), 64))
        }
    }
}

fn valid_source(s: &str) -> bool {
    s.chars().count() <= MAX_SOURCE && s.parse::<Source>().is_ok()
}

fn valid_source_parts(source: &Source) -> bool {
    // A name containing '@' would not survive the `name@version` round trip.
    !source.name.contains('@')
}

fn text(value: Option<&str>, limit: usize) -> Result<(), Invalid> {
    match value {
        Some(s) if s.chars().count() > limit => Err(Invalid::InvalidField),
        _ => Ok(()),
    }
}

fn required(value: &str) -> Result<(), Invalid> {
    if value.trim().is_empty() {
        return Err(Invalid::InvalidField);
    }
    text(Some(value), MAX_TEXT)
}

/// Editor paths are repository-relative (privacy rule): never absolute,
/// never escaping the repository.
fn relative_path(path: &str) -> Result<(), Invalid> {
    required(path)?;
    let bytes = path.as_bytes();
    let absolute = path.starts_with('/')
        || path.starts_with('\\')
        || path.starts_with('~')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':');
    let escapes = path.split(['/', '\\']).any(|segment| segment == "..");
    if absolute || escapes {
        return Err(Invalid::InvalidField);
    }
    Ok(())
}

/// Abbreviated (≥ 7) or full SHA-1 / SHA-256 hex.
fn commit_hash(hash: &str) -> Result<(), Invalid> {
    if (7..=64).contains(&hash.len()) && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Invalid::InvalidField)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::event::{EditorSave, Empty, GitBackfillCommit, RepositoryRef, WindowFocus};

    fn now() -> DateTime<Utc> {
        "2026-11-20T12:00:00Z".parse().unwrap()
    }

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn active() -> Event {
        Event::new(
            Source::new("agent", "0.1.0"),
            ts("2026-11-20T11:00:00Z"),
            Some(ts("2026-11-20T11:05:00Z")),
            Payload::PresenceActive(Empty {}),
        )
    }

    fn save(path: &str) -> Event {
        Event::new(
            Source::new("vscode", "0.1.0"),
            ts("2026-11-20T11:00:00Z"),
            None,
            Payload::EditorSave(EditorSave {
                file_path: path.into(),
            }),
        )
    }

    fn wire(event: &Event) -> Value {
        serde_json::to_value(event).unwrap()
    }

    #[test]
    fn valid_events_pass() {
        assert_eq!(validate(&active(), now()), Ok(()));
        assert_eq!(validate(&save("src/main.rs"), now()), Ok(()));
        let parsed = parse(wire(&active()), now()).unwrap();
        assert_eq!(parsed, active_with_id(&parsed));
    }

    fn active_with_id(other: &Event) -> Event {
        let mut event = active();
        event.event_id = other.event_id;
        event
    }

    #[test]
    fn rejects_missing_fields() {
        for name in REQUIRED_FIELDS {
            let mut value = wire(&active());
            value.as_object_mut().unwrap().remove(*name);
            assert_eq!(parse(value, now()), Err(Invalid::MissingField), "{name}");

            let mut value = wire(&active());
            value[*name] = Value::Null;
            assert_eq!(
                parse(value, now()),
                Err(Invalid::MissingField),
                "{name} = null"
            );
        }
        assert_eq!(parse(json!([1]), now()), Err(Invalid::NotAnObject));
    }

    #[test]
    fn rejects_end_before_start() {
        let mut event = active();
        event.timestamp_end = Some(ts("2026-11-20T10:59:59Z"));
        assert_eq!(validate(&event, now()), Err(Invalid::InvalidTimestamps));
    }

    #[test]
    fn interval_and_point_shapes() {
        let mut interval = active();
        interval.timestamp_end = None;
        assert_eq!(validate(&interval, now()), Err(Invalid::InvalidTimestamps));

        let mut point = save("a.rs");
        point.timestamp_end = Some(point.timestamp_start);
        assert_eq!(validate(&point, now()), Ok(()));
        point.timestamp_end = Some(ts("2026-11-20T11:00:01Z"));
        assert_eq!(validate(&point, now()), Err(Invalid::InvalidTimestamps));
    }

    #[test]
    fn rejects_future_beyond_skew() {
        let mut event = active();
        event.timestamp_start = ts("2026-11-20T12:00:00Z");
        event.timestamp_end = Some(ts("2026-11-20T12:05:00Z"));
        assert_eq!(validate(&event, now()), Ok(()), "exactly 5 min is allowed");
        event.timestamp_end = Some(ts("2026-11-20T12:05:01Z"));
        assert_eq!(validate(&event, now()), Err(Invalid::TimestampInFuture));

        let mut point = save("a.rs");
        point.timestamp_start = ts("2026-11-20T12:06:00Z");
        assert_eq!(validate(&point, now()), Err(Invalid::TimestampInFuture));
    }

    #[test]
    fn rejects_metadata_over_8_kb() {
        let focus = |chars: usize| {
            Event::new(
                Source::new("agent", "0.1.0"),
                ts("2026-11-20T11:00:00Z"),
                Some(ts("2026-11-20T11:01:00Z")),
                Payload::WindowFocus(WindowFocus {
                    // 2 bytes each in UTF-8; each field stays within MAX_TEXT.
                    app: Some("é".repeat(chars)),
                    title: Some("é".repeat(chars)),
                }),
            )
        };
        let big = focus(MAX_TEXT);
        assert!(metadata_len(&big.payload) > MAX_METADATA_BYTES);
        assert_eq!(validate(&big, now()), Err(Invalid::MetadataTooLarge));
        assert_eq!(validate(&focus(100), now()), Ok(()));
    }

    #[test]
    fn rejects_unknown_type() {
        let mut value = wire(&active());
        value["event_type"] = json!("keyboard.keystroke");
        assert_eq!(parse(value, now()), Err(Invalid::UnknownEventType));
    }

    #[test]
    fn envelope_field_reasons() {
        let cases: Vec<(&str, Value, Invalid)> = vec![
            ("event_id", json!("not-a-uuid"), Invalid::InvalidEventId),
            (
                "event_id",
                json!("6f9619ff-8b86-4011-b42d-00cf4fc964ff"), // v4
                Invalid::InvalidEventId,
            ),
            (
                "schema_version",
                json!(2),
                Invalid::UnsupportedSchemaVersion,
            ),
            (
                "schema_version",
                json!("1"),
                Invalid::UnsupportedSchemaVersion,
            ),
            ("source", json!("agent"), Invalid::InvalidSource),
            (
                "source",
                json!(format!("a@{}", "1".repeat(200))),
                Invalid::InvalidSource,
            ),
            (
                "timestamp_start",
                json!("yesterday"),
                Invalid::InvalidTimestamps,
            ),
            ("timestamp_end", json!(5), Invalid::InvalidTimestamps),
            ("metadata", json!([]), Invalid::InvalidMetadata),
            ("project_hint", json!(7), Invalid::InvalidMetadata),
        ];
        for (name, bad, expected) in cases {
            let mut value = wire(&active());
            value[name] = bad;
            assert_eq!(parse(value, now()), Err(expected), "{name}");
        }
    }

    #[test]
    fn metadata_must_match_type() {
        let mut value = wire(&save("a.rs"));
        value["metadata"] = json!({"path": "a.rs"});
        assert_eq!(parse(value, now()), Err(Invalid::InvalidMetadata));
    }

    #[test]
    fn editor_paths_are_repository_relative() {
        for good in ["src/main.rs", "README.md", "a/b/c.txt", "..hidden"] {
            assert_eq!(validate(&save(good), now()), Ok(()), "{good}");
        }
        for bad in [
            "",
            "  ",
            "/home/dev/api/src/main.rs",
            "C:\\Users\\dev\\api\\main.rs",
            "c:/x",
            "\\\\server\\share\\x",
            "~/api/x",
            "../secrets.txt",
            "src/../../x",
        ] {
            assert_eq!(
                validate(&save(bad), now()),
                Err(Invalid::InvalidField),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn git_rules() {
        let backfill = |hash: &str, estimated: bool| {
            Event::new(
                Source::new("agent", "0.1.0"),
                ts("2026-11-20T11:00:00Z"),
                None,
                Payload::GitBackfillCommit(GitBackfillCommit {
                    hash: hash.into(),
                    branch: None,
                    estimated,
                }),
            )
        };
        assert_eq!(validate(&backfill("a1b2c3d", true), now()), Ok(()));
        assert_eq!(
            validate(&backfill("a1b2c3d", false), now()),
            Err(Invalid::InvalidMetadata)
        );
        for bad in ["abc", "zzzzzzz", &"a".repeat(65)] {
            assert_eq!(
                validate(&backfill(bad, true), now()),
                Err(Invalid::InvalidField),
                "{bad}"
            );
        }
    }

    #[test]
    fn text_limits() {
        let mut event = active();
        event.project_hint = Some("p".repeat(MAX_PROJECT_HINT + 1));
        assert_eq!(validate(&event, now()), Err(Invalid::InvalidField));

        let mut event = active();
        event.repository = Some(RepositoryRef {
            root_path: Some("r".repeat(MAX_TEXT + 1)),
            remote_url: None,
        });
        assert_eq!(validate(&event, now()), Err(Invalid::InvalidField));
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(Invalid::MissingField.code(), "missing_field");
        assert_eq!(
            Invalid::TimestampInFuture.to_string(),
            "timestamp_in_future"
        );
    }
}
