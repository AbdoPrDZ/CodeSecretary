//! Local bootstrap configuration (`agent.toml`).
//!
//! Only what the agent needs before it can reach the server. Everything the
//! user manages from the dashboard (exclusions, project roots, pause windows)
//! arrives as remote `AgentConfig` instead (AG-7).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_SERVER_URL: &str = "http://localhost:8080";
pub const DEFAULT_LOG_LEVEL: &str = "info";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalConfig {
    /// Base URL of the warehouse device API.
    pub server_url: String,
    /// `tracing` filter directive, e.g. `info` or `secretary_agent=debug`.
    /// Overridden by the `SECRETARY_LOG` environment variable.
    pub log_level: String,
    /// Folders the Git tracker scans for repositories (`~` allowed), added to
    /// the ones from the dashboard. Both empty: the home folder (AG-10).
    pub project_roots: Vec<String>,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_SERVER_URL.to_owned(),
            log_level: DEFAULT_LOG_LEVEL.to_owned(),
            project_roots: Vec::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read config file {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    #[error("invalid config file {path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

impl LocalConfig {
    /// Loads the config; a missing file means defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(ConfigError::Io {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let config: Self = toml::from_str(&text).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
        config.validate().map_err(|message| ConfigError::Invalid {
            path: path.to_owned(),
            message,
        })?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        let url = self.server_url.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("server_url must start with https:// or http://".to_owned());
        }
        if self.log_level.trim().is_empty() {
            return Err("log_level must not be empty".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join("agent.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn missing_file_gives_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let config = LocalConfig::load(&tmp.path().join("agent.toml")).unwrap();
        assert_eq!(config, LocalConfig::default());
    }

    #[test]
    fn partial_file_fills_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(tmp.path(), r#"server_url = "https://example.test""#);
        let config = LocalConfig::load(&path).unwrap();
        assert_eq!(config.server_url, "https://example.test");
        assert_eq!(config.log_level, DEFAULT_LOG_LEVEL);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(tmp.path(), "sever_url = \"https://typo.test\"\n");
        assert!(matches!(
            LocalConfig::load(&path),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn non_http_server_url_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(tmp.path(), r#"server_url = "ftp://example.test""#);
        assert!(matches!(
            LocalConfig::load(&path),
            Err(ConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn malformed_toml_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(tmp.path(), "server_url = ");
        assert!(matches!(
            LocalConfig::load(&path),
            Err(ConfigError::Parse { .. })
        ));
    }
}
