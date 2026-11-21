//! Remote agent config (AG-7): fetch `GET /v1/agent/config`, cache it, and
//! watch the pause windows.
//!
//! - [`ConfigCache`] holds the current [`AgentConfig`] in memory and in
//!   `<data>/agent_config.json`, so exclusions apply from the first event
//!   after a restart, even offline. No file → defaults (nothing excluded).
//! - [`ConfigTask`] re-fetches every [`CONFIG_REFRESH`] with the cached
//!   `ETag` (a dashboard change applies within 5 min).
//! - [`PauseWatch`] emits `agent.pause` / `agent.resume` (`exclusion_window`)
//!   when the local time enters or leaves a pause window; the [`Pipeline`]
//!   drops everything in between.
//!
//! The cache holds the user's own exclusion patterns and survives `logout`
//! on purpose: events collected while signed out are uploaded after the next
//! sign-in, so they must still be filtered. Patterns are never logged.
//!
//! [`Pipeline`]: crate::pipeline::Pipeline

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use secretary_shared::Event;
use secretary_shared::config::{AgentConfig, Exclusions, defaults};
use secretary_shared::event::{AgentPauseChange, PauseReason, Payload};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::api::{ApiError, ConfigFetch, Warehouse};
use crate::buffer::agent_source;
use crate::pipeline::Pipeline;
use crate::status::StatusBoard;

/// Appendix-A-derived: re-fetch at least this often.
pub const CONFIG_REFRESH: Duration = defaults::CONFIG_REFRESH;
/// After a failed fetch, try again this soon.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// How often the pause watcher checks the clock.
pub const PAUSE_CHECK: Duration = Duration::from_secs(5);

/// The config in effect, with `~` in path patterns already expanded.
#[derive(Debug, Clone, PartialEq)]
pub struct Active {
    pub config: AgentConfig,
    pub exclusions: Exclusions,
}

impl Active {
    fn new(config: AgentConfig) -> Self {
        let exclusions = expand_home(&config.exclusions);
        Self { config, exclusions }
    }
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    etag: Option<String>,
    fetched_at: DateTime<Utc>,
    config: AgentConfig,
}

pub struct ConfigCache {
    path: PathBuf,
    current: RwLock<(Arc<Active>, Option<String>)>,
}

impl ConfigCache {
    /// Loads the cached config; a missing or unreadable file means defaults.
    pub fn load(path: &Path) -> Self {
        let (config, etag) = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<OnDisk>(&text) {
                Ok(disk) => (disk.config, disk.etag),
                Err(_) => {
                    warn!("cached agent config is unreadable; using defaults until the next fetch");
                    (AgentConfig::default(), None)
                }
            },
            Err(_) => (AgentConfig::default(), None),
        };
        Self {
            path: path.to_owned(),
            current: RwLock::new((Arc::new(Active::new(config)), etag)),
        }
    }

    pub fn current(&self) -> Arc<Active> {
        self.current.read().expect("config lock").0.clone()
    }

    pub fn etag(&self) -> Option<String> {
        self.current.read().expect("config lock").1.clone()
    }

    /// Applies a newly fetched config and persists it (atomically).
    pub fn replace(&self, config: AgentConfig, etag: Option<String>) -> Result<()> {
        let disk = OnDisk {
            etag: etag.clone(),
            fetched_at: Utc::now(),
            config: config.clone(),
        };
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&disk)?)
            .with_context(|| format!("could not write {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("could not replace {}", self.path.display()))?;
        *self.current.write().expect("config lock") = (Arc::new(Active::new(config)), etag);
        Ok(())
    }
}

pub struct ConfigTask {
    pub warehouse: Arc<Warehouse>,
    pub cache: Arc<ConfigCache>,
    pub every: Duration,
}

impl ConfigTask {
    pub async fn run(self, token: CancellationToken) -> anyhow::Result<()> {
        loop {
            let wait = tokio::select! {
                () = token.cancelled() => return Ok(()),
                wait = self.refresh() => wait,
            };
            tokio::select! {
                () = token.cancelled() => return Ok(()),
                () = tokio::time::sleep(wait) => {}
            }
        }
    }

