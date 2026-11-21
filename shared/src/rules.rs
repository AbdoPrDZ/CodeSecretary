//! Project mapping rules (D17): a user-made rule binds evidence to a project.
//!
//! One rule is a signal and a pattern:
//!
//! | Signal | Matches | Pattern |
//! |---|---|---|
//! | `path` | repository root, working directory, workspace and file paths | a plain folder matches it and everything below it; `*` within a segment, `**` across segments, `?` one character |
//! | `remote` | normalized repository remotes (`github.com/acme/api`) | same as `path`; a pasted clone URL is normalized |
//! | `domain` | website domains | the domain and its subdomains; a pasted URL is cut to its host |
//! | `app` | the focused app | the app name, `.exe` optional |
//! | `title` | window titles | a substring, or a whole-title glob when it has `*` or `?` |
//!
//! Matching is case-insensitive. The engine resolves an event with
//! [`resolve`]; the dashboard previews a rule with the same semantics (its
//! Python copy is checked against `shared/fixtures/rule-match-v1.json`).

use serde::{Deserialize, Serialize};

use crate::config::{app_name, domain_matches, glob_one, normalize_path, path_matches};
use crate::event::{Event, Payload};

/// Longest stored pattern.
pub const MAX_PATTERN: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Path,
    Remote,
    Domain,
    App,
    Title,
}

impl Signal {
    pub const ALL: [Signal; 5] = [
        Signal::Path,
        Signal::Remote,
        Signal::Domain,
        Signal::App,
        Signal::Title,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: i64,
    pub project_id: i64,
    pub signal: Signal,
    /// As stored (already through [`normalize_pattern`]).
    pub pattern: String,
    #[serde(default)]
    pub priority: i32,
}

/// The pattern as stored, or `None` when it cannot match anything.
pub fn normalize_pattern(signal: Signal, pattern: &str) -> Option<String> {
    let p = pattern.trim();
    if p.is_empty() || p.chars().count() > MAX_PATTERN {
        return None;
    }
    let out = match signal {
        Signal::Path => {
            let p = p.replace('\\', "/");
            let trimmed = p.trim_end_matches('/');
            if trimmed.is_empty() {
                "/".to_owned()
            } else {
                trimmed.to_owned()
            }
        }
        Signal::Remote => normalize_remote_pattern(p)?,
        Signal::Domain => {
            let host = p.split_once("://").map_or(p, |(_, rest)| rest);
            let host = host.split(['/', '?', '#']).next().unwrap_or_default();
            let host = host.rsplit('@').next().unwrap_or_default();
            let host = host.split(':').next().unwrap_or_default();
            host.trim_start_matches("*.")
                .trim_end_matches('.')
                .to_ascii_lowercase()
        }
        Signal::App | Signal::Title => p.to_owned(),
    };
    (!out.is_empty()).then_some(out)
}

/// `scheme://[user@]host[:port]/path`, `[user@]host:path` or `host/path`
/// to `host/path`, lowercased, without `.git` and slashes at the ends.
fn normalize_remote_pattern(p: &str) -> Option<String> {
    let joined = if let Some((_, rest)) = p.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority.rsplit('@').next().unwrap_or_default();
        let host = host.split(':').next().unwrap_or_default();
        format!("{host}/{path}")
    } else {
        match p.split_once(':') {
            Some((user_host, path)) if !user_host.contains('/') => {
                let host = user_host.rsplit('@').next().unwrap_or_default();
                format!("{host}/{path}")
            }
            _ => p.to_owned(),
        }
    };
    let joined = joined.to_lowercase();
    let trimmed = joined.trim_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let trimmed = trimmed.trim_end_matches('/');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// One stored pattern against one evidence value of its signal.
pub fn matches_value(signal: Signal, pattern: &str, value: &str) -> bool {
    match signal {
        Signal::Path | Signal::Remote => {
            path_matches(&normalize_path(pattern), &normalize_path(value))
        }
        Signal::Domain => domain_matches(pattern, value),
        Signal::App => {
            let app = app_name(value);
            !app.is_empty() && app == app_name(pattern)
        }
        Signal::Title => {
            let (pattern, title) = (pattern.to_lowercase(), value.to_lowercase());
            if pattern.contains(['*', '?']) {
                glob_one(&pattern, &title)
            } else {
                title.contains(&pattern)
            }
        }
    }
}

/// The values of one event that rules look at, per signal.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    pub paths: Vec<String>,
    pub remotes: Vec<String>,
    pub domains: Vec<String>,
    pub apps: Vec<String>,
    pub titles: Vec<String>,
}

