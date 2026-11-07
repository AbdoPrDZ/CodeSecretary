//! HTTP endpoint tests that need no database (WH-1).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use secretary_warehouse::db::Db;
use secretary_warehouse::http::{AppState, router};
use tower::ServiceExt;

/// A lazy pool pointing at a closed port: the app boots, the DB is "down".
fn app_with_unreachable_db() -> axum::Router {
    let db = Db::connect_lazy("postgres://nobody:nothing@127.0.0.1:1/none", 1).unwrap();
    router(AppState { db })
}

async fn get(app: axum::Router, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn healthz_is_ok_even_when_database_is_down() {
    let (status, body) = get(app_with_unreachable_db(), "/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn readyz_reports_unavailable_when_database_is_down() {
    let (status, body) = get(app_with_unreachable_db(), "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["reason"], "database");
}

#[tokio::test]
async fn unknown_routes_are_404() {
    let response = app_with_unreachable_db()
        .oneshot(Request::get("/v1/nope").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
