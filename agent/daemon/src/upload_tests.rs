//! Upload path against a mock warehouse + Keycloak token endpoint (AG-2/4/5):
//! token refresh and rotation, retry on 401, sign-out on revocation,
//! re-registration, acknowledgement, and clock offset. No real keychain.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use flate2::read::GzDecoder;
use secretary_shared::Event;
use secretary_shared::api::BatchRequest;
use secretary_shared::event::{Empty, Payload};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

use crate::api::Warehouse;
use crate::auth::Link;
use crate::auth::store::{MemoryStore, TokenStore, refresh_token_key};
use crate::buffer::{Buffer, agent_source};
use crate::heartbeat::{ClockOffset, HeartbeatTask};
use crate::state::{Account, AgentState};
use crate::uploader::{Drained, Uploader};

const INSTALLATION: &str = "test-installation";

struct Fixture {
    _home: tempfile::TempDir,
    server: MockServer,
    store: Arc<MemoryStore>,
    state_file: std::path::PathBuf,
    buffer: Arc<Buffer>,
    clock: Arc<ClockOffset>,
    warehouse: Arc<Warehouse>,
}

impl Fixture {
    async fn new(signed_in: bool) -> Self {
        let home = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let state_file = home.path().join("state.json");
        let store = Arc::new(MemoryStore::default());
        if signed_in {
            let state = AgentState {
                installation_id: Some(INSTALLATION.into()),
                account: Some(Account {
                    server_url: server.uri(),
                    issuer: format!("{}/realms/secretary", server.uri()),
                    client_id: "secretary-agent".into(),
                    token_endpoint: format!("{}/token", server.uri()),
                    revocation_endpoint: None,
                    device_id: Some(1),
                    email: Some("dev@example.test".into()),
                }),
            };
            state.save(&state_file).unwrap();
            store
                .save(&refresh_token_key(INSTALLATION), "refresh-1")
                .unwrap();
        }
        let buffer = Arc::new(Buffer::open(&home.path().join("buffer.sqlite3")).unwrap());
        let http = reqwest::Client::new();
        let link = Arc::new(Link::new(state_file.clone(), store.clone(), http.clone()));
        Self {
            warehouse: Arc::new(Warehouse::new(http, link)),
            _home: home,
            server,
            store,
            state_file,
            buffer,
            clock: Arc::new(ClockOffset::default()),
        }
    }

    fn uploader(&self) -> Uploader {
        Uploader {
            buffer: self.buffer.clone(),
            warehouse: self.warehouse.clone(),
            clock: self.clock.clone(),
            every: Duration::from_secs(60),
        }
    }

    fn fill(&self, n: usize) {
        for i in 0..n {
            let start = Utc::now() - chrono::Duration::minutes(30 - i as i64);
            let event = Event::new(
                agent_source(),
                start,
                Some(start + chrono::Duration::seconds(30)),
                Payload::PresenceActive(Empty {}),
            );
            self.buffer.push(&event).unwrap();
        }
    }

    fn refresh_token(&self) -> Option<String> {
        self.store.load(&refresh_token_key(INSTALLATION)).unwrap()
    }

    async fn token_ok(&self, access: &str, rotated: &str) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": access, "expires_in": 300, "refresh_token": rotated,
                "token_type": "Bearer",
            })))
            .mount(&self.server)
            .await;
    }
}

/// Accepts every event in a gzip batch, like the warehouse does.
struct AcceptAll;

impl Respond for AcceptAll {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        assert_eq!(
            request
                .headers
                .get("content-encoding")
                .map(|v| v.to_str().unwrap()),
            Some("gzip")
        );
        let mut text = String::new();
        GzDecoder::new(&request.body[..])
            .read_to_string(&mut text)
            .unwrap();
        let batch: BatchRequest = serde_json::from_str(&text).unwrap();
        let ids: Vec<String> = batch
            .events
            .iter()
            .map(|e| e.event_id.to_string())
            .collect();
        ResponseTemplate::new(200).set_body_json(json!({
            "accepted": ids, "duplicates": [], "rejected": [], "server_time": Utc::now(),
        }))
    }
}

