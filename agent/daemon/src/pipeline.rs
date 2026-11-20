//! Event pipeline (AG-6): collector → validate → exclusions → pause → redact
//! → buffer (AGENT plan §3.1).
//!
//! Every collector hands its events to [`Pipeline::submit`]; nothing else
//! writes to the buffer. Dropped events are counted, never logged with their
//! contents.
//!
//! - **Validate** with `secretary_shared::validate` (device clock).
//! - **Exclusions** (AG-7): an event touching an excluded path or app is
//!   dropped whole.
//! - **Pause windows** (AG-7): point events inside a window are dropped;
//!   intervals are cut so only the parts outside every window remain.
//!   `agent.*` events are exempt (they report the pause itself).
//! - **Redact** with `secretary_shared::redact` (SH-5), then validate
//!   again (masking changes lengths), so nothing unredacted is buffered.
//!   Exclusions run on the raw event first: patterns name real paths and
//!   apps, not their redacted forms.

use std::sync::Arc;

use chrono::{DateTime, Duration as TimeDelta, Local, TimeZone, Utc};
use secretary_shared::config::{AgentConfig, Exclusions};
use secretary_shared::event::Payload;
use secretary_shared::redact;
use secretary_shared::validate::{self, Invalid};
use secretary_shared::{Event, EventId};
use tracing::{debug, warn};

use crate::buffer::Buffer;
use crate::remote_config::ConfigCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Buffered, as this many events (an interval cut by a pause window may
    /// become several).
    Buffered(usize),
    Invalid(Invalid),
    Excluded,
    Paused,
    /// The buffer could not be written (already logged).
    Failed,
}

pub struct Pipeline {
    buffer: Arc<Buffer>,
    config: Arc<ConfigCache>,
}

impl Pipeline {
    pub fn new(buffer: Arc<Buffer>, config: Arc<ConfigCache>) -> Self {
        Self { buffer, config }
    }

    pub fn submit(&self, event: Event) -> Outcome {
        let outcome = self.process(event, Utc::now(), &Local);
        match outcome {
            Outcome::Invalid(reason) => warn!(%reason, "dropped an invalid event"),
            Outcome::Excluded => debug!("dropped an excluded event"),
            Outcome::Paused => debug!("dropped an event inside a pause window"),
            Outcome::Buffered(_) | Outcome::Failed => {}
        }
        outcome
    }

    fn process<Tz: TimeZone>(&self, event: Event, now: DateTime<Utc>, tz: &Tz) -> Outcome {
        if let Err(reason) = validate::validate(&event, now) {
            return Outcome::Invalid(reason);
        }
        let active = self.config.current();
        if excluded(&event, &active.exclusions) {
            return Outcome::Excluded;
        }
        let pieces = if is_agent_event(&event) {
            vec![event]
        } else {
            outside_pauses(&active.config, event, tz)
        };
        if pieces.is_empty() {
            return Outcome::Paused;
        }
        let mut pieces = pieces;
        for piece in &mut pieces {
            redact::redact(piece);
            if let Err(reason) = validate::validate(piece, now) {
                return Outcome::Invalid(reason);
            }
        }
        for piece in &pieces {
            if let Err(err) = self.buffer.push(piece) {
                warn!(error = %format!("{err:#}"), "could not buffer an event");
                return Outcome::Failed;
            }
        }
        Outcome::Buffered(pieces.len())
    }
}

fn is_agent_event(event: &Event) -> bool {
    event.event_type().starts_with("agent.")
}

/// The event touches an excluded path or app.
pub fn excluded(event: &Event, exclusions: &Exclusions) -> bool {
    if exclusions.is_empty() {
        return false;
    }
    let root = event
        .repository
        .as_ref()
        .and_then(|r| r.root_path.as_deref());
    let mut paths: Vec<String> = root
        .into_iter()
        .chain(event.working_directory.as_deref())
        .map(str::to_owned)
        .collect();
    let mut file = |relative: &str| {
        if let Some(root) = root {
            paths.push(format!("{}/{relative}", root.trim_end_matches(['/', '\\'])));
        }
    };
    match &event.payload {
        Payload::WindowFocus(m) => {
            if m.app
                .as_deref()
                .is_some_and(|app| exclusions.excludes_app(app))
            {
                return true;
            }
        }
        Payload::EditorWorkspaceOpen(m) => paths.push(m.workspace_path.clone()),
        Payload::EditorFileFocus(m) => file(&m.file_path),
        Payload::EditorEditBurst(m) => file(&m.file_path),
        Payload::EditorSave(m) => file(&m.file_path),
        _ => {}
    }
    paths.iter().any(|p| exclusions.excludes_path(p))
}

