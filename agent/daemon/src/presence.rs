//! Presence collector (AG-8): `presence.active` / `presence.idle` intervals.
//!
//! Polls the OS idle time every [`POLL_EVERY`] and emits an interval whenever
//! the state changes, plus a checkpoint every [`MAX_INTERVAL`] so long
//! stretches reach the warehouse without waiting for the next change.
//!
//! - Idle starts at the **last input**, not when the threshold is crossed:
//!   after [`IDLE_AFTER`] without input, the idle interval is back-dated to
//!   `now - idle_time` (never before the current interval's start, and
//!   never before the collector started: time before that belongs to no run,
//!   and a restart must not claim the same idle stretch twice).
//! - A gap between polls much longer than the poll interval means the
//!   machine slept or the process was suspended: the open interval is closed
//!   at the last poll and nothing is claimed for the gap.
//! - On shutdown the open interval is closed and buffered.
//!
//! No metadata: presence events carry no titles, paths or input content.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use secretary_shared::Event;
use secretary_shared::event::{Empty, Payload};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::buffer::agent_source;
use crate::focus::FocusTracker;
use crate::pipeline::Pipeline;
use crate::platform::Platform;
use crate::probe::{Presence, classify};
use crate::status::StatusBoard;

pub const POLL_EVERY: Duration = Duration::from_secs(5);
/// The loop's tick: focus is sampled every tick, presence every `POLL_EVERY`.
pub const FOCUS_EVERY: Duration = Duration::from_secs(1);
/// No input for this long counts as idle. The engine applies its own
/// (longer, configurable) idle threshold on top of these intervals.
pub const IDLE_AFTER: Duration = Duration::from_secs(60);
pub const MAX_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub presence: Presence,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Interval {
    pub fn into_event(self) -> Event {
        let payload = match self.presence {
            Presence::Active => Payload::PresenceActive(Empty {}),
            Presence::Idle => Payload::PresenceIdle(Empty {}),
        };
        Event::new(agent_source(), self.start, Some(self.end), payload)
    }
}

/// The state machine, separate from the OS so it can be tested with a fake clock.
#[derive(Debug)]
pub struct Tracker {
    poll: Duration,
    idle_after: Duration,
    max_interval: Duration,
    open: Option<(Presence, DateTime<Utc>)>,
    last_poll: Option<DateTime<Utc>>,
}

impl Tracker {
    pub fn new(poll: Duration, idle_after: Duration, max_interval: Duration) -> Self {
        Self {
            poll,
            idle_after,
            max_interval,
            open: None,
            last_poll: None,
        }
    }

    /// The open interval: current presence and since when.
    pub fn current(&self) -> Option<(Presence, DateTime<Utc>)> {
        self.open
    }

    /// Feeds one sample; returns the intervals it completes.
    pub fn observe(&mut self, now: DateTime<Utc>, idle: Duration) -> Vec<Interval> {
        let mut done = Vec::new();
        // Nothing is claimed before the first sample (collector start) or across a sleep gap.
        let mut floor = self.last_poll.is_none().then_some(now);

        // Suspend/sleep: close at the last poll, claim nothing for the gap.
        if let Some(last) = self.last_poll {
            let gap = (now - last).to_std().unwrap_or_default();
            if gap > self.poll * 3 {
                if let Some(interval) = self.close(last) {
                    done.push(interval);
                }
                floor = Some(now);
            }
        }
        self.last_poll = Some(now);

        let state = classify(idle, self.idle_after);
        let last_input = now - chrono::Duration::from_std(idle).unwrap_or_default();

        match self.open {
            None => {
                let start = match state {
                    Presence::Active => now,
                    // Back-date to the last input, but not before the floor.
                    Presence::Idle => floor.map_or(last_input, |f| last_input.max(f)),
                };
                self.open = Some((state, start.min(now)));
            }
            Some((open, start)) if open == state => {
                if (now - start).to_std().unwrap_or_default() >= self.max_interval {
                    done.push(Interval {
                        presence: open,
                        start,
                        end: now,
                    });
                    self.open = Some((open, now));
                }
            }
            Some((Presence::Active, start)) => {
                let idle_start = last_input.max(start).min(now);
                if idle_start > start {
                    done.push(Interval {
                        presence: Presence::Active,
                        start,
                        end: idle_start,
                    });
                }
                self.open = Some((Presence::Idle, idle_start));
            }
            Some((Presence::Idle, start)) => {
                done.push(Interval {
                    presence: Presence::Idle,
                    start,
                    end: now,
                });
                self.open = Some((Presence::Active, now));
            }
        }
        done
    }

