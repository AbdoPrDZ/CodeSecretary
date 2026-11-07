use std::process::ExitCode;

use secretary_warehouse::config::Config;
use secretary_warehouse::db::{self, Db};
use secretary_warehouse::http::{self, AppState};
use secretary_warehouse::telemetry;
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("secretary-warehouse: {err}");
            return ExitCode::from(2);
        }
    };
    telemetry::init(config.log_format);

    match serve(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!(error = %format!("{err:#}"), "warehouse failed");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config: Config) -> anyhow::Result<()> {
    info!(version = env!("CARGO_PKG_VERSION"), "warehouse starting");

    if let Some(owner_url) = &config.migration_database_url {
        db::migrate(owner_url).await?;
        info!("migrations applied");
    }

    let db = Db::connect_lazy(&config.database_url, config.database_max_connections)?;
    let app = http::router(AppState { db: db.clone() });

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    info!(addr = %config.bind, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    db.close().await;
    info!("warehouse stopped");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
    info!("shutdown requested");
}