impl Evidence {
    pub fn from_event(event: &Event) -> Self {
        let mut ev = Evidence::default();
        let root = event.repository.as_ref().and_then(|r| r.root_path.clone());
        if let Some(repo) = &event.repository {
            ev.paths.extend(repo.root_path.clone());
            ev.remotes.extend(repo.remote_url.clone());
        }
        ev.paths.extend(event.working_directory.clone());
        // Editor file paths are relative to the repository root.
        let file = |path: &str| match &root {
            Some(root) => format!("{}/{}", root.trim_end_matches(['/', '\\']), path),
            None => path.to_owned(),
        };
        match &event.payload {
            Payload::WindowFocus(m) => {
                ev.apps.extend(m.app.clone());
                ev.titles.extend(m.title.clone());
            }
            Payload::EditorWorkspaceOpen(m) => {
                ev.paths.push(m.workspace_path.clone());
                ev.remotes.extend(m.remote_url.clone());
            }
            Payload::EditorFileFocus(m) => ev.paths.push(file(&m.file_path)),
            Payload::EditorEditBurst(m) => ev.paths.push(file(&m.file_path)),
            Payload::EditorSave(m) => ev.paths.push(file(&m.file_path)),
            Payload::GitCommit(m) => ev.remotes.extend(m.remote_url.clone()),
            _ => {}
        }
        ev
    }

    pub fn values(&self, signal: Signal) -> &[String] {
        match signal {
            Signal::Path => &self.paths,
            Signal::Remote => &self.remotes,
            Signal::Domain => &self.domains,
            Signal::App => &self.apps,
            Signal::Title => &self.titles,
        }
    }

    pub fn matches(&self, rule: &Rule) -> bool {
        self.values(rule.signal)
            .iter()
            .any(|v| matches_value(rule.signal, &rule.pattern, v))
    }
}

/// The rule that assigns this evidence: the highest priority, then the most
/// specific (longest) pattern, then the oldest rule.
pub fn resolve<'a>(rules: &'a [Rule], evidence: &Evidence) -> Option<&'a Rule> {
    rules.iter().filter(|r| evidence.matches(r)).min_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(b.pattern.chars().count().cmp(&a.pattern.chars().count()))
            .then(a.id.cmp(&b.id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{RepositoryRef, Source, WindowFocus};
    use chrono::Utc;

    pub const FIXTURE: &str = include_str!("../fixtures/rule-match-v1.json");

    fn signal(name: &str) -> Signal {
        serde_json::from_value(serde_json::Value::String(name.into())).unwrap()
    }

    #[test]
    fn fixture_cases() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        for case in fixture["normalize"].as_array().unwrap() {
            let s = signal(case["signal"].as_str().unwrap());
            let got = normalize_pattern(s, case["pattern"].as_str().unwrap());
            assert_eq!(got.as_deref(), case["expected"].as_str(), "{case}");
        }
        for case in fixture["match"].as_array().unwrap() {
            let s = signal(case["signal"].as_str().unwrap());
            let pattern = normalize_pattern(s, case["pattern"].as_str().unwrap()).unwrap();
            let got = matches_value(s, &pattern, case["value"].as_str().unwrap());
            assert_eq!(got, case["matches"].as_bool().unwrap(), "{case}");
        }
    }

    fn rule(id: i64, project_id: i64, signal: Signal, pattern: &str, priority: i32) -> Rule {
        Rule {
            id,
            project_id,
            signal,
            pattern: normalize_pattern(signal, pattern).unwrap(),
            priority,
        }
    }

    #[test]
    fn evidence_and_resolution() {
        let mut event = Event::new(
            Source::new("agent", "0.1.0"),
            Utc::now(),
            Some(Utc::now()),
            Payload::WindowFocus(WindowFocus {
                app: Some("Code.exe".into()),
                title: Some("main.rs - api - Visual Studio Code".into()),
            }),
        );
        event.repository = Some(RepositoryRef {
            root_path: Some("C:\\work\\acme\\api".into()),
            remote_url: Some("github.com/acme/api".into()),
        });
        let ev = Evidence::from_event(&event);
        let rules = [
            rule(1, 10, Signal::App, "code", 0),
            rule(2, 20, Signal::Remote, "github.com/acme", 0),
            rule(3, 30, Signal::Path, "c:/work/acme/api/**", 0),
            rule(4, 40, Signal::Title, "nothing here", 9),
        ];
        // Equal priority: the longest pattern wins.
        assert_eq!(resolve(&rules, &ev).map(|r| r.id), Some(3));
        let mut boosted = rules.clone();
        boosted[0].priority = 5;
        assert_eq!(resolve(&boosted, &ev).map(|r| r.id), Some(1));
        assert_eq!(resolve(&rules[3..], &ev), None);
    }
}