    /// Closes the open interval (shutdown).
    pub fn finish(&mut self, now: DateTime<Utc>) -> Option<Interval> {
        let end = self.last_poll.map_or(now, |last| {
            now.min(last + chrono::Duration::from_std(self.poll).unwrap_or_default())
        });
        self.close(end)
    }

    fn close(&mut self, end: DateTime<Utc>) -> Option<Interval> {
        let (presence, start) = self.open.take()?;
        (end > start).then_some(Interval {
            presence,
            start,
            end,
        })
    }
}

/// Runs the collector on a blocking thread (OS handles need not be `Send`).
pub async fn run(
    pipeline: Arc<Pipeline>,
    status: Arc<StatusBoard>,
    token: CancellationToken,
) -> anyhow::Result<()> {
    let handle = tokio::task::spawn_blocking(move || collect(&pipeline, &status, &token));
    handle.await?;
    Ok(())
}

fn collect(pipeline: &Pipeline, status: &StatusBoard, token: &CancellationToken) {
    let mut platform = match Platform::connect() {
        Ok(platform) => platform,
        Err(err) => {
            warn!(error = %err, "presence collector unavailable on this session");
            status.update(|s| s.presence.collector_available = false);
            return;
        }
    };
    status.update(|s| s.presence.collector_available = true);
    info!(
        poll_s = POLL_EVERY.as_secs(),
        idle_after_s = IDLE_AFTER.as_secs(),
        "presence collector started"
    );
    let mut tracker = Tracker::new(POLL_EVERY, IDLE_AFTER, MAX_INTERVAL);
    // Focus (AG-9) shares this thread and OS connection, sampled every second.
    let mut focus = FocusTracker::new(IDLE_AFTER);
    let titles = platform.titles_available();
    let mut last_error: Option<String> = None;
    let ticks_per_poll = (POLL_EVERY.as_secs() / FOCUS_EVERY.as_secs()).max(1);
    let mut tick: u64 = 0;

    while !token.is_cancelled() {
        let now = Utc::now();
        match platform.idle_time() {
            Ok(idle) => {
                last_error = None;
                let window = platform.focused_window().ok().flatten().map(|mut w| {
                    if !titles {
                        w.title = None;
                    }
                    w
                });
                for interval in focus.observe(now, window.as_ref(), idle) {
                    pipeline.submit(interval.into_event());
                }
                if tick.is_multiple_of(ticks_per_poll) {
                    for interval in tracker.observe(now, idle) {
                        pipeline.submit(interval.into_event());
                    }
                    let current = tracker.current();
                    status.update(|s| {
                        s.presence.state = current.map(|(p, _)| match p {
                            Presence::Active => "active".to_owned(),
                            Presence::Idle => "idle".to_owned(),
                        });
                        s.presence.since = current.map(|(_, since)| since);
                    });
                }
            }
            Err(err) => {
                let kind = err.to_string();
                if last_error.as_ref() != Some(&kind) {
                    warn!(error = %kind, "could not read idle time");
                    last_error = Some(kind);
                }
            }
        }
        tick += 1;
        sleep_unless_cancelled(token, FOCUS_EVERY);
    }

    let now = Utc::now();
    if let Some(interval) = focus.finish(now) {
        pipeline.submit(interval.into_event());
    }
    if let Some(interval) = tracker.finish(now) {
        pipeline.submit(interval.into_event());
    }
    debug!("presence collector stopped");
}

