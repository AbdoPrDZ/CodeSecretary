//! Agent lifecycle: start components, wait for a stop request, shut down.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tracing::{info, warn};

use crate::api::{self, Warehouse};
use crate::auth::Link;
use crate::auth::store::KeyringStore;
use crate::buffer::Buffer;
use crate::config::LocalConfig;
use crate::heartbeat::{ClockOffset, HEARTBEAT_EVERY, HeartbeatTask};
use crate::local_api::LocalApi;
use crate::paths::AgentPaths;
use crate::pipeline::Pipeline;
use crate::remote_config::{CONFIG_REFRESH, ConfigCache, ConfigTask, PAUSE_CHECK, PauseWatch};
use crate::status::{StatusBoard, StatusWriter, WRITE_EVERY};
use crate::supervisor::{ShutdownOutcome, Supervisor};
use crate::uploader::{UPLOAD_EVERY, Uploader};
use crate::{git, presence, shutdown};

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

    let buffer = Arc::new(Buffer::open(&paths.buffer_file())?);
    info!(queued = buffer.len()?, "event buffer opened");
    let http = api::http_client()?;
    let link = Arc::new(Link::new(
        paths.state_file(),
        Arc::new(KeyringStore),
        http.clone(),
    ));
    let status = Arc::new(StatusBoard::default());
    let warehouse = Arc::new(Warehouse::with_status(http, link, status.clone()));
    let clock = Arc::new(ClockOffset::default());
    let config_cache = Arc::new(ConfigCache::load(&paths.agent_config_file()));
    let pipeline = Arc::new(Pipeline::new(buffer.clone(), config_cache.clone()));

    let mut supervisor = Supervisor::new();
    supervisor.spawn("status", {
        let writer = StatusWriter {
            board: status.clone(),
            buffer: buffer.clone(),
            state_file: paths.state_file(),
            out: paths.status_file(),
            every: WRITE_EVERY,
        };
        move |token| writer.run(token)
    });
    supervisor.spawn("config", {
        let task = ConfigTask {
            warehouse: warehouse.clone(),
            cache: config_cache.clone(),
            every: CONFIG_REFRESH,
        };
        move |token| task.run(token)
    });
    supervisor.spawn("pause", {
        let watch = PauseWatch {
            cache: config_cache.clone(),
            pipeline: pipeline.clone(),
            status: status.clone(),
            every: PAUSE_CHECK,
        };
        move |token| watch.run(token)
    });
    supervisor.spawn("git", {
        let pipeline = pipeline.clone();
        let status = status.clone();
        let state_file = paths.git_state_file();
        let local_roots = config.project_roots.clone();
        move |token| {
            // Local roots plus the dashboard's, read again at every rescan.
            let roots = move || {
                let mut all = local_roots.clone();
                all.extend(config_cache.current().config.project_roots.iter().cloned());
                git::effective_roots(&all)
            };
            git::run(pipeline, status, state_file, roots, token)
        }
    });
    supervisor.spawn("local_api", {
        let api = LocalApi {
            pipeline: pipeline.clone(),
            status: status.clone(),
            file: paths.local_api_file(),
        };
        move |token| api.run(token)
    });
    supervisor.spawn("presence", move |token| {
        presence::run(pipeline, status, token)
    });
    supervisor.spawn("uploader", {
        let uploader = Uploader {
            buffer: buffer.clone(),
            warehouse: warehouse.clone(),
            clock: clock.clone(),
            every: UPLOAD_EVERY,
        };
        move |token| uploader.run(token)
    });
    supervisor.spawn("heartbeat", {
        let task = HeartbeatTask {
            warehouse,
            buffer,
            clock,
            every: HEARTBEAT_EVERY,
        };
        move |token| task.run(token)
    });

    let stop_file = paths.stop_file();
    let _ = std::fs::remove_file(&stop_file); // a request from before this start is stale
    let reason = tokio::select! {
        signal = shutdown::wait_for_signal() => signal?,
        () = sleep_or_forever(run_for) => "run-for elapsed",
        () = stop_requested(&stop_file) => "stop requested by `secretary-agent stop`",
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

/// Resolves when the stop-request file appears (and removes it). It works the
/// same on every OS, also for an agent started without a console.
async fn stop_requested(path: &std::path::Path) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if path.exists() {
            let _ = std::fs::remove_file(path);
            return;
        }
    }
}

async fn sleep_or_forever(duration: Option<Duration>) {
    match duration {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending().await,
    }
}
