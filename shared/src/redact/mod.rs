//! Redaction (SHARED plan §4.2, SH-5; levels: D18): runs in the agent before
//! an event is buffered.
//!
//! [`redact_with`] applies one [`RedactionLevel`]:
//!
//! - `secrets` (default): credentials only ([`mask_credentials`]) in free
//!   text; paths, titles, URLs and e-mail addresses are sent as collected.
//! - `none`: nothing is masked.
//! - `full`: the rules below ([`redact`]).
//!
//! Git remotes are normalized to `host/owner/repo` at every level: it is the
//! project-matching key, and it drops credentials embedded in the URL.
//!
//! The `full` rules, per field:
//!
//! - **Window titles:** secrets and e-mail addresses masked, URLs stripped
//!   ([`strip_url`]), truncated to [`MAX_TITLE`] characters.
//! - **Git remotes:** normalized to `host/owner/repo` ([`normalize_remote`]),
//!   which also drops any credentials.
//! - **Everything else** (branches, paths, names, hints): secrets and
//!   e-mail addresses masked ([`redact_text`]).
//!
//! Structured fields (hashes, counts, enums) are left alone. The rule set
//! version is [`crate::config::REDACTION_VERSION`].

mod patterns;
mod url;

use std::sync::LazyLock;

use regex::Regex;

pub use self::patterns::{EMAIL_MASK, MASK, mask_credentials, mask_emails, mask_secrets};
pub use self::url::{ID, normalize_remote, strip_url};
pub use crate::config::RedactionLevel;
use crate::event::{Event, Payload};

/// Window titles are cut to this many characters (after masking).
pub const MAX_TITLE: usize = 200;

static URL_IN_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:https?|wss?|ftp)://[^\s<>"'`]+"#).expect("valid URL pattern")
});

/// Applies `level` to `event` in place.
pub fn redact_with(event: &mut Event, level: RedactionLevel) {
    match level {
        RedactionLevel::Full => redact(event),
        RedactionLevel::Secrets => apply_to_text(event, &mask_credentials),
        RedactionLevel::None => apply_to_text(event, &|s: &str| s.to_owned()),
    }
}

/// Runs `mask` over the free-text fields (titles, app names, branches,
/// hints). Paths are left as collected; remotes are normalized.
fn apply_to_text(event: &mut Event, mask: &dyn Fn(&str) -> String) {
    let text = |v: &mut Option<String>| {
        if let Some(s) = v {
            *s = mask(s);
        }
    };
    text(&mut event.project_hint);
    if let Some(repository) = &mut event.repository {
        remote_opt(&mut repository.remote_url);
    }
    match &mut event.payload {
        Payload::WindowFocus(m) => {
            text(&mut m.app);
            text(&mut m.title);
        }
        Payload::EditorWorkspaceOpen(m) => remote_opt(&mut m.remote_url),
        Payload::EditorDebugSession(m) => text(&mut m.configuration_name),
        Payload::GitCommit(m) => {
            text(&mut m.branch);
            remote_opt(&mut m.remote_url);
        }
        Payload::GitCheckout(m) => {
            text(&mut m.from_branch);
            text(&mut m.to_branch);
        }
        Payload::GitMerge(m) => text(&mut m.branch),
        Payload::GitBackfillCommit(m) => text(&mut m.branch),
        Payload::EditorFileFocus(_)
        | Payload::EditorEditBurst(_)
        | Payload::EditorSave(_)
        | Payload::PresenceActive(_)
        | Payload::PresenceIdle(_)
        | Payload::AgentHeartbeat(_)
        | Payload::AgentPause(_)
        | Payload::AgentResume(_)
        | Payload::AgentBufferOverflow(_) => {}
    }
}

/// The `full` level: every rule, every field.
pub fn redact(event: &mut Event) {
    redact_opt(&mut event.project_hint);
    redact_opt(&mut event.working_directory);
    if let Some(repository) = &mut event.repository {
        redact_opt(&mut repository.root_path);
        remote_opt(&mut repository.remote_url);
    }
    match &mut event.payload {
        Payload::WindowFocus(m) => {
            redact_opt(&mut m.app);
            if let Some(title) = &mut m.title {
                *title = redact_title(title);
            }
        }
        Payload::EditorWorkspaceOpen(m) => {
            m.workspace_path = redact_text(&m.workspace_path);
            remote_opt(&mut m.remote_url);
        }
        Payload::EditorFileFocus(m) => m.file_path = redact_text(&m.file_path),
        Payload::EditorEditBurst(m) => m.file_path = redact_text(&m.file_path),
        Payload::EditorSave(m) => m.file_path = redact_text(&m.file_path),
        Payload::EditorDebugSession(m) => redact_opt(&mut m.configuration_name),
        Payload::GitCommit(m) => {
            redact_opt(&mut m.branch);
            remote_opt(&mut m.remote_url);
        }
        Payload::GitCheckout(m) => {
            redact_opt(&mut m.from_branch);
            redact_opt(&mut m.to_branch);
        }
        Payload::GitMerge(m) => redact_opt(&mut m.branch),
        Payload::GitBackfillCommit(m) => redact_opt(&mut m.branch),
        Payload::PresenceActive(_)
        | Payload::PresenceIdle(_)
        | Payload::AgentHeartbeat(_)
        | Payload::AgentPause(_)
        | Payload::AgentResume(_)
        | Payload::AgentBufferOverflow(_) => {}
    }
}

/// Window title rule: URLs stripped, secrets and e-mails masked, truncated.
pub fn redact_title(title: &str) -> String {
    let stripped = URL_IN_TEXT.replace_all(title, |c: &regex::Captures| {
        let raw = c[0].trim_end_matches(['.', ',', ')', ']', ';', ':', '!', '?']);
        let tail = &c[0][raw.len()..];
        // Credentials and tokens in the URL are masked before stripping, so
        // an unparseable URL still loses them.
        let masked = mask_secrets(raw);
        format!("{}{tail}", strip_url(&masked).unwrap_or(masked))
    });
    let masked = mask_emails(&mask_secrets(&stripped));
    truncate(&masked, MAX_TITLE)
}

/// Normalized remote, or (for local or odd remotes) the text with secrets masked.
pub fn redact_remote(remote: &str) -> String {
    normalize_remote(remote).unwrap_or_else(|| redact_text(remote))
}

/// Free-text rule: secrets and e-mail addresses masked.
pub fn redact_text(text: &str) -> String {
    mask_emails(&mask_secrets(text))
}

fn redact_opt(value: &mut Option<String>) {
    if let Some(v) = value {
        *v = redact_text(v);
    }
}

fn remote_opt(value: &mut Option<String>) {
    if let Some(v) = value {
        *v = redact_remote(v);
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    // `max - 1` characters plus the ellipsis: never longer than `max`.
    let cut = text
        .char_indices()
        .nth(max.saturating_sub(1))
        .map_or(text.len(), |(i, _)| i);
    format!("{}…", &text[..cut])
}

#[cfg(test)]
mod tests;
