//! Engine inputs and outputs. The core ([`crate::correlate`], [`crate::calculate`])
//! is a pure function of a [`Context`]: no database, no clock, no randomness.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone, Utc};
use chrono_tz::Tz;
use secretary_shared::Event;
use secretary_shared::rules::Rule;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ALGORITHM_VERSION;

pub type Time = DateTime<Utc>;

/// A half-open time range `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub start: Time,
    pub end: Time,
}

impl Span {
    pub fn new(start: Time, end: Time) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> TimeDelta {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn intersect(&self, other: &Span) -> Option<Span> {
        let s = Span::new(self.start.max(other.start), self.end.min(other.end));
        (!s.is_empty()).then_some(s)
    }
}

/// Calculation settings (blueprint Appendix A). Part of [`Settings::config_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Same-project pieces this close are one session, if the user was present in between.
    pub merge_gap_secs: i64,
    /// Shorter sessions are dropped.
    pub min_session_secs: i64,
    /// Credit for point events (save, commit, checkout, merge) and the
    /// minimum for an edit burst, counted back from the event.
    pub point_credit_secs: i64,
    /// Temporal context window (§10.1 step 5).
    pub context_window_secs: i64,
    /// Presence gaps up to this long count as present. The agent marks idle
    /// after 60 s without input; the blueprint's idle threshold is 5 min, so
    /// the 4 min in between are reading and thinking time.
    pub presence_gap_fill_secs: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            merge_gap_secs: 10 * 60,
            min_session_secs: 60,
            point_credit_secs: 2 * 60,
            context_window_secs: 5 * 60,
            presence_gap_fill_secs: 4 * 60,
        }
    }
}

impl Settings {
    /// SHA-256 (hex) of the algorithm version and these settings.
    pub fn config_hash(&self) -> String {
        let json = serde_json::to_string(self).expect("settings serialize");
        let digest = Sha256::digest(format!("v{ALGORITHM_VERSION}:{json}").as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn secs(v: i64) -> TimeDelta {
        TimeDelta::seconds(v)
    }
}

/// One stored event; timestamps are the server-adjusted ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayEvent {
    pub device_id: i64,
    pub event: Event,
}

impl DayEvent {
    pub fn id(&self) -> Uuid {
        *self.event.event_id.as_uuid()
    }

    pub fn span(&self) -> Span {
        let start = self.event.timestamp_start;
        Span::new(start, self.event.timestamp_end.unwrap_or(start).max(start))
    }
}

/// A mapping rule; `shared` = a workspace rule (no owner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRow {
    #[serde(flatten)]
    pub rule: Rule,
    #[serde(default)]
    pub shared: bool,
}

/// Everything one (user, local day) calculation depends on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub local_day: NaiveDate,
    pub time_zone: String,
    /// Events overlapping the day widened by [`DAY_MARGIN_SECS`], any order.
    pub events: Vec<DayEvent>,
    #[serde(default)]
    pub rules: Vec<RuleRow>,
    /// Manual assignments: event → project (`None` = "not project work").
    #[serde(default)]
    pub overrides: BTreeMap<Uuid, Option<i64>>,
    /// Time-range corrections of this local day.
    #[serde(default)]
    pub adjustments: Vec<Adjustment>,
    #[serde(default)]
    pub settings: Settings,
}

/// Context loaded around each day: lets sessions near midnight see their neighbours.
pub const DAY_MARGIN_SECS: i64 = 3600;

impl Context {
    /// The local day as UTC `[start, end)`, DST-aware.
    pub fn day(&self) -> Span {
        let tz = parse_tz(&self.time_zone);
        day_bounds(tz, self.local_day)
    }

    pub fn window(&self) -> Span {
        let day = self.day();
        let m = TimeDelta::seconds(DAY_MARGIN_SECS);
        Span::new(day.start - m, day.end + m)
    }
}

/// An unknown zone name falls back to UTC (the dashboard validates names).
pub fn parse_tz(name: &str) -> Tz {
    name.parse().unwrap_or(Tz::UTC)
}