fn sleep_unless_cancelled(token: &CancellationToken, total: Duration) {
    let step = Duration::from_millis(100);
    let mut slept = Duration::ZERO;
    while slept < total && !token.is_cancelled() {
        std::thread::sleep(step);
        slept += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLL: Duration = Duration::from_secs(5);

    fn at(secs: i64) -> DateTime<Utc> {
        "2026-11-07T09:00:00Z".parse::<DateTime<Utc>>().unwrap() + chrono::Duration::seconds(secs)
    }

    fn tracker() -> Tracker {
        Tracker::new(POLL, Duration::from_secs(60), Duration::from_secs(300))
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn idle_is_backdated_to_last_input() {
        let mut t = tracker();
        let mut out = Vec::new();
        // Active from t=0; last input at t=100; idle noticed at t=160.
        let mut now = 0;
        while now <= 200 {
            let idle = if now <= 100 { 0 } else { (now - 100) as u64 };
            out.extend(t.observe(at(now), secs(idle)));
            now += 5;
        }
        // Input again at t=200.
        out.extend(t.observe(at(205), secs(0)));
        assert_eq!(
            out,
            vec![
                Interval {
                    presence: Presence::Active,
                    start: at(0),
                    end: at(100)
                },
                Interval {
                    presence: Presence::Idle,
                    start: at(100),
                    end: at(205)
                },
            ]
        );
    }

    #[test]
    fn long_stretches_are_checkpointed() {
        let mut t = tracker();
        let mut out = Vec::new();
        let mut now = 0;
        while now <= 700 {
            out.extend(t.observe(at(now), secs(1)));
            now += 5;
        }
        assert_eq!(
            out,
            vec![
                Interval {
                    presence: Presence::Active,
                    start: at(0),
                    end: at(300)
                },
                Interval {
                    presence: Presence::Active,
                    start: at(300),
                    end: at(600)
                },
            ]
        );
        assert_eq!(
            t.finish(at(702)),
            Some(Interval {
                presence: Presence::Active,
                start: at(600),
                end: at(702)
            })
        );
    }

    #[test]
    fn sleep_gap_closes_the_interval_and_claims_nothing() {
        let mut t = tracker();
        assert!(t.observe(at(0), secs(0)).is_empty());
        assert!(t.observe(at(5), secs(0)).is_empty());
        // Laptop lid closed for an hour; on wake the OS reports a long idle time.
        let out = t.observe(at(3605), secs(3600));
        assert_eq!(
            out,
            vec![Interval {
                presence: Presence::Active,
                start: at(0),
                end: at(5)
            }]
        );
        // The idle interval starts at wake-up, not inside the sleep.
        let out = t.observe(at(3610), secs(0));
        assert_eq!(
            out,
            vec![Interval {
                presence: Presence::Idle,
                start: at(3605),
                end: at(3610)
            }]
        );
    }

    #[test]
    fn starting_while_idle_claims_nothing_before_start() {
        let mut t = tracker();
        assert!(t.observe(at(0), secs(120)).is_empty());
        let out = t.observe(at(5), secs(0));
        assert_eq!(
            out,
            vec![Interval {
                presence: Presence::Idle,
                start: at(0),
                end: at(5)
            }]
        );
    }

    #[test]
    fn finish_does_not_claim_time_after_the_last_poll() {
        let mut t = tracker();
        t.observe(at(0), secs(0));
        t.observe(at(5), secs(0));
        // Shut down long after the last poll (e.g. the thread was starved).
        assert_eq!(
            t.finish(at(100)),
            Some(Interval {
                presence: Presence::Active,
                start: at(0),
                end: at(10)
            })
        );
    }

    #[test]
    fn intervals_become_presence_events() {
        let event = Interval {
            presence: Presence::Idle,
            start: at(0),
            end: at(60),
        }
        .into_event();
        assert_eq!(event.event_type(), "presence.idle");
        assert_eq!(event.timestamp_end, Some(at(60)));
        assert_eq!(
            event.source.to_string(),
            format!("agent@{}", env!("CARGO_PKG_VERSION"))
        );
    }
}
