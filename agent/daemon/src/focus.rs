//! Focus collector (AG-9): `window.focus` intervals of the foreground app and
//! window title, while the user is active.
//!
//! [`FocusTracker`] is the state machine; the presence collector's loop feeds
//! it one sample per second from the same OS connection (no second thread).
//!
//! - An interval lasts while the same app + title stay in front.
//! - Going idle ends it at the last input; nothing is claimed while idle.
//! - Intervals are cut every [`MAX_INTERVAL`] so a long focus is not lost on
//!   a crash, and shorter than [`DEBOUNCE`] are dropped (alt-tab flicker).
//! - A gap between samples longer than [`SLEEP_GAP`] (sleep, suspend) ends
//!   the interval at the last sample.
//!
//! Titles go through the pipeline (exclusions, the configured redaction).

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use secretary_shared::Event;
use secretary_shared::event::{Payload, WindowFocus};
use secretary_shared::validate::MAX_TEXT;

use crate::buffer::agent_source;
use crate::platform::FocusedWindow;

/// Shorter focus intervals are dropped (AGENT plan §3.2: debounced 2 s).
pub const DEBOUNCE: Duration = Duration::from_secs(2);
/// A long focus is cut into intervals of at most this length.
pub const MAX_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// More time than this between two samples means the machine slept.
pub const SLEEP_GAP: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    app: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusInterval {
    pub app: Option<String>,
    pub title: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl FocusInterval {
    pub fn into_event(self) -> Event {
        Event::new(
            agent_source(),
            self.start,
            Some(self.end),
            Payload::WindowFocus(WindowFocus {
                app: self.app,
                title: self.title,
            }),
        )
    }
}

#[derive(Debug)]
pub struct FocusTracker {
    idle_after: Duration,
    open: Option<(Key, DateTime<Utc>)>,
    last_sample: Option<DateTime<Utc>>,
}

impl FocusTracker {
    pub fn new(idle_after: Duration) -> Self {
        Self {
            idle_after,
            open: None,
            last_sample: None,
        }
    }

    /// One sample: the foreground window (if any) and the time since the last
    /// input. Returns the intervals this sample completes.
    pub fn observe(
        &mut self,
        now: DateTime<Utc>,
        window: Option<&FocusedWindow>,
        idle: Duration,
    ) -> Vec<FocusInterval> {
        let mut done = Vec::new();
        let gap = self
            .last_sample
            .map(|last| (now - last).to_std().unwrap_or_default());
        if let (Some(gap), Some(last)) = (gap, self.last_sample)
            && gap > SLEEP_GAP
        {
            self.close(last, &mut done);
        }
        self.last_sample = Some(now);

        if idle >= self.idle_after {
            // Ends at the last input, never before the interval began.
            let last_input = now - TimeDelta::from_std(idle).unwrap_or(TimeDelta::zero());
            self.close(last_input, &mut done);
            return done;
        }
        let key = window.map(|w| Key {
            app: clean(w.app.as_deref()),
            title: clean(w.title.as_deref()),
        });
        match (&self.open, key) {
            (Some((open, start)), Some(key)) if *open == key => {
                if (now - *start).to_std().unwrap_or_default() >= MAX_INTERVAL {
                    self.close(now, &mut done);
                    self.open = Some((key, now));
                }
            }
            (_, Some(key)) if key.app.is_some() || key.title.is_some() => {
                self.close(now, &mut done);
                self.open = Some((key, now));
            }
            _ => self.close(now, &mut done), // nothing in front (desktop, lock screen)
        }
        done
    }

    /// Closes the open interval at shutdown: at most one sample period after
    /// the last sample (a starved thread claims nothing extra).
    pub fn finish(&mut self, now: DateTime<Utc>) -> Option<FocusInterval> {
        let mut done = Vec::new();
        let end = self
            .last_sample
            .map_or(now, |last| (last + TimeDelta::seconds(1)).min(now));
        self.close(end, &mut done);
        done.pop()
    }

    fn close(&mut self, at: DateTime<Utc>, done: &mut Vec<FocusInterval>) {
        if let Some((key, start)) = self.open.take() {
            let end = at.max(start);
            if (end - start).to_std().unwrap_or_default() >= DEBOUNCE {
                done.push(FocusInterval {
                    app: key.app,
                    title: key.title,
                    start,
                    end,
                });
            }
        }
    }
}

/// Trimmed, non-empty, and within the validation limit.
fn clean(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.chars().take(MAX_TEXT).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + s, 0).unwrap()
    }

    fn win(app: &str, title: &str) -> FocusedWindow {
        FocusedWindow {
            app: Some(app.into()),
            title: Some(title.into()),
            pid: Some(1),
        }
    }

    fn tracker() -> FocusTracker {
        FocusTracker::new(Duration::from_secs(60))
    }

    const ACTIVE: Duration = Duration::ZERO;

    /// Samples every 5 s from `from` to `to` (inclusive), as the loop would
    /// (it samples every second; 5 s keeps the tests short and under the
    /// sleep gap).
    fn feed(
        t: &mut FocusTracker,
        from: i64,
        to: i64,
        w: &FocusedWindow,
        idle: impl Fn(i64) -> u64,
    ) -> Vec<FocusInterval> {
        let mut done = Vec::new();
        for s in (from..=to).step_by(5) {
            done.extend(t.observe(at(s), Some(w), Duration::from_secs(idle(s))));
        }
        done
    }

    #[test]
    fn a_change_of_window_closes_the_interval() {
        let mut t = tracker();
        assert!(feed(&mut t, 0, 25, &win("Code.exe", "main.rs"), |_| 0).is_empty());
        let done = t.observe(at(30), Some(&win("chrome.exe", "Docs")), ACTIVE);
        assert_eq!(
            done,
            vec![FocusInterval {
                app: Some("Code.exe".into()),
                title: Some("main.rs".into()),
                start: at(0),
                end: at(30)
            }]
        );
        // The same app with another title is another interval.
        let done = t.observe(at(40), Some(&win("chrome.exe", "Issue 12")), ACTIVE);
        assert_eq!(done[0].title.as_deref(), Some("Docs"));
    }

    #[test]
    fn flicker_shorter_than_two_seconds_is_dropped() {
        let mut t = tracker();
        t.observe(at(0), Some(&win("a", "x")), ACTIVE);
        assert!(
            t.observe(at(1), Some(&win("b", "y")), ACTIVE).is_empty(),
            "a lasted 1 s"
        );
        let done = t.observe(at(10), Some(&win("a", "x")), ACTIVE);
        assert_eq!(
            (done[0].app.as_deref(), done[0].start, done[0].end),
            (Some("b"), at(1), at(10))
        );
    }

    #[test]
    fn idle_ends_the_interval_at_the_last_input() {
        let mut t = tracker();
        let w = win("a", "x");
        // Input until 30 s, then none: idle reaches the 60 s threshold at 90 s.
        let done = feed(&mut t, 0, 120, &w, |s| {
            if s <= 30 { 0 } else { (s - 30) as u64 }
        });
        assert_eq!(
            done.iter().map(|i| (i.start, i.end)).collect::<Vec<_>>(),
            [(at(0), at(30))]
        );
        // Back to work: a new interval from then.
        assert!(feed(&mut t, 125, 135, &w, |_| 0).is_empty());
        assert_eq!(
            t.finish(at(140)).map(|i| (i.start, i.end)),
            Some((at(125), at(136)))
        );
    }

    #[test]
    fn long_focus_is_cut_every_five_minutes() {
        let mut t = tracker();
        let done = feed(&mut t, 0, 700, &win("a", "x"), |_| 0);
        assert_eq!(
            done.iter().map(|i| (i.start, i.end)).collect::<Vec<_>>(),
            [(at(0), at(300)), (at(300), at(600))]
        );
    }

    #[test]
    fn sleep_gap_ends_at_the_last_sample() {
        let mut t = tracker();
        t.observe(at(0), Some(&win("a", "x")), ACTIVE);
        t.observe(at(5), Some(&win("a", "x")), ACTIVE);
        let done = t.observe(at(3600), Some(&win("a", "x")), ACTIVE);
        assert_eq!((done[0].start, done[0].end), (at(0), at(5)));
        assert_eq!(
            t.finish(at(3700)).map(|i| (i.start, i.end)),
            None,
            "reopened at wake, too short"
        );
    }

    #[test]
    fn no_window_or_empty_names_claim_nothing() {
        let mut t = tracker();
        t.observe(at(0), Some(&win("a", "x")), ACTIVE);
        let done = t.observe(at(10), None, ACTIVE); // desktop / lock screen
        assert_eq!(done.len(), 1);
        let blank = FocusedWindow {
            app: Some("  ".into()),
            title: None,
            pid: None,
        };
        assert!(t.observe(at(20), Some(&blank), ACTIVE).is_empty());
        assert!(t.finish(at(30)).is_none());
    }

    #[test]
    fn titles_are_capped_and_become_window_focus_events() {
        let mut t = tracker();
        feed(&mut t, 0, 55, &win("a", &"t".repeat(MAX_TEXT + 50)), |_| 0);
        let event = t.finish(at(60)).unwrap().into_event();
        assert_eq!(event.event_type(), "window.focus");
        let Payload::WindowFocus(m) = &event.payload else {
            unreachable!()
        };
        assert_eq!(m.title.as_ref().unwrap().chars().count(), MAX_TEXT);
        assert!(secretary_shared::validate::validate(&event, at(120)).is_ok());
    }
}