#[tokio::test]
async fn uploads_acks_and_persists_the_rotated_refresh_token() {
    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(AcceptAll)
        .expect(2) // 700 events → batches of 500 + 200
        .mount(&f.server)
        .await;

    f.fill(700);
    assert_eq!(f.uploader().drain().await, Drained::Done { sent: 700 });
    assert_eq!(f.buffer.len().unwrap(), 0);
    assert_eq!(
        f.refresh_token().as_deref(),
        Some("refresh-2"),
        "rotation persisted"
    );
}

#[tokio::test]
async fn a_401_refreshes_once_and_retries() {
    let f = Fixture::new(true).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("refresh_token=refresh-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "stale", "expires_in": 300, "refresh_token": "refresh-2",
        })))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("refresh_token=refresh-2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "fresh", "expires_in": 300, "refresh_token": "refresh-3",
        })))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .and(header("authorization", "Bearer stale"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "invalid_token"})))
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .and(header("authorization", "Bearer fresh"))
        .respond_with(AcceptAll)
        .mount(&f.server)
        .await;

    f.fill(3);
    assert_eq!(f.uploader().drain().await, Drained::Done { sent: 3 });
    assert_eq!(f.refresh_token().as_deref(), Some("refresh-3"));
}

#[tokio::test]
async fn revoked_session_signs_out_and_keeps_events() {
    let f = Fixture::new(true).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant", "error_description": "Session not active",
        })))
        .mount(&f.server)
        .await;

    f.fill(4);
    assert_eq!(f.uploader().drain().await, Drained::SignedOut);
    assert_eq!(
        f.buffer.len().unwrap(),
        4,
        "events wait for the next sign-in"
    );
    assert_eq!(
        f.refresh_token(),
        None,
        "dead token removed from the keychain"
    );
    assert!(AgentState::load(&f.state_file).unwrap().account.is_none());
    // Signed out: the next attempt makes no request at all.
    assert_eq!(f.uploader().drain().await, Drained::SignedOut);
}

#[tokio::test]
async fn revoked_device_signs_out() {
    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "device_revoked"})))
        .expect(2) // original + one retry after refresh
        .mount(&f.server)
        .await;

    f.fill(2);
    assert_eq!(f.uploader().drain().await, Drained::SignedOut);
    assert_eq!(f.buffer.len().unwrap(), 2);
    assert_eq!(f.refresh_token(), None);
}

