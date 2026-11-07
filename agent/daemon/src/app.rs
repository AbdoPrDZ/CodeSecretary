//! Agent lifecycle: start components, wait for a stop request, shut down.

use std::time::Duration;

use anyhow::Result;
use tracing::{info, warn};

use crate::config::LocalConfig;
use crate::paths::AgentPaths;
use crate::shutdown;
use crate::supervisor::{ShutdownOutcome, Supervisor};

/// How long components get to flush after a stop request. Kept under the
/// ~5 s Windows allows for console-close/logoff handlers where possible;
/// components should finish well inside it.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(4);

pub async fn run(paths: AgentPaths, config: LocalConfig, run_for: Option<Duration>) -> Result<()> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "agent started"
    );
    // Directories and server URL are the user's own config, not activity metadata.
    info!(data_dir = %paths.data_dir.display(), server_url = %config.server_url, "configuration loaded");

    #[allow(unused_mut)] // components are registered here by later tasks (AG-2…AG-14)
    let mut supervisor = Supervisor::new();

    let reason = tokio::select! {
        signal = shutdown::wait_for_signal() => signal?,
        () = sleep_or_forever(run_for) => "run-for elapsed",
    };
    info!(reason, "stop requested");

    match supervisor.shutdown(SHUTDOWN_GRACE).await {
        ShutdownOutcome::Clean => info!("agent stopped"),
        ShutdownOutcome::TimedOut { aborted } => {
            warn!(aborted, "agent stopped after aborting components");
        }
    }
    Ok(())
}

async fn sleep_or_forever(duration: Option<Duration>) {
    match duration {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending().await,
    }
}
