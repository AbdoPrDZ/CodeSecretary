//! Agent configuration (`GET /v1/agent/config`, SH-8) and the default values
//! of blueprint Appendix A.
//!
//! The warehouse builds an [`AgentConfig`] from the user's settings and
//! exclusions and serves it with an `ETag`; the agent caches it (AG-7) and
//! applies it in its pipeline (AG-6). Every field has a serde default, so an
//! older agent ignores new fields and a newer one fills missing ones.
//!
//! Matching is deliberately generous: an exclusion that matches too much
//! only hides data, one that matches too little leaks it. All matching is
//! case-insensitive and treats `\` as `/`.

use std::time::Duration;

use chrono::{Datelike, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

/// Appendix A defaults. Agent values are also [`AgentConfig`] defaults;
/// engine values are used by `engine/` (and shown in the dashboard).
pub mod defaults {
    use std::time::Duration;

    // Engine (blueprint §10–§11).
    /// Gap without input after which presence counts as idle (§11.2 step 1).
    pub const IDLE_THRESHOLD: Duration = Duration::from_secs(5 * 60);
    /// Sessions closer than this merge (§11.2 step 5).
    pub const MERGE_GAP: Duration = Duration::from_secs(10 * 60);
    /// Shorter sessions are dropped (§11.2 step 6).
    pub const MIN_SESSION: Duration = Duration::from_secs(60);
    /// Temporal context window for attribution (§10.1 step 5).
    pub const CONTEXT_WINDOW: Duration = Duration::from_secs(5 * 60);
    /// Sessions below this confidence go to review (§10.1).
    pub const REVIEW_CONFIDENCE: f64 = 0.7;
    /// Engine job debounce per (user, local day) (§11.1).
    pub const ENGINE_DEBOUNCE: Duration = Duration::from_secs(2 * 60);
    /// Raw event retention, in months (§13).
    pub const RAW_RETENTION_MONTHS: u32 = 12;
    /// AI assistance is opt-in (§12.2).
    pub const AI_ENABLED: bool = false;

    // Agent (§8.1, §8.3).
    /// Upload at least this often…
    pub const UPLOAD_INTERVAL: Duration = Duration::from_secs(60);
    /// …or as soon as this many events are buffered.
    pub const UPLOAD_MAX_EVENTS: u32 = 500;
    /// Buffered events older than this are dropped (with an overflow marker).
    pub const BUFFER_MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
    pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);
    /// VS Code edit-burst aggregation window.
    pub const EDIT_AGGREGATION_WINDOW: Duration = Duration::from_secs(30);
    /// The agent re-fetches its config at least this often (AG-7 acceptance:
    /// a dashboard change applies within 5 min).
    pub const CONFIG_REFRESH: Duration = Duration::from_secs(5 * 60);
}

/// Version of the redaction rule set (SHARED §4.2) the agent must apply.
pub const REDACTION_VERSION: u32 = 1;

/// `GET /v1/agent/config` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
#[serde(default)]
pub struct AgentConfig {
    pub exclusions: Exclusions,
    /// Collection is paused inside these windows (device local time).
    pub pause_windows: Vec<PauseWindow>,
    /// Directories the Git tracker scans for repositories (AG-10).
    pub project_roots: Vec<String>,
    pub redaction_version: u32,
    /// What the agent masks before buffering (D18).
    pub redaction: RedactionLevel,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub upload_interval_secs: u64,
    pub upload_max_events: u32,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub heartbeat_interval_secs: u64,
    pub buffer_max_age_days: u32,
    #[cfg_attr(feature = "codegen", ts(type = "number"))]
    pub edit_aggregation_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            exclusions: Exclusions::default(),
            pause_windows: Vec::new(),
            project_roots: Vec::new(),
            redaction_version: REDACTION_VERSION,
            redaction: RedactionLevel::default(),
            upload_interval_secs: defaults::UPLOAD_INTERVAL.as_secs(),
            upload_max_events: defaults::UPLOAD_MAX_EVENTS,
            heartbeat_interval_secs: defaults::HEARTBEAT_INTERVAL.as_secs(),
            buffer_max_age_days: (defaults::BUFFER_MAX_AGE.as_secs() / 86_400) as u32,
            edit_aggregation_secs: defaults::EDIT_AGGREGATION_WINDOW.as_secs(),
        }
    }
}

