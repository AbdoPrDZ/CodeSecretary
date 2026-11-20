//! Redaction (SHARED plan §4.2, SH-5): runs in the agent before an event is
//! buffered, so secrets never touch disk or leave the device.
//!
//! [`redact`] applies the default rules to every free-text field of an
//! event:
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

pub use self::patterns::{EMAIL_MASK, MASK, mask_emails, mask_secrets};
pub use self::url::{ID, normalize_remote, strip_url};
use crate::event::{Event, Payload};

/// Window titles are cut to this many characters (after masking).
pub const MAX_TITLE: usize = 200;

static URL_IN_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:https?|wss?|ftp)://[^\s<>"'`]+"#).expect("valid URL pattern")
});

/// Applies every default rule to `event` in place.
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
