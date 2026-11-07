//! Tracing setup.
//!
//! PRIVACY RULE: never log event metadata (titles, URLs, paths, commands),
//! request bodies, tokens, or connection strings. Log IDs, counts, status
//! codes, durations, and error kinds only.

use tracing_subscriber::EnvFilter;

use crate::config::LogFormat;

pub fn init(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder.json().flatten_event(true).init(),
    }
}