impl AgentConfig {
    pub fn upload_interval(&self) -> Duration {
        Duration::from_secs(self.upload_interval_secs.clamp(5, 3600))
    }

    pub fn heartbeat_interval(&self) -> Duration {
        Duration::from_secs(self.heartbeat_interval_secs.clamp(5, 3600))
    }

    /// The pause window `at` (device local time) falls in, if any.
    pub fn paused_at(&self, at: NaiveDateTime) -> Option<&PauseWindow> {
        self.pause_windows.iter().find(|w| w.contains(at))
    }
}

/// How much the agent masks before buffering (D18). Never collected at any
/// level: file contents, keystrokes, clipboard, screenshots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RedactionLevel {
    /// Credentials only (keys, tokens, passwords, private keys, URL
    /// user:password); everything else as collected.
    #[default]
    Secrets,
    /// Nothing masked (remotes are still normalized).
    None,
    /// Every SH-5 rule: titles cut and stripped of URLs and e-mails, paths
    /// and free text masked for high-entropy strings too.
    Full,
}

/// What the agent must never collect. Matching events are dropped before
/// they reach the buffer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
#[serde(default)]
pub struct Exclusions {
    /// Path patterns: a plain path excludes it and everything below it;
    /// `*` matches within one segment, `**` across segments, `?` one character.
    pub paths: Vec<String>,
    /// App names (process or bundle name), exact, `.exe` optional.
    pub apps: Vec<String>,
    /// Domains; each also excludes its subdomains.
    pub domains: Vec<String>,
}

impl Exclusions {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.apps.is_empty() && self.domains.is_empty()
    }

    /// `path` is absolute (with `~` already expanded by the caller).
    pub fn excludes_path(&self, path: &str) -> bool {
        let path = normalize_path(path);
        self.paths
            .iter()
            .any(|pattern| path_matches(&normalize_path(pattern), &path))
    }

    pub fn excludes_app(&self, app: &str) -> bool {
        let app = app_name(app);
        !app.is_empty() && self.apps.iter().any(|a| app_name(a) == app)
    }

    pub fn excludes_domain(&self, domain: &str) -> bool {
        self.domains.iter().any(|d| domain_matches(d, domain))
    }
}

/// A recurring local-time window, e.g. 12:00–13:00 on weekdays.
/// `end` before `start` wraps past midnight (22:00–06:00).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "codegen", derive(schemars::JsonSchema, ts_rs::TS))]
pub struct PauseWindow {
    #[serde(with = "hh_mm")]
    #[cfg_attr(
        feature = "codegen",
        schemars(with = "String", regex(pattern = r"^\d{2}:\d{2}(:\d{2})?$")),
        ts(type = "string")
    )]
    pub start: NaiveTime,
    #[serde(with = "hh_mm")]
    #[cfg_attr(
        feature = "codegen",
        schemars(with = "String", regex(pattern = r"^\d{2}:\d{2}(:\d{2})?$")),
        ts(type = "string")
    )]
    pub end: NaiveTime,
    /// ISO weekdays (1 = Monday … 7 = Sunday) the window *starts* on; empty
    /// or absent means every day.
    #[serde(default)]
    pub days: Vec<u8>,
}

impl PauseWindow {
    pub fn contains(&self, at: NaiveDateTime) -> bool {
        let time = at.time();
        let day = at.weekday().number_from_monday() as u8;
        let on = |d: u8| self.days.is_empty() || self.days.contains(&d);
        if self.start <= self.end {
            on(day) && self.start <= time && time < self.end
        } else {
            // Wraps midnight: the late part belongs to today's window, the
            // early part to yesterday's.
            let yesterday = if day == 1 { 7 } else { day - 1 };
            (time >= self.start && on(day)) || (time < self.end && on(yesterday))
        }
    }
}

