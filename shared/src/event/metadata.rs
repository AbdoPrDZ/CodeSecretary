//! Event types and their metadata (blueprint Appendix B, Phase 1).
//!
//! Every field here is already redacted when it leaves the agent. Paths are
//! repository-relative; window titles pass through the redactor (SH-5).

use serde::{Deserialize, Serialize};

/// `event_type` + `metadata`, adjacently tagged so the envelope stays flat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
#[serde(tag = "event_type", content = "metadata")]
pub enum Payload {
    #[serde(rename = "presence.active")]
    PresenceActive(Empty),
    #[serde(rename = "presence.idle")]
    PresenceIdle(Empty),
    #[serde(rename = "window.focus")]
    WindowFocus(WindowFocus),
    #[serde(rename = "editor.workspace_open")]
    EditorWorkspaceOpen(EditorWorkspaceOpen),
    #[serde(rename = "editor.file_focus")]
    EditorFileFocus(EditorFileFocus),
    #[serde(rename = "editor.edit_burst")]
    EditorEditBurst(EditorEditBurst),
    #[serde(rename = "editor.save")]
    EditorSave(EditorSave),
    #[serde(rename = "editor.debug_session")]
    EditorDebugSession(EditorDebugSession),
    #[serde(rename = "git.commit")]
    GitCommit(GitCommit),
    #[serde(rename = "git.checkout")]
    GitCheckout(GitCheckout),
    #[serde(rename = "git.merge")]
    GitMerge(GitMerge),
    #[serde(rename = "git.backfill_commit")]
    GitBackfillCommit(GitBackfillCommit),
    #[serde(rename = "agent.heartbeat")]
    AgentHeartbeat(AgentHeartbeat),
    #[serde(rename = "agent.pause")]
    AgentPause(AgentPauseChange),
    #[serde(rename = "agent.resume")]
    AgentResume(AgentPauseChange),
    /// The local buffer hit its bound and dropped the oldest events (AGENT §3.1).
    #[serde(rename = "agent.buffer_overflow")]
    AgentBufferOverflow(AgentBufferOverflow),
}

/// Every Phase 1 `event_type`, in catalog order.
pub const EVENT_TYPES: &[&str] = &[
    "presence.active",
    "presence.idle",
    "window.focus",
    "editor.workspace_open",
    "editor.file_focus",
    "editor.edit_burst",
    "editor.save",
    "editor.debug_session",
    "git.commit",
    "git.checkout",
    "git.merge",
    "git.backfill_commit",
    "agent.heartbeat",
    "agent.pause",
    "agent.resume",
    "agent.buffer_overflow",
];

impl Payload {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::PresenceActive(_) => "presence.active",
            Self::PresenceIdle(_) => "presence.idle",
            Self::WindowFocus(_) => "window.focus",
            Self::EditorWorkspaceOpen(_) => "editor.workspace_open",
            Self::EditorFileFocus(_) => "editor.file_focus",
            Self::EditorEditBurst(_) => "editor.edit_burst",
            Self::EditorSave(_) => "editor.save",
            Self::EditorDebugSession(_) => "editor.debug_session",
            Self::GitCommit(_) => "git.commit",
            Self::GitCheckout(_) => "git.checkout",
            Self::GitMerge(_) => "git.merge",
            Self::GitBackfillCommit(_) => "git.backfill_commit",
            Self::AgentHeartbeat(_) => "agent.heartbeat",
            Self::AgentPause(_) => "agent.pause",
            Self::AgentResume(_) => "agent.resume",
            Self::AgentBufferOverflow(_) => "agent.buffer_overflow",
        }
    }
}

