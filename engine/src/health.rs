//! Health endpoint for Compose and orchestrators (not a user API):
//! `GET /healthz` (process up) and `GET /readyz` (database reachable and the
//! workers ticked recently).

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::worker::Stats;

/// Workers that have not looped for this long are considered stuck.
const STUCK_SECS: u64 = 15 * 60;

pub async fn serve(
    addr: SocketAddr,
    pool: PgPool,
    stats: Arc<Stats>,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    loop {
        let (stream, _) = tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            r = listener.accept() => match r {
                Ok(s) => s,
                Err(e) => { warn!(error = %e.kind(), "health accept failed"); continue; }
            },
        };
        let (pool, stats) = (pool.clone(), stats.clone());
        tokio::spawn(async move {
            let svc = service_fn(move |req| respond(req, pool.clone(), stats.clone()));
            let _ = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), svc)
                .await;
        });
    }
}

async fn respond(
    req: Request<Incoming>,
    pool: PgPool,
    stats: Arc<Stats>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let (status, body) = match req.uri().path() {
        "/healthz" => (StatusCode::OK, "ok"),
        "/readyz" => {
            let db = sqlx::query("SELECT 1").execute(&pool).await.is_ok();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            let tick = stats.last_tick.load(Ordering::Relaxed);
            if !db {
                (StatusCode::SERVICE_UNAVAILABLE, "database unavailable")
            } else if now.saturating_sub(tick) > STUCK_SECS {
                (StatusCode::SERVICE_UNAVAILABLE, "workers stuck")
            } else {
                (StatusCode::OK, "ready")
            }
        }
        _ => (StatusCode::NOT_FOUND, "not found"),
    };
    let mut resp = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *resp.status_mut() = status;
    Ok(resp)
}