/// The parts of `event` outside every pause window (local time in `tz`).
/// Point events are kept or dropped whole; the first piece of an interval
/// keeps the event id, later pieces get new ones.
fn outside_pauses<Tz: TimeZone>(config: &AgentConfig, event: Event, tz: &Tz) -> Vec<Event> {
    if config.pause_windows.is_empty() {
        return vec![event];
    }
    let start = event.timestamp_start;
    let paused = |at: DateTime<Utc>| {
        config
            .paused_at(at.with_timezone(tz).naive_local())
            .is_some()
    };
    let end = match event.timestamp_end {
        Some(end) if end > start => end,
        _ => {
            return if paused(start) {
                Vec::new()
            } else {
                vec![event]
            };
        }
    };

    // Cut at every window edge inside (start, end), then keep the spans
    // whose midpoint is not paused.
    let mut cuts = vec![start, end];
    let first_day = start.with_timezone(tz).date_naive() - TimeDelta::days(1);
    let last_day = end.with_timezone(tz).date_naive() + TimeDelta::days(1);
    for day in first_day.iter_days().take_while(|d| *d <= last_day) {
        for window in &config.pause_windows {
            for edge in [window.start, window.end] {
                if let Some(at) = tz.from_local_datetime(&day.and_time(edge)).earliest() {
                    let at = at.with_timezone(&Utc);
                    if start < at && at < end {
                        cuts.push(at);
                    }
                }
            }
        }
    }
    cuts.sort();
    cuts.dedup();

    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
    for pair in cuts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if paused(a + (b - a) / 2) {
            continue;
        }
        match spans.last_mut() {
            Some(last) if last.1 == a => last.1 = b,
            _ => spans.push((a, b)),
        }
    }

    spans
        .into_iter()
        .enumerate()
        .map(|(i, (a, b))| {
            let mut piece = event.clone();
            if i > 0 {
                piece.event_id = EventId::new();
            }
            piece.timestamp_start = a;
            piece.timestamp_end = Some(b);
            piece
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, NaiveTime};
    use secretary_shared::config::PauseWindow;
    use secretary_shared::event::{AgentPauseChange, EditorSave, Empty, PauseReason, WindowFocus};
    use secretary_shared::{RepositoryRef, Source};

    use super::*;

    fn ts(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    fn now() -> DateTime<Utc> {
        ts("2026-11-20T18:00:00Z")
    }

    fn active(start: &str, end: &str) -> Event {
        Event::new(
            Source::new("agent", "0.1.0"),
            ts(start),
            Some(ts(end)),
            Payload::PresenceActive(Empty {}),
        )
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        buffer: Arc<Buffer>,
        cache: Arc<ConfigCache>,
        pipeline: Pipeline,
    }

    fn fixture(config: AgentConfig) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let buffer = Arc::new(Buffer::open(&dir.path().join("buffer.sqlite3")).unwrap());
        let cache = Arc::new(ConfigCache::load(&dir.path().join("agent_config.json")));
        cache.replace(config, None).unwrap();
        let pipeline = Pipeline::new(buffer.clone(), cache.clone());
        Fixture {
            _dir: dir,
            buffer,
            cache,
            pipeline,
        }
    }

    fn lunch() -> AgentConfig {
        AgentConfig {
            pause_windows: vec![PauseWindow {
                start: hm(12, 0),
                end: hm(13, 0),
                days: vec![],
            }],
            ..AgentConfig::default()
        }
    }

    #[test]
    fn valid_events_are_buffered() {
        let f = fixture(AgentConfig::default());
        let event = active("2026-11-20T09:00:00Z", "2026-11-20T09:05:00Z");
        assert_eq!(
            f.pipeline.process(event.clone(), now(), &Utc),
            Outcome::Buffered(1)
        );
        assert_eq!(f.buffer.peek(10).unwrap(), vec![event]);
    }

    #[test]
    fn invalid_events_never_reach_the_buffer() {
        let f = fixture(AgentConfig::default());
        let backwards = active("2026-11-20T09:05:00Z", "2026-11-20T09:00:00Z");
        assert_eq!(
            f.pipeline.process(backwards, now(), &Utc),
            Outcome::Invalid(Invalid::InvalidTimestamps)
        );
        let future = active("2026-11-20T18:10:00Z", "2026-11-20T18:11:00Z");
        assert_eq!(
            f.pipeline.process(future, now(), &Utc),
            Outcome::Invalid(Invalid::TimestampInFuture)
        );
        assert_eq!(f.buffer.len().unwrap(), 0);
    }

    #[test]
    fn excluded_paths_and_apps_are_dropped() {
        let f = fixture(AgentConfig {
            exclusions: Exclusions {
                paths: vec!["/home/dev/clients/acme".into()],
                apps: vec!["KeePassXC".into()],
                ..Exclusions::default()
            },
            ..AgentConfig::default()
        });
        let at = ts("2026-11-20T09:00:00Z");
        let mut save = Event::new(
            Source::new("vscode", "0.1.0"),
            at,
            None,
            Payload::EditorSave(EditorSave {
                file_path: "src/main.rs".into(),
            }),
        );
        save.repository = Some(RepositoryRef {
            root_path: Some("/home/dev/clients/acme".into()),
            remote_url: None,
        });
        assert_eq!(
            f.pipeline.process(save.clone(), now(), &Utc),
            Outcome::Excluded
        );

        save.repository = Some(RepositoryRef {
            root_path: Some("/home/dev/api".into()),
            remote_url: None,
        });
        assert_eq!(f.pipeline.process(save, now(), &Utc), Outcome::Buffered(1));

        let focus = |app: &str| {
            Event::new(
                Source::new("agent", "0.1.0"),
                at,
                Some(at + TimeDelta::seconds(30)),
                Payload::WindowFocus(WindowFocus {
                    app: Some(app.into()),
                    title: None,
                }),
            )
        };
        assert_eq!(
            f.pipeline.process(focus("keepassxc.exe"), now(), &Utc),
            Outcome::Excluded
        );
        assert_eq!(
            f.pipeline.process(focus("Code.exe"), now(), &Utc),
            Outcome::Buffered(1)
        );
    }

    #[test]
    fn config_changes_apply_to_the_next_event() {
        let f = fixture(AgentConfig::default());
        let mut config = AgentConfig::default();
        config.exclusions.paths.push("/secret".into());
        f.cache.replace(config, None).unwrap();
        let mut event = active("2026-11-20T09:00:00Z", "2026-11-20T09:05:00Z");
        event.working_directory = Some("/secret/x".into());
        assert_eq!(f.pipeline.process(event, now(), &Utc), Outcome::Excluded);
    }

    #[test]
    fn intervals_are_cut_around_pause_windows() {
        let f = fixture(lunch());
        // 11:50–13:10 UTC → 11:50–12:00 and 13:00–13:10.
        let event = active("2026-11-20T11:50:00Z", "2026-11-20T13:10:00Z");
        let id = event.event_id;
        assert_eq!(f.pipeline.process(event, now(), &Utc), Outcome::Buffered(2));
        let stored = f.buffer.peek(10).unwrap();
        let spans: Vec<_> = stored
            .iter()
            .map(|e| (e.timestamp_start, e.timestamp_end.unwrap()))
            .collect();
        assert_eq!(
            spans,
            vec![
                (ts("2026-11-20T11:50:00Z"), ts("2026-11-20T12:00:00Z")),
                (ts("2026-11-20T13:00:00Z"), ts("2026-11-20T13:10:00Z")),
            ]
        );
        assert_eq!(stored[0].event_id, id);
        assert_ne!(stored[1].event_id, id);

        let inside = active("2026-11-20T12:10:00Z", "2026-11-20T12:20:00Z");
        assert_eq!(f.pipeline.process(inside, now(), &Utc), Outcome::Paused);
    }

    #[test]
    fn pause_windows_use_local_time() {
        let f = fixture(lunch());
        let utc_plus_1 = FixedOffset::east_opt(3600).unwrap();
        // 11:10–11:20 UTC is 12:10–12:20 local: paused.
        let event = active("2026-11-20T11:10:00Z", "2026-11-20T11:20:00Z");
        assert_eq!(
            f.pipeline.process(event.clone(), now(), &utc_plus_1),
            Outcome::Paused
        );
        assert_eq!(f.pipeline.process(event, now(), &Utc), Outcome::Buffered(1));
    }

    #[test]
    fn points_in_a_window_are_dropped_but_agent_events_pass() {
        let f = fixture(lunch());
        let at = ts("2026-11-20T12:30:00Z");
        let save = Event::new(
            Source::new("vscode", "0.1.0"),
            at,
            None,
            Payload::EditorSave(EditorSave {
                file_path: "a.rs".into(),
            }),
        );
        assert_eq!(f.pipeline.process(save, now(), &Utc), Outcome::Paused);
        let pause = Event::new(
            Source::new("agent", "0.1.0"),
            at,
            None,
            Payload::AgentPause(AgentPauseChange {
                reason: PauseReason::ExclusionWindow,
            }),
        );
        assert_eq!(f.pipeline.process(pause, now(), &Utc), Outcome::Buffered(1));
    }

    /// The shared redaction corpus (SH-5), run through the agent path: no
    /// secret reaches the buffer file, in any field the agent fills.
    #[test]
    fn redaction_corpus_never_reaches_the_buffer() {
        #[derive(serde::Deserialize)]
        struct Corpus {
            items: Vec<Item>,
        }
        #[derive(serde::Deserialize)]
        struct Item {
            template: String,
            secret: Vec<String>,
        }
        let corpus: Corpus = serde_json::from_str(include_str!(
            "../../../shared/fixtures/redaction-corpus-v1.json"
        ))
        .unwrap();
        assert!(corpus.items.len() >= 100);

        let f = fixture(AgentConfig::default());
        let at = ts("2026-11-20T09:00:00Z");
        let mut secrets = Vec::new();
        for item in &corpus.items {
            let secret = item.secret.concat();
            let text = item.template.replace("{s}", &secret);
            let mut focus = Event::new(
                Source::new("agent", "0.1.0"),
                at,
                Some(at + TimeDelta::seconds(30)),
                Payload::WindowFocus(WindowFocus {
                    app: Some("Code.exe".into()),
                    title: Some(text.clone()),
                }),
            );
            focus.project_hint = Some(text.chars().take(200).collect());
            let outcome = f.pipeline.process(focus, now(), &Utc);
            assert_eq!(outcome, Outcome::Buffered(1), "{}", item.template);
            secrets.push(secret);
        }

        let stored = serde_json::to_string(&f.buffer.peek(1000).unwrap()).unwrap();
        // The database file and its WAL, byte for byte.
        let mut raw = Vec::new();
        for entry in std::fs::read_dir(f._dir.path()).unwrap() {
            raw.extend(std::fs::read(entry.unwrap().path()).unwrap());
        }
        let raw = String::from_utf8_lossy(&raw);
        let survivors: Vec<&String> = secrets
            .iter()
            .filter(|s| stored.contains(s.as_str()) || raw.contains(s.as_str()))
            .collect();
        assert!(
            survivors.is_empty(),
            "{} secrets reached the buffer:\n{survivors:#?}",
            survivors.len()
        );
    }

    #[test]
    fn overnight_windows_cut_across_midnight() {
        let f = fixture(AgentConfig {
            pause_windows: vec![PauseWindow {
                start: hm(23, 0),
                end: hm(1, 0),
                days: vec![],
            }],
            ..AgentConfig::default()
        });
        let event = active("2026-11-19T22:30:00Z", "2026-11-20T01:30:00Z");
        assert_eq!(f.pipeline.process(event, now(), &Utc), Outcome::Buffered(2));
        let spans: Vec<_> = f
            .buffer
            .peek(10)
            .unwrap()
            .iter()
            .map(|e| (e.timestamp_start, e.timestamp_end.unwrap()))
            .collect();
        assert_eq!(
            spans,
            vec![
                (ts("2026-11-19T22:30:00Z"), ts("2026-11-19T23:00:00Z")),
                (ts("2026-11-20T01:00:00Z"), ts("2026-11-20T01:30:00Z")),
            ]
        );
    }
}
