//! `secretary-engine`: run the workers until SIGTERM / Ctrl-C.
//!
//! Exit codes: 0 ok, 1 failure, 2 configuration error.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use secretary_engine::config::Config;
use secretary_engine::worker::{Calculator, Stats, run_reaper, run_worker};
use secretary_engine::{ALGORITHM_VERSION, health, queue};
use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Time the current jobs get to finish after a stop signal.
const GRACE: Duration = Duration::from_secs(20);

fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("secretary-engine: {e}");
            return ExitCode::from(2);
        }
    };
    init_tracing(&config);
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!(error = %e, "runtime failed");
            return ExitCode::from(1);
        }
    };
    match rt.block_on(run(config)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(error = %e, "engine failed");
            ExitCode::from(1)
        }
    }
}

fn init_tracing(config: &Config) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.log_level));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if config.log_json {
        builder.json().init();
    } else {
        builder.init();
    }
}

async fn run(config: Config) -> anyhow::Result<()> {
    info!(
        version = env!("CARGO_PKG_VERSION"),
        algorithm = ALGORITHM_VERSION,
        workers = config.workers,
        "engine starting"
    );
    let pool = connect(&config).await?;

    match queue::enqueue_uncalculated(&pool, config.recalc_days, ALGORITHM_VERSION).await {
        Ok(n) => info!(
            queued = n,
            days = config.recalc_days,
            "queued days without current results"
        ),
        Err(e) => warn!(error = %queue::error_kind(&e), "startup recalculation not queued"),
    }

    let cancel = CancellationToken::new();
    let stats = Arc::new(Stats::default());
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    let pid = std::process::id();
    let calculator = Arc::new(Calculator);
    let mut tasks = tokio::task::JoinSet::new();
    for n in 0..config.workers {
        tasks.spawn(run_worker(
            pool.clone(),
            calculator.clone(),
            format!("{host}:{pid}:{n}"),
            config.poll,
            stats.clone(),
            cancel.clone(),
        ));
    }
    tasks.spawn(run_reaper(
        pool.clone(),
        Duration::from_secs(60),
        cancel.clone(),
    ));
    {
        let (pool, stats, cancel, addr) = (
            pool.clone(),
            stats.clone(),
            cancel.clone(),
            config.health_bind,
        );
        tasks.spawn(async move {
            if let Err(e) = health::serve(addr, pool, stats, cancel).await {
                error!(error = %e, "health endpoint failed");
            }
        });
    }
    info!(health = %config.health_bind, "engine ready");

    shutdown_signal().await;
    info!("stopping");
    cancel.cancel();
    if tokio::time::timeout(GRACE, async { while tasks.join_next().await.is_some() {} })
        .await
        .is_err()
    {
        warn!("jobs still running after the grace period; they will be reclaimed");
        tasks.abort_all();
    }
    pool.close().await;
    info!("stopped");
    Ok(())
}

/// The database may still be starting (Compose): retry for up to a minute.
async fn connect(config: &Config) -> anyhow::Result<sqlx::PgPool> {
    let opts = PgPoolOptions::new()
        .max_connections(config.workers as u32 + 2)
        .acquire_timeout(Duration::from_secs(10));
    let mut last = None;
    for _ in 0..30 {
        match opts.clone().connect(&config.database_url).await {
            Ok(pool) => return Ok(pool),
            Err(e) => {
                warn!(error = %queue::error_kind(&e), "database not reachable yet");
                last = Some(e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
    Err(anyhow::anyhow!(
        "database unreachable: {}",
        last.map(|e| queue::error_kind(&e)).unwrap_or_default()
    ))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
