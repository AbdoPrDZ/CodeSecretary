//! Process settings from the environment.
//!
//! | Variable | Default | |
//! |---|---|---|
//! | `DATABASE_URL` | required | An RLS-bound login (member of `secretary_app`) |
//! | `ENGINE_WORKERS` | 2 | Concurrent jobs |
//! | `ENGINE_POLL_MS` | 1000 | Idle wait between claims |
//! | `ENGINE_RECALC_DAYS` | 30 | Startup: days checked for missing/old results |
//! | `ENGINE_HEALTH_BIND` | `0.0.0.0:8070` | `/healthz`, `/readyz` |
//! | `LOG_FORMAT` | `text` | `text` or `json` |
//! | `LOG_LEVEL` | `info` | or `RUST_LOG` |

use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub workers: usize,
    pub poll: Duration,
    pub recalc_days: i32,
    pub health_bind: SocketAddr,
    pub log_json: bool,
    pub log_level: String,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(String);

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let num = |key: &str, default: u64, min: u64, max: u64| -> Result<u64, ConfigError> {
            match get(key) {
                None => Ok(default),
                Some(v) => v
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|n| (min..=max).contains(n))
                    .ok_or_else(|| ConfigError(format!("{key} must be {min}..={max}"))),
            }
        };
        let database_url = get("DATABASE_URL")
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| ConfigError("DATABASE_URL is required".into()))?;
        let health_bind = get("ENGINE_HEALTH_BIND")
            .unwrap_or_else(|| "0.0.0.0:8070".into())
            .parse()
            .map_err(|_| ConfigError("ENGINE_HEALTH_BIND must be host:port".into()))?;
        let log_json = match get("LOG_FORMAT").as_deref().unwrap_or("text") {
            "json" => true,
            "text" => false,
            _ => return Err(ConfigError("LOG_FORMAT must be text or json".into())),
        };
        Ok(Self {
            database_url,
            workers: num("ENGINE_WORKERS", 2, 1, 64)? as usize,
            poll: Duration::from_millis(num("ENGINE_POLL_MS", 1000, 50, 60_000)?),
            recalc_days: num("ENGINE_RECALC_DAYS", 30, 0, 366)? as i32,
            health_bind,
            log_json,
            log_level: get("LOG_LEVEL").unwrap_or_else(|| "info".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation() {
        let c =
            Config::from_lookup(|k| (k == "DATABASE_URL").then(|| "postgres://x".into())).unwrap();
        assert_eq!((c.workers, c.recalc_days, c.log_json), (2, 30, false));
        assert!(Config::from_lookup(|_| None).is_err());
        assert!(
            Config::from_lookup(|k| match k {
                "DATABASE_URL" => Some("postgres://x".into()),
                "ENGINE_WORKERS" => Some("0".into()),
                _ => None,
            })
            .is_err()
        );
    }
}
