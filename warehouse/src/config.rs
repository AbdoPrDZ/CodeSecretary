//! Configuration from environment variables.
//!
//! | Variable | Default | Meaning |
//! |---|---|---|
//! | `DATABASE_URL` | — (required) | App connection; a login role in `secretary_app`, never the table owner |
//! | `DATABASE_MIGRATION_URL` | unset | If set, run migrations at startup with this (owner) connection |
//! | `WAREHOUSE_BIND` | `0.0.0.0:8080` | Listen address |
//! | `DATABASE_MAX_CONNECTIONS` | `10` | Pool size |
//! | `LOG_FORMAT` | `text` | `text` or `json` |
//! | `RUST_LOG` | `info` | `tracing` filter |

use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub migration_database_url: Option<String>,
    pub database_max_connections: u32,
    pub log_format: LogFormat,
}

// Manual Debug: never print connection strings (they contain passwords).
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .field("database_url", &"<redacted>")
            .field(
                "migration_database_url",
                &self.migration_database_url.as_ref().map(|_| "<redacted>"),
            )
            .field("database_max_connections", &self.database_max_connections)
            .field("log_format", &self.log_format)
            .finish()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{0} is required")]
    Missing(&'static str),
    #[error("{name} is invalid: {reason}")]
    Invalid { name: &'static str, reason: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds the config from any key → value lookup (used by tests).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let get = |name: &str| get(name).filter(|v| !v.trim().is_empty());

        let bind = match get("WAREHOUSE_BIND") {
            Some(v) => v
                .parse()
                .map_err(|e: std::net::AddrParseError| ConfigError::Invalid {
                    name: "WAREHOUSE_BIND",
                    reason: e.to_string(),
                })?,
            None => SocketAddr::from(([0, 0, 0, 0], 8080)),
        };
        let database_url = get("DATABASE_URL").ok_or(ConfigError::Missing("DATABASE_URL"))?;
        let database_max_connections = match get("DATABASE_MAX_CONNECTIONS") {
            Some(v) => match v.parse::<u32>() {
                Ok(n) if n > 0 => n,
                _ => {
                    return Err(ConfigError::Invalid {
                        name: "DATABASE_MAX_CONNECTIONS",
                        reason: "must be a positive integer".to_owned(),
                    });
                }
            },
            None => 10,
        };
        let log_format = match get("LOG_FORMAT").as_deref() {
            None | Some("text") => LogFormat::Text,
            Some("json") => LogFormat::Json,
            Some(other) => {
                return Err(ConfigError::Invalid {
                    name: "LOG_FORMAT",
                    reason: format!("expected text or json, got {other:?}"),
                });
            }
        };

        Ok(Self {
            bind,
            database_url,
            migration_database_url: get("DATABASE_MIGRATION_URL"),
            database_max_connections,
            log_format,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn defaults_with_only_database_url() {
        let c = Config::from_lookup(lookup(&[("DATABASE_URL", "postgres://x")])).unwrap();
        assert_eq!(c.bind, SocketAddr::from(([0, 0, 0, 0], 8080)));
        assert_eq!(c.database_max_connections, 10);
        assert_eq!(c.log_format, LogFormat::Text);
        assert_eq!(c.migration_database_url, None);
    }

    #[test]
    fn database_url_is_required() {
        assert_eq!(
            Config::from_lookup(lookup(&[])),
            Err(ConfigError::Missing("DATABASE_URL"))
        );
        assert_eq!(
            Config::from_lookup(lookup(&[("DATABASE_URL", "  ")])),
            Err(ConfigError::Missing("DATABASE_URL"))
        );
    }

    #[test]
    fn invalid_values_are_rejected() {
        for (name, value) in [
            ("WAREHOUSE_BIND", "nope"),
            ("DATABASE_MAX_CONNECTIONS", "0"),
            ("LOG_FORMAT", "xml"),
        ] {
            let result =
                Config::from_lookup(lookup(&[("DATABASE_URL", "postgres://x"), (name, value)]));
            assert!(
                matches!(result, Err(ConfigError::Invalid { name: n, .. }) if n == name),
                "{name}={value}"
            );
        }
    }

    #[test]
    fn debug_never_prints_connection_strings() {
        let c = Config::from_lookup(lookup(&[
            ("DATABASE_URL", "postgres://u:secret@h/db"),
            ("DATABASE_MIGRATION_URL", "postgres://o:secret2@h/db"),
        ]))
        .unwrap();
        let printed = format!("{c:?}");
        assert!(!printed.contains("secret"), "{printed}");
    }
}