#[tokio::test]
async fn unregistered_device_registers_then_retries() {
    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .respond_with(
            ResponseTemplate::new(403).set_body_json(json!({"error": "device_not_registered"})),
        )
        .up_to_n_times(1)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .respond_with(AcceptAll)
        .mount(&f.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/devices"))
        .and(body_string_contains("\"agent_version\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"device_id": 9})))
        .expect(1)
        .mount(&f.server)
        .await;

    f.fill(1);
    assert_eq!(f.uploader().drain().await, Drained::Done { sent: 1 });
    let state = AgentState::load(&f.state_file).unwrap();
    assert_eq!(state.account.unwrap().device_id, Some(9));
}

#[tokio::test]
async fn server_errors_keep_events_for_retry() {
    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    Mock::given(method("POST"))
        .and(path("/v1/events:batch"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&f.server)
        .await;

    f.fill(5);
    assert_eq!(f.uploader().drain().await, Drained::Failed);
    assert_eq!(f.buffer.len().unwrap(), 5);
    assert!(
        f.refresh_token().is_some(),
        "a server outage is not a sign-out"
    );
}

#[tokio::test]
async fn never_signed_in_makes_no_requests() {
    let f = Fixture::new(false).await;
    f.fill(2);
    assert_eq!(f.uploader().drain().await, Drained::SignedOut);
    assert!(f.server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn heartbeat_reports_depth_and_measures_clock_offset() {
    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    // Server clock 10 minutes ahead of this device → offset ≈ -600 000 ms.
    Mock::given(method("POST"))
        .and(path("/v1/agent/heartbeat"))
        .and(body_string_contains("\"queue_depth\":3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "server_time": Utc::now() + chrono::Duration::minutes(10),
        })))
        .expect(1)
        .mount(&f.server)
        .await;

    f.fill(3);
    let task = HeartbeatTask {
        warehouse: f.warehouse.clone(),
        buffer: f.buffer.clone(),
        clock: f.clock.clone(),
        every: Duration::from_secs(60),
    };
    task.beat().await;
    let offset = f.clock.get();
    assert!((-601_000..=-599_000).contains(&offset), "offset {offset}");
}

// ------------------------------------------------------------- agent config (AG-7)

#[tokio::test]
async fn config_is_fetched_cached_and_revalidated_with_its_etag() {
    use crate::remote_config::{ConfigCache, ConfigTask};

    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    // With the cached ETag → 304; registered first so it takes priority.
    Mock::given(method("GET"))
        .and(path("/v1/agent/config"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304).insert_header("etag", "\"v1\""))
        .expect(1)
        .mount(&f.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/agent/config"))
        .and(header("authorization", "Bearer access-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_json(json!({
                    "exclusions": {"paths": ["/secret"], "apps": ["Slack"], "domains": []},
                    "pause_windows": [{"start": "12:00", "end": "13:00", "days": [1]}],
                    "project_roots": [], "redaction_version": 1,
                })),
        )
        .expect(1)
        .mount(&f.server)
        .await;

    let cache_file = f._home.path().join("agent_config.json");
    let task = ConfigTask {
        warehouse: f.warehouse.clone(),
        cache: Arc::new(ConfigCache::load(&cache_file)),
        every: Duration::from_secs(300),
    };
    assert_eq!(task.refresh().await, Duration::from_secs(300));
    let active = task.cache.current();
    assert_eq!(active.config.exclusions.apps, ["Slack"]);
    assert_eq!(active.config.pause_windows.len(), 1);
    assert_eq!(
        active.config.upload_max_events, 500,
        "missing fields default"
    );
    assert!(active.exclusions.excludes_path("/secret/notes.md"));

    // Second fetch sends the ETag and keeps the cached config on 304.
    task.refresh().await;
    assert_eq!(task.cache.current().config.exclusions.apps, ["Slack"]);

    // A restart starts from the cache, before any fetch.
    let restarted = ConfigCache::load(&cache_file);
    assert_eq!(restarted.etag().as_deref(), Some("\"v1\""));
    assert!(restarted.current().exclusions.excludes_app("slack.exe"));

    let requests = f.server.received_requests().await.unwrap();
    let config_calls: Vec<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/v1/agent/config")
        .collect();
    assert_eq!(config_calls.len(), 2);
    assert!(config_calls[0].headers.get("if-none-match").is_none());
}

#[tokio::test]
async fn config_fetch_failure_keeps_the_cached_config() {
    use crate::remote_config::{ConfigCache, ConfigTask};
    use secretary_shared::config::AgentConfig;

    let f = Fixture::new(true).await;
    f.token_ok("access-1", "refresh-2").await;
    Mock::given(method("GET"))
        .and(path("/v1/agent/config"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&f.server)
        .await;

    let cache = Arc::new(ConfigCache::load(&f._home.path().join("agent_config.json")));
    let mut cached = AgentConfig::default();
    cached.exclusions.paths.push("/secret".into());
    cache
        .replace(cached.clone(), Some("\"v0\"".into()))
        .unwrap();
    let task = ConfigTask {
        warehouse: f.warehouse.clone(),
        cache: cache.clone(),
        every: Duration::from_secs(300),
    };
    assert_eq!(
        task.refresh().await,
        Duration::from_secs(60),
        "retries sooner"
    );
    assert_eq!(cache.current().config, cached);
}
