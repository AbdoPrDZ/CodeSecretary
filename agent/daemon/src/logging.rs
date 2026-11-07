//! Logging to stderr and to daily-rotated files in the log directory.
//!
//! PRIVACY RULE: never log event metadata — window titles, URLs, paths,
//! commands, repository names. Log IDs, counts, durations and error kinds only.

use std::path::Path;

use anyhow::{Context, Result};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

/// Overrides the configured log level, e.g. `SECRETARY_LOG=debug`.
pub const LOG_ENV: &str = "SECRETARY_LOG";

/// Log files kept (one per day).
const MAX_LOG_FILES: usize = 14;

/// Installs the global subscriber. Keep the returned guard alive until exit,
/// otherwise buffered lines are lost.
pub fn init(log_dir: &Path, level: &str) -> Result<WorkerGuard> {
    let filter = match std::env::var(LOG_ENV) {
        Ok(directive) if !directive.trim().is_empty() => {
            EnvFilter::try_new(&directive).with_context(|| format!("invalid {LOG_ENV} value"))?
        }
        _ => EnvFilter::try_new(level).context("invalid log_level in agent.toml")?,
    };

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("agent")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .with_context(|| format!("could not open log directory {}", log_dir.display()))?;
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_ansi(false).with_writer(file_writer))
        .try_init()
        .context("logging was already initialized")?;

    Ok(guard)
}