mod hh_mm {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(time: &NaiveTime, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&time.format("%H:%M"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveTime, D::Error> {
        let text = String::deserialize(d)?;
        NaiveTime::parse_from_str(&text, "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(&text, "%H:%M:%S"))
            .map_err(serde::de::Error::custom)
    }
}

/// `domain` is `pattern` or one of its subdomains (`*.` prefix optional).
pub(crate) fn domain_matches(pattern: &str, domain: &str) -> bool {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    let d = pattern
        .trim()
        .trim_start_matches("*.")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    !d.is_empty()
        && (domain == d
            || domain
                .strip_suffix(&d)
                .is_some_and(|prefix| prefix.ends_with('.')))
}

pub(crate) fn normalize_path(path: &str) -> String {
    let mut p = path.trim().replace('\\', "/").to_lowercase();
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

pub(crate) fn app_name(app: &str) -> String {
    let name = app.trim().replace('\\', "/");
    let name = name.rsplit('/').next().unwrap_or_default().to_lowercase();
    name.strip_suffix(".exe").map(str::to_owned).unwrap_or(name)
}

pub(crate) fn path_matches(pattern: &str, path: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    if !pattern.contains(['*', '?']) {
        // Plain path: itself and everything below it.
        return path == pattern
            || path
                .strip_prefix(pattern)
                .is_some_and(|rest| rest.starts_with('/') || pattern.ends_with('/'));
    }
    let pattern: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = path.split('/').collect();
    // A glob also excludes everything below what it matches.
    (1..=path.len()).any(|n| glob_segments(&pattern, &path[..n]))
}

fn glob_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| glob_segments(rest, &path[skip..])),
        Some((first, rest)) => path
            .split_first()
            .is_some_and(|(seg, path_rest)| glob_one(first, seg) && glob_segments(rest, path_rest)),
    }
}

/// `*` and `?` within one path segment.
pub(crate) fn glob_one(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    #[test]
    fn defaults_match_blueprint_appendix_a() {
        use defaults::*;
        assert_eq!(IDLE_THRESHOLD, Duration::from_secs(300));
        assert_eq!(MERGE_GAP, Duration::from_secs(600));
        assert_eq!(MIN_SESSION, Duration::from_secs(60));
        assert_eq!(CONTEXT_WINDOW, Duration::from_secs(300));
        assert_eq!(REVIEW_CONFIDENCE, 0.7);
        assert_eq!(ENGINE_DEBOUNCE, Duration::from_secs(120));
        assert_eq!(RAW_RETENTION_MONTHS, 12);
        const { assert!(!AI_ENABLED) };

        let config = AgentConfig::default();
        assert_eq!(config.upload_interval(), Duration::from_secs(60));
        assert_eq!(config.upload_max_events, 500);
        assert_eq!(config.buffer_max_age_days, 30);
        assert_eq!(config.heartbeat_interval(), Duration::from_secs(60));
        assert_eq!(config.edit_aggregation_secs, 30);
        assert_eq!(config.redaction_version, REDACTION_VERSION);
        assert!(config.exclusions.is_empty() && config.pause_windows.is_empty());
    }

    #[test]
    fn wire_format_and_missing_fields() {
        let config: AgentConfig = serde_json::from_str(
            r#"{"exclusions":{"apps":["Slack"]},
                "pause_windows":[{"start":"12:00","end":"13:00","days":[1,2,3,4,5]}],
                "some_future_field":true}"#,
        )
        .unwrap();
        assert_eq!(config.exclusions.apps, ["Slack"]);
        assert_eq!(
            config.upload_max_events, 500,
            "missing fields take defaults"
        );
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["pause_windows"][0]["start"], "12:00");
        assert_eq!(serde_json::from_value::<AgentConfig>(json).unwrap(), config);
        assert!(serde_json::from_str::<AgentConfig>("{}").is_ok());
        // Postgres `time` renders with seconds.
        let w: PauseWindow =
            serde_json::from_str(r#"{"start":"12:00:00","end":"13:30:00"}"#).unwrap();
        assert_eq!(w.end, NaiveTime::from_hms_opt(13, 30, 0).unwrap());
    }