/// No metadata (`{}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct Empty {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct WindowFocus {
    #[serde(default)]
    pub app: Option<String>,
    /// Redacted title.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct EditorWorkspaceOpen {
    pub workspace_path: String,
    #[serde(default)]
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct EditorFileFocus {
    pub file_path: String,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct EditorEditBurst {
    pub file_path: String,
    pub edit_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct EditorSave {
    pub file_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct EditorDebugSession {
    #[serde(default)]
    pub configuration_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct GitCommit {
    pub hash: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub files_changed: Option<u32>,
    #[serde(default)]
    pub remote_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct GitCheckout {
    #[serde(default)]
    pub from_branch: Option<String>,
    #[serde(default)]
    pub to_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct GitMerge {
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct GitBackfillCommit {
    pub hash: String,
    #[serde(default)]
    pub branch: Option<String>,
    /// Always `true`: backfilled time is an estimate.
    pub estimated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct AgentHeartbeat {
    pub agent_version: String,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub queue_depth: u64,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub clock_offset_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    User,
    ExclusionWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct AgentPauseChange {
    pub reason: PauseReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct AgentBufferOverflow {
    /// Events dropped (oldest first).
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub dropped: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, RepositoryRef, Source};

    fn one_of_each() -> Vec<Payload> {
        vec![
            Payload::PresenceActive(Empty {}),
            Payload::PresenceIdle(Empty {}),
            Payload::WindowFocus(WindowFocus {
                app: Some("Code.exe".into()),
                title: Some("main.rs — [redacted]".into()),
            }),
            Payload::EditorWorkspaceOpen(EditorWorkspaceOpen {
                workspace_path: "/home/dev/api".into(),
                remote_url: Some("github.com/acme/api".into()),
            }),
            Payload::EditorFileFocus(EditorFileFocus {
                file_path: "src/main.rs".into(),
                language: Some("rust".into()),
            }),
            Payload::EditorEditBurst(EditorEditBurst {
                file_path: "src/main.rs".into(),
                edit_count: 12,
            }),
            Payload::EditorSave(EditorSave {
                file_path: "src/main.rs".into(),
            }),
            Payload::EditorDebugSession(EditorDebugSession {
                configuration_name: Some("Launch".into()),
            }),
            Payload::GitCommit(GitCommit {
                hash: "a1b2c3d".into(),
                branch: Some("main".into()),
                files_changed: Some(3),
                remote_url: Some("github.com/acme/api".into()),
            }),
            Payload::GitCheckout(GitCheckout {
                from_branch: Some("main".into()),
                to_branch: Some("feature".into()),
            }),
            Payload::GitMerge(GitMerge {
                branch: Some("feature".into()),
            }),
            Payload::GitBackfillCommit(GitBackfillCommit {
                hash: "a1b2c3d".into(),
                branch: None,
                estimated: true,
            }),
            Payload::AgentHeartbeat(AgentHeartbeat {
                agent_version: "0.1.0".into(),
                queue_depth: 4,
                clock_offset_ms: -120,
            }),
            Payload::AgentPause(AgentPauseChange {
                reason: PauseReason::User,
            }),
            Payload::AgentResume(AgentPauseChange {
                reason: PauseReason::ExclusionWindow,
            }),
            Payload::AgentBufferOverflow(AgentBufferOverflow { dropped: 10 }),
        ]
    }

    #[test]
    fn every_event_type_round_trips() {
        let payloads = one_of_each();
        let types: Vec<&str> = payloads.iter().map(Payload::event_type).collect();
        assert_eq!(types, EVENT_TYPES, "catalog covers every variant, in order");

        for payload in payloads {
            let mut event = Event::new(
                Source::new("test", "1.0.0"),
                "2026-11-07T09:00:00Z".parse().unwrap(),
                None,
                payload,
            );
            event.repository = Some(RepositoryRef {
                root_path: Some("/home/dev/api".into()),
                remote_url: Some("github.com/acme/api".into()),
            });
            let json = serde_json::to_value(&event).unwrap();
            assert_eq!(json["event_type"], event.event_type());
            assert!(json["metadata"].is_object(), "{json}");
            let back: Event = serde_json::from_value(json).unwrap();
            assert_eq!(back, event);
        }
    }

    #[test]
    fn pause_reason_is_snake_case() {
        let json = serde_json::to_value(Payload::AgentPause(AgentPauseChange {
            reason: PauseReason::ExclusionWindow,
        }))
        .unwrap();
        assert_eq!(json["metadata"]["reason"], "exclusion_window");
    }
}
