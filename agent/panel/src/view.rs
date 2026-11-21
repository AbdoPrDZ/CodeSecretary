//! What the panel shows, as plain data (no GUI types), so it can be tested.

use chrono::{DateTime, Local, Utc};
use secretary_shared::local::{AgentStatus, ConnectionState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Good,
    Warn,
    Bad,
    Off,
}

pub enum Snapshot {
    /// No status file: the daemon never ran under this user (or another home).
    Missing,
    /// The file exists but is not a status (e.g. mid-replace on some systems).
    Unreadable,
    Status(Box<AgentStatus>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub label: &'static str,
    pub value: String,
    pub level: Option<Level>,
}

fn row(label: &'static str, value: impl Into<String>) -> Row {
    Row {
        label,
        value: value.into(),
        level: None,
    }
}

fn colored(label: &'static str, value: impl Into<String>, level: Level) -> Row {
    Row {
        label,
        value: value.into(),
        level: Some(level),
    }
}

/// The headline: one color, one sentence, one hint.
pub fn summary(snapshot: &Snapshot, now: DateTime<Utc>) -> (Level, String, String) {
    let status = match snapshot {
        Snapshot::Missing => {
            return (
                Level::Off,
                "Agent not started".into(),
                "Run `secretary-agent run` (after `secretary-agent login`).".into(),
            );
        }
        Snapshot::Unreadable => {
            return (Level::Warn, "Status unreadable".into(), "Retrying…".into());
        }
        Snapshot::Status(status) => status,
    };
    if !status.is_live(now) {
        let when = status
            .written_at
            .map_or_else(|| "unknown".into(), |t| ago(t, now));
        let what = if status.running {
            "Agent not responding"
        } else {
            "Agent stopped"
        };
        return (Level::Off, what.into(), format!("Last status {when}."));
    }
    match status.connection.state {
        ConnectionState::SignedOut => (
            Level::Warn,
            "Running · signed out".into(),
            "Collecting locally. Run `secretary-agent login` to upload.".into(),
        ),
        ConnectionState::Offline => (
            Level::Bad,
            "Running · server unreachable".into(),
            format!(
                "{} — retrying; {} event(s) wait locally.",
                status.connection.last_error.as_deref().unwrap_or("error"),
                status.upload.queued
            ),
        ),
        ConnectionState::Online => (
            Level::Good,
            if status.config.paused {
                "Running · paused"
            } else {
                "Running · connected"
            }
            .into(),
            status
                .account
                .as_ref()
                .map_or_else(String::new, |a| a.server_url.clone()),
        ),
        ConnectionState::Unknown => (Level::Warn, "Running · connecting…".into(), String::new()),
    }
}

/// Detail sections, in display order.
pub fn rows(status: &AgentStatus, now: DateTime<Utc>) -> Vec<(&'static str, Vec<Row>)> {
    let when = |t: Option<DateTime<Utc>>| t.map_or_else(|| "never".to_owned(), |t| ago(t, now));
    let mut out = Vec::new();

    let account = match &status.account {
        Some(a) => vec![
            row(
                "Signed in as",
                a.email.clone().unwrap_or_else(|| "yes".into()),
            ),
            row("Server", a.server_url.clone()),
            row(
                "Device",
                a.device_id
                    .map_or("not registered".into(), |id| format!("#{id}")),
            ),
        ],
        None => vec![colored("Signed in", "no", Level::Warn)],
    };
    out.push(("Account", account));

    let c = &status.connection;
    let state = match c.state {
        ConnectionState::Online => colored("Connection", "online", Level::Good),
        ConnectionState::Offline => colored("Connection", "offline", Level::Bad),
        ConnectionState::SignedOut => colored("Connection", "signed out", Level::Warn),
        ConnectionState::Unknown => colored("Connection", "not yet tried", Level::Off),
    };
    let mut connection = vec![state, row("Last success", when(c.last_ok_at))];
    if let Some(error) = &c.last_error {
        connection.push(row(
            "Last error",
            format!("{error} ({})", when(c.last_error_at)),
        ));
    }
    out.push(("Server", connection));

    let u = &status.upload;
    let queued_level = match u.queued {
        0..=499 => None,
        500..=9_999 => Some(Level::Warn),
        _ => Some(Level::Bad),
    };
    let mut upload = vec![Row {
        label: "Waiting to upload",
        value: format!("{} event(s)", u.queued),
        level: queued_level,
    }];
    if let Some(oldest) = u.oldest_queued_at {
        upload.push(row("Oldest waiting", ago(oldest, now)));
    }
    upload.push(row(
        "Last upload",
        u.last_upload_at.map_or("none since start".into(), |t| {
            format!("{} ({} events)", ago(t, now), u.last_upload_events)
        }),
    ));
    upload.push(row(
        "Uploaded since start",
        u.uploaded_since_start.to_string(),
    ));
    out.push(("Uploads", upload));

    let h = &status.heartbeat;
    out.push((
        "Heartbeat",
        vec![
            row("Last heartbeat", when(h.last_at)),
            row("Clock offset", format!("{:+} ms", h.clock_offset_ms)),
        ],
    ));

    let p = &status.presence;
    let presence = if !p.collector_available && p.state.is_none() {
        vec![colored("Activity", "collector unavailable", Level::Warn)]
    } else {
        vec![
            match p.state.as_deref() {
                Some("active") => colored("You are", "active", Level::Good),
                Some("idle") => colored("You are", "idle", Level::Off),
                _ => row("You are", "—"),
            },
            row("Since", p.since.map_or("—".into(), clock)),
        ]
    };
    out.push(("Activity", presence));

    let cfg = &status.config;
    out.push((
        "Rules",
        vec![
            row("Fetched", when(cfg.fetched_at)),
            row(
                "Exclusions",
                format!(
                    "{} folders · {} apps · {} sites",
                    cfg.path_exclusions, cfg.app_exclusions, cfg.domain_exclusions
                ),
            ),
            row("Pause windows", cfg.pause_windows.to_string()),
            if cfg.paused {
                colored("Now", "paused", Level::Warn)
            } else {
                row("Now", "collecting")
            },
        ],
    ));

    out.push((
        "Agent",
        vec![
            row("Version", status.version.clone()),
            row("Process", status.pid.to_string()),
            row(
                "Started",
                status
                    .started_at
                    .map_or("—".into(), |t| format!("{} ({})", clock(t), ago(t, now))),
            ),
        ],
    ));
    out
}

/// "just now", "42 s ago", "5 min ago", "3 h ago", or a date.
pub fn ago(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let s = (now - t).num_seconds();
    match s {
        i64::MIN..=-1 => "just now".into(),
        0..=4 => "just now".into(),
        5..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => t.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
    }
}

fn clock(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use secretary_shared::local::{ConnectionStatus, StatusAccount};

    use super::*;

    fn now() -> DateTime<Utc> {
        "2026-11-20T12:00:00Z".parse().unwrap()
    }

    fn live(state: ConnectionState) -> AgentStatus {
        AgentStatus {
            running: true,
            written_at: Some(now()),
            connection: ConnectionStatus {
                state,
                last_error: Some("warehouse unreachable".into()),
                ..Default::default()
            },
            account: Some(StatusAccount {
                email: Some("dev@example.test".into()),
                server_url: "https://h:8443".into(),
                device_id: Some(4),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn headline_follows_the_daemon_state() {
        let level = |s: &Snapshot| summary(s, now()).0;
        assert_eq!(level(&Snapshot::Missing), Level::Off);
        assert_eq!(
            level(&Snapshot::Status(Box::new(live(ConnectionState::Online)))),
            Level::Good
        );
        assert_eq!(
            level(&Snapshot::Status(Box::new(live(ConnectionState::Offline)))),
            Level::Bad
        );
        assert_eq!(
            level(&Snapshot::Status(Box::new(live(
                ConnectionState::SignedOut
            )))),
            Level::Warn
        );

        let mut stale = live(ConnectionState::Online);
        stale.written_at = Some(now() - chrono::Duration::minutes(5));
        let (level, headline, detail) = summary(&Snapshot::Status(Box::new(stale)), now());
        assert_eq!(
            (level, headline.as_str()),
            (Level::Off, "Agent not responding")
        );
        assert_eq!(detail, "Last status 5 min ago.");

        let mut stopped = live(ConnectionState::Online);
        stopped.running = false;
        assert_eq!(
            summary(&Snapshot::Status(Box::new(stopped)), now()).1,
            "Agent stopped"
        );
    }

    #[test]
    fn offline_headline_says_why_and_what_waits() {
        let mut status = live(ConnectionState::Offline);
        status.upload.queued = 12;
        let (_, _, detail) = summary(&Snapshot::Status(Box::new(status)), now());
        assert_eq!(
            detail,
            "warehouse unreachable — retrying; 12 event(s) wait locally."
        );
    }

    #[test]
    fn rows_cover_every_section() {
        let sections: Vec<&str> = rows(&live(ConnectionState::Online), now())
            .iter()
            .map(|(s, _)| *s)
            .collect();
        assert_eq!(
            sections,
            [
                "Account",
                "Server",
                "Uploads",
                "Heartbeat",
                "Activity",
                "Rules",
                "Agent"
            ]
        );
        let account = &rows(&live(ConnectionState::Online), now())[0].1;
        assert_eq!(account[2].value, "#4");
    }

    #[test]
    fn ago_is_human() {
        let at = |s: i64| now() - chrono::Duration::seconds(s);
        assert_eq!(ago(at(2), now()), "just now");
        assert_eq!(ago(at(42), now()), "42 s ago");
        assert_eq!(ago(at(300), now()), "5 min ago");
        assert_eq!(ago(at(7200), now()), "2 h ago");
        assert_eq!(ago(now() + chrono::Duration::seconds(3), now()), "just now");
    }
}