    #[test]
    fn intervals_are_clamped() {
        let config = AgentConfig {
            upload_interval_secs: 0,
            heartbeat_interval_secs: 1_000_000,
            ..AgentConfig::default()
        };
        assert_eq!(config.upload_interval(), Duration::from_secs(5));
        assert_eq!(config.heartbeat_interval(), Duration::from_secs(3600));
    }

    #[test]
    fn plain_paths_exclude_themselves_and_below() {
        let ex = Exclusions {
            paths: vec![
                "/home/dev/private".into(),
                "C:\\Users\\dev\\Secret\\".into(),
            ],
            ..Exclusions::default()
        };
        for yes in [
            "/home/dev/private",
            "/home/dev/private/",
            "/home/dev/private/notes/a.md",
            "/HOME/dev/Private/x",
            "c:/users/dev/secret/x.txt",
            "C:\\Users\\dev\\Secret",
        ] {
            assert!(ex.excludes_path(yes), "{yes}");
        }
        for no in [
            "/home/dev/private-notes",
            "/home/dev",
            "/home/dev/api/private",
        ] {
            assert!(!ex.excludes_path(no), "{no}");
        }
    }

    #[test]
    fn glob_paths() {
        let ex = Exclusions {
            paths: vec![
                "/home/*/clients/acme".into(),
                "**/.ssh".into(),
                "/work/**/secret-*".into(),
                "/tmp/?.log".into(),
            ],
            ..Exclusions::default()
        };
        for yes in [
            "/home/dev/clients/acme",
            "/home/dev/clients/acme/src/main.rs",
            "/home/dev/.ssh/id_ed25519",
            "/root/.ssh",
            "/work/secret-x",
            "/work/a/b/secret-plan/doc.md",
            "/tmp/a.log",
        ] {
            assert!(ex.excludes_path(yes), "{yes}");
        }
        for no in [
            "/home/dev/clients/acme2",
            "/home/a/b/clients/acme",
            "/work/a/not-secret",
            "/tmp/ab.log",
            "/home/dev/ssh",
        ] {
            assert!(!ex.excludes_path(no), "{no}");
        }
    }

    #[test]
    fn apps_and_domains() {
        let ex = Exclusions {
            apps: vec!["Slack".into(), "keepassxc.exe".into()],
            domains: vec!["bank.example".into(), "*.mail.example.".into()],
            ..Exclusions::default()
        };
        for yes in ["slack", "Slack.exe", "C:\\Apps\\KeePassXC.exe", "keepassxc"] {
            assert!(ex.excludes_app(yes), "{yes}");
        }
        assert!(!ex.excludes_app("slackware") && !ex.excludes_app(""));
        for yes in [
            "bank.example",
            "WWW.Bank.Example",
            "mail.example",
            "a.mail.example.",
        ] {
            assert!(ex.excludes_domain(yes), "{yes}");
        }
        for no in ["notbank.example", "bank.example.evil", "example"] {
            assert!(!ex.excludes_domain(no), "{no}");
        }
    }

    #[test]
    fn pause_windows() {
        let lunch = PauseWindow {
            start: NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(13, 0, 0).unwrap(),
            days: vec![1, 2, 3, 4, 5],
        };
        assert!(lunch.contains(at("2026-11-20 12:30"))); // Friday
        assert!(lunch.contains(at("2026-11-20 12:00")));
        assert!(!lunch.contains(at("2026-11-20 13:00")), "end is exclusive");
        assert!(!lunch.contains(at("2026-11-21 12:30")), "Saturday");

        let night = PauseWindow {
            start: NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
            days: vec![5], // starts Friday night
        };
        assert!(night.contains(at("2026-11-20 23:00")), "Friday late");
        assert!(
            night.contains(at("2026-11-21 05:59")),
            "Saturday early belongs to Friday"
        );
        assert!(
            !night.contains(at("2026-11-20 05:00")),
            "Friday early belongs to Thursday"
        );
        assert!(!night.contains(at("2026-11-21 23:00")));

        let config = AgentConfig {
            pause_windows: vec![lunch.clone(), night],
            ..AgentConfig::default()
        };
        assert_eq!(config.paused_at(at("2026-11-19 12:15")), Some(&lunch));
        assert_eq!(config.paused_at(at("2026-11-19 15:00")), None);
    }
}