/// Local midnight to the next local midnight, in UTC. A midnight skipped by
/// a DST change starts at the first valid local time after it.
pub fn day_bounds(tz: Tz, day: NaiveDate) -> Span {
    let midnight = |d: NaiveDate| -> Time {
        let mut t = d.and_hms_opt(0, 0, 0).expect("midnight");
        for _ in 0..4 {
            if let Some(local) = tz.from_local_datetime(&t).earliest() {
                return local.with_timezone(&Utc);
            }
            t += TimeDelta::minutes(30);
        }
        Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))
    };
    let next = day.succ_opt().expect("date in range");
    Span::new(midnight(day), midnight(next))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    User,
    Rule,
    SharedRule,
    Temporal,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::User => "user",
            Method::Rule => "rule",
            Method::SharedRule => "shared_rule",
            Method::Temporal => "temporal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub event_id: Uuid,
    pub project_id: i64,
    pub method: Method,
    pub confidence: f64,
    pub rule_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Active,
    Waiting,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Active => "active",
            Kind::Waiting => "waiting",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// `None` = unassigned.
    pub project_id: Option<i64>,
    pub kind: Kind,
    pub start: Time,
    pub end: Time,
    #[serde(default)]
    pub low_confidence: bool,
    #[serde(default)]
    pub estimated: bool,
    /// Added by the user (an `add` adjustment); has no evidence.
    #[serde(default, skip_serializing_if = "is_false")]
    pub manual: bool,
    /// Sorted, unique.
    pub evidence: Vec<Uuid>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjustmentKind {
    /// Manual time for `project_id`; replaces whatever the range held.
    Add,
    /// Drop the range (only from `from_project_id` when set). Trims and
    /// deletes are removes.
    Remove,
    /// Move the range to `project_id` (only from `from_project_id` when set).
    /// Reassigning and splitting sessions are reassigns.
    Reassign,
}

/// A user's time-range correction (`time_adjustments`), anchored to time and
/// project so it survives recalculation (EN-10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjustment {
    /// Applied in id order (the order the user made them).
    pub id: i64,
    pub kind: AdjustmentKind,
    pub start: Time,
    pub end: Time,
    #[serde(default)]
    pub project_id: Option<i64>,
    #[serde(default)]
    pub from_project_id: Option<i64>,
}

/// Derived rows for one (user, local day).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub assignments: Vec<Assignment>,
    pub sessions: Vec<Session>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn day_bounds_follow_dst() {
        let paris = parse_tz("Europe/Paris");
        // Spring forward: 23 h; fall back: 25 h; a normal day: 24 h.
        assert_eq!(
            day_bounds(paris, d("2026-03-29")).len(),
            TimeDelta::hours(23)
        );
        assert_eq!(
            day_bounds(paris, d("2026-10-25")).len(),
            TimeDelta::hours(25)
        );
        let nov = day_bounds(paris, d("2026-11-20"));
        assert_eq!(nov.start.to_rfc3339(), "2026-11-19T23:00:00+00:00");
        assert_eq!(nov.len(), TimeDelta::hours(24));
    }

    #[test]
    fn skipped_midnight_starts_at_first_valid_time() {
        // America/Santiago skips 00:00–01:00 on its spring-forward day.
        let santiago = parse_tz("America/Santiago");
        let day = day_bounds(santiago, d("2026-09-06"));
        assert_eq!(day.start.to_rfc3339(), "2026-09-06T04:00:00+00:00");
    }

    #[test]
    fn unknown_zone_is_utc_and_hash_is_stable() {
        assert_eq!(parse_tz("Not/AZone"), Tz::UTC);
        let h = Settings::default().config_hash();
        assert_eq!(h.len(), 64);
        assert_eq!(h, Settings::default().config_hash());
        let other = Settings {
            merge_gap_secs: 1,
            ..Settings::default()
        };
        assert_ne!(h, other.config_hash());
    }
}