    /// One fetch; returns how long to wait before the next.
    pub async fn refresh(&self) -> Duration {
        let etag = self.cache.etag();
        match self.warehouse.fetch_config(etag.as_deref()).await {
            Ok(ConfigFetch::NotModified) => {
                debug!("agent config unchanged");
                let active = self.cache.current();
                self.warehouse.status().update(|s| {
                    s.config.fetched_at = Some(Utc::now());
                    s.config.path_exclusions = active.exclusions.paths.len();
                    s.config.app_exclusions = active.exclusions.apps.len();
                    s.config.domain_exclusions = active.exclusions.domains.len();
                    s.config.pause_windows = active.config.pause_windows.len();
                });
                self.every
            }
            Ok(ConfigFetch::Changed { config, etag }) => {
                let active = Active::new(config.clone());
                let counts = (
                    active.exclusions.paths.len(),
                    active.exclusions.apps.len(),
                    active.exclusions.domains.len(),
                    config.pause_windows.len(),
                );
                self.warehouse.status().update(|s| {
                    s.config.fetched_at = Some(Utc::now());
                    s.config.path_exclusions = counts.0;
                    s.config.app_exclusions = counts.1;
                    s.config.domain_exclusions = counts.2;
                    s.config.pause_windows = counts.3;
                });
                match self.cache.replace(config, etag) {
                    Ok(()) => info!(
                        path_exclusions = counts.0,
                        app_exclusions = counts.1,
                        domain_exclusions = counts.2,
                        pause_windows = counts.3,
                        "agent config applied"
                    ),
                    Err(err) => {
                        warn!(error = %format!("{err:#}"), "could not cache the agent config")
                    }
                }
                self.every
            }
            // Keep the cached config; the uploader reports sign-in problems.
            Err(ApiError::SignedOut) => self.every.min(RETRY_AFTER),
            Err(err) => {
                warn!(error = %err, "could not fetch the agent config");
                self.every.min(RETRY_AFTER)
            }
        }
    }
}

/// Emits `agent.pause` / `agent.resume` at pause-window edges.
pub struct PauseWatch {
    pub cache: Arc<ConfigCache>,
    pub pipeline: Arc<Pipeline>,
    pub status: Arc<StatusBoard>,
    pub every: Duration,
}

impl PauseWatch {
    pub async fn run(self, token: CancellationToken) -> anyhow::Result<()> {
        let mut paused = false;
        loop {
            paused = self.check(paused, Utc::now());
            tokio::select! {
                () = token.cancelled() => return Ok(()),
                () = tokio::time::sleep(self.every) => {}
            }
        }
    }

    /// Compares the pause state at `now` with `was`; emits on change.
    pub fn check(&self, was: bool, now: DateTime<Utc>) -> bool {
        let local = now.with_timezone(&Local).naive_local();
        let paused = self.cache.current().config.paused_at(local).is_some();
        self.status.update(|s| s.config.paused = paused);
        if paused != was {
            let change = AgentPauseChange {
                reason: PauseReason::ExclusionWindow,
            };
            let payload = if paused {
                Payload::AgentPause(change)
            } else {
                Payload::AgentResume(change)
            };
            info!(
                paused,
                "pause window {}",
                if paused { "started" } else { "ended" }
            );
            self.pipeline
                .submit(Event::new(agent_source(), now, None, payload));
        }
        paused
    }
}

/// Expands a leading `~` in path patterns to the home directory.
fn expand_home(exclusions: &Exclusions) -> Exclusions {
    let home = directories::BaseDirs::new().map(|d| d.home_dir().to_string_lossy().into_owned());
    let expand = |p: &String| match (&home, p.strip_prefix('~')) {
        (Some(home), Some(rest)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            format!("{home}{rest}")
        }
        _ => p.clone(),
    };
    Exclusions {
        paths: exclusions.paths.iter().map(expand).collect(),
        ..exclusions.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_corrupt_cache_means_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent_config.json");
        let cache = ConfigCache::load(&path);
        assert_eq!(cache.current().config, AgentConfig::default());
        assert_eq!(cache.etag(), None);

        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(
            ConfigCache::load(&path).current().config,
            AgentConfig::default()
        );
    }

    #[test]
    fn replace_persists_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent_config.json");
        let mut config = AgentConfig::default();
        config.exclusions.apps.push("Slack".into());
        ConfigCache::load(&path)
            .replace(config.clone(), Some("\"v1\"".into()))
            .unwrap();

        let reopened = ConfigCache::load(&path);
        assert_eq!(reopened.current().config, config);
        assert_eq!(reopened.etag().as_deref(), Some("\"v1\""));
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn home_is_expanded_in_path_patterns() {
        let home = directories::BaseDirs::new().unwrap().home_dir().to_owned();
        let active = Active::new(AgentConfig {
            exclusions: Exclusions {
                paths: vec!["~/private".into(), "~other/x".into(), "/abs".into()],
                ..Exclusions::default()
            },
            ..AgentConfig::default()
        });
        assert!(
            active
                .exclusions
                .excludes_path(&home.join("private").join("a.txt").to_string_lossy())
        );
        assert_eq!(active.exclusions.paths[1], "~other/x");
        assert_eq!(active.exclusions.paths[2], "/abs");
    }
}
