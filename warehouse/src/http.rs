//! HTTP routes. Device API endpoints (`/v1/...`) are added by WH-4…WH-10.

use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .with_state(state)
}

/// Liveness: the process is up and serving.
async fn healthz() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: the database is reachable.
async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match tokio::time::timeout(Duration::from_secs(2), state.db.ping()).await {
        Ok(Ok(())) => (StatusCode::OK, Json(json!({ "status": "ready" }))),
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "readiness check failed");
            unavailable()
        }
        Err(_) => {
            tracing::warn!("readiness check timed out");
            unavailable()
        }
    }
}

fn unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "status": "unavailable", "reason": "database" })),
    )
}
