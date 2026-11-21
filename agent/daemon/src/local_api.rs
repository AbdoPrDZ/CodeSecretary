//! Local API (AG-12): editor extensions and shell hooks hand their events to
//! the daemon here (AGENT plan §3.4, contract in `secretary_shared::local`).
//!
//! - Binds `127.0.0.1` on a random free port and writes the port and one
//!   bearer token per collector to `<config>/local.json`, readable by the
//!   current user only. New tokens at every start; the file is removed on a
//!   clean stop.
//! - Refuses any request with an `Origin` header (web pages) or a `Host`
//!   other than this loopback address (DNS rebinding), then requires the
//!   bearer token of a known collector.
//! - A collector may send only its own event types, with its own `source`
//!   name. Accepted events go through the same [`Pipeline`] as the daemon's
//!   own (validate, exclusions, pause, redact, buffer).
//!
//! Nothing from a request body is logged.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE, HOST, ORIGIN};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use secretary_shared::local::{
    COLLECTORS, LOCAL_EVENTS_PATH, LOCAL_STATUS_PATH, LocalAgentState, LocalApiFile, LocalEvents,
    LocalEventsResponse, LocalRejection, MAX_LOCAL_BODY, MAX_LOCAL_EVENTS, collector_allows,
};
use secretary_shared::validate;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::pipeline::{Outcome, Pipeline};
use crate::status::StatusBoard;

/// A client that sends no complete request head within this time is dropped.
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);

pub struct LocalApi {
    pub pipeline: Arc<Pipeline>,
    pub status: Arc<StatusBoard>,
    /// `<config>/local.json`.
    pub file: PathBuf,
}

impl LocalApi {
    pub async fn run(self, token: CancellationToken) -> Result<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context("could not bind the local API")?;
        let server = self.serve(listener)?;
        info!(port = server.port, "local API listening");
        let result = server.accept_until(token).await;
        // Clients must not find a stale port and tokens.
        let _ = std::fs::remove_file(&self.file);
        self.status.update(|s| s.local_api.listening = false);
        result
    }

    /// Generates tokens, writes `local.json` and returns the server.
    fn serve(&self, listener: TcpListener) -> Result<Server> {
        let port = listener.local_addr()?.port();
        let tokens: BTreeMap<String, String> = COLLECTORS
            .iter()
            .map(|name| ((*name).to_owned(), new_token()))
            .collect();
        write_private(
            &self.file,
            &LocalApiFile {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                pid: std::process::id(),
                port,
                tokens: tokens.clone(),
                written_at: Some(Utc::now()),
            },
        )?;
        self.status.update(|s| s.local_api.listening = true);
        Ok(Server {
            listener,
            port,
            handler: Arc::new(Handler {
                pipeline: self.pipeline.clone(),
                status: self.status.clone(),
                tokens,
                port,
            }),
        })
    }
}

struct Server {
    listener: TcpListener,
    port: u16,
    handler: Arc<Handler>,
}

impl Server {
    async fn accept_until(self, token: CancellationToken) -> Result<()> {
        loop {
            let (stream, peer) = tokio::select! {
                () = token.cancelled() => return Ok(()),
                accepted = self.listener.accept() => match accepted {
                    Ok(pair) => pair,
                    Err(err) => {
                        warn!(error = %err, "local API accept failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                },
            };
            if !peer.ip().is_loopback() {
                continue; // cannot happen on a loopback bind; refuse anyway
            }
            let handler = self.handler.clone();
            let token = token.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| {
                    let handler = handler.clone();
                    async move { Ok::<_, Infallible>(handler.handle(request).await) }
                });
                let mut builder = http1::Builder::new();
                builder
                    .timer(TokioTimer::new())
                    .header_read_timeout(HEADER_TIMEOUT);
                let connection = builder.serve_connection(TokioIo::new(stream), service);
                tokio::select! {
                    result = connection => {
                        if let Err(err) = result {
                            debug!(error = %err, "local API connection ended with an error");
                        }
                    }
                    () = token.cancelled() => {}
                }
            });
        }
    }
}

struct Handler {
    pipeline: Arc<Pipeline>,
    status: Arc<StatusBoard>,
    tokens: BTreeMap<String, String>,
    port: u16,
}

type Reply = Response<Full<Bytes>>;

impl Handler {
    async fn handle(&self, request: Request<Incoming>) -> Reply {
        if request.headers().contains_key(ORIGIN) {
            return error(StatusCode::FORBIDDEN, "origin_not_allowed");
        }
        let host = request.headers().get(HOST).and_then(|h| h.to_str().ok());
        if !self.host_allowed(host) {
            return error(StatusCode::FORBIDDEN, "host_not_allowed");
        }
        let Some(collector) = self.collector(&request) else {
            return error(StatusCode::UNAUTHORIZED, "invalid_token");
        };
        match (request.method(), request.uri().path()) {
            (&Method::GET, LOCAL_STATUS_PATH) => self.state(),
            (&Method::POST, LOCAL_EVENTS_PATH) => self.events(collector, request).await,
            (_, LOCAL_STATUS_PATH | LOCAL_EVENTS_PATH) => {
                error(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
            }
            _ => error(StatusCode::NOT_FOUND, "not_found"),
        }
    }

    fn host_allowed(&self, host: Option<&str>) -> bool {
        let port = self.port.to_string();
        host.and_then(|h| h.rsplit_once(':'))
            .is_some_and(|(name, p)| p == port && matches!(name, "127.0.0.1" | "localhost"))
    }

    /// The collector whose token the request carries.
    fn collector(&self, request: &Request<Incoming>) -> Option<String> {
        let header = request.headers().get(AUTHORIZATION)?.to_str().ok()?;
        let presented = header.strip_prefix("Bearer ")?.trim();
        self.tokens
            .iter()
            .find(|(_, token)| constant_time_eq(token.as_bytes(), presented.as_bytes()))
            .map(|(name, _)| name.clone())
    }

    fn state(&self) -> Reply {
        let status = self.status.snapshot();
        json(
            StatusCode::OK,
            &LocalAgentState {
                version: status.version,
                paused: status.config.paused,
                connection: status.connection.state,
            },
        )
    }

    async fn events(&self, collector: String, request: Request<Incoming>) -> Reply {
        let body = match Limited::new(request.into_body(), MAX_LOCAL_BODY)
            .collect()
            .await
        {
            Ok(body) => body.to_bytes(),
            Err(err) if err.is::<http_body_util::LengthLimitError>() => {
                return error(StatusCode::PAYLOAD_TOO_LARGE, "batch_too_large");
            }
            Err(_) => return error(StatusCode::BAD_REQUEST, "invalid_body"),
        };
        let Ok(batch) = serde_json::from_slice::<LocalEvents>(&body) else {
            return error(StatusCode::BAD_REQUEST, "invalid_body");
        };
        if batch.events.len() > MAX_LOCAL_EVENTS {
            return error(StatusCode::PAYLOAD_TOO_LARGE, "batch_too_large");
        }
        let pipeline = self.pipeline.clone();
        // The pipeline writes SQLite: keep it off the async workers.
        let result =
            tokio::task::spawn_blocking(move || submit(&pipeline, &collector, batch.events)).await;
        match result {
            Ok(Some(response)) => {
                if response.accepted > 0 {
                    let accepted = u64::from(response.accepted);
                    self.status.update(|s| {
                        s.local_api.events_since_start += accepted;
                        s.local_api.last_event_at = Some(Utc::now());
                    });
                }
                json(StatusCode::OK, &response)
            }
            // Retrying is safe: the buffer ignores event ids it already holds.
            Ok(None) | Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "buffer_unavailable"),
        }
    }
}

/// Checks and submits each event. `None` when the buffer could not be written.
fn submit(pipeline: &Pipeline, collector: &str, events: Vec<Value>) -> Option<LocalEventsResponse> {
    let mut response = LocalEventsResponse::default();
    let now = Utc::now();
    for (index, value) in events.into_iter().enumerate() {
        let reject = |reason: &str| LocalRejection {
            index: u32::try_from(index).unwrap_or(u32::MAX),
            reason: reason.to_owned(),
        };
        if let Some(reason) = foreign(collector, &value) {
            response.rejected.push(reject(reason));
            continue;
        }
        let event = match validate::parse(value, now) {
            Ok(event) => event,
            Err(reason) => {
                response.rejected.push(reject(reason.code()));
                continue;
            }
        };
        match pipeline.submit(event) {
            Outcome::Buffered(_) | Outcome::Excluded | Outcome::Paused => response.accepted += 1,
            Outcome::Invalid(reason) => response.rejected.push(reject(reason.code())),
            Outcome::Failed => return None,
        }
    }
    Some(response)
}

/// Why `collector` may not send this event, if it may not.
fn foreign(collector: &str, value: &Value) -> Option<&'static str> {
    let event_type = value.get("event_type").and_then(Value::as_str);
    if !event_type.is_some_and(|t| collector_allows(collector, t)) {
        return Some("forbidden_event_type");
    }
    let source = value.get("source").and_then(Value::as_str).unwrap_or("");
    match source.split_once('@') {
        Some((name, _)) if name == collector => None,
        _ => Some("source_mismatch"),
    }
}

fn json<T: serde::Serialize>(status: StatusCode, body: &T) -> Reply {
    let bytes = serde_json::to_vec(body).expect("response serializes");
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("valid response")
}

fn error(status: StatusCode, code: &str) -> Reply {
    json(status, &serde_json::json!({ "error": code }))
}

/// 256 random bits as hex (two v4 UUIDs: 244 random bits from the OS RNG,
/// plus fixed version bits).
fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Writes `value` atomically, readable by the current user only.
fn write_private(path: &Path, value: &LocalApiFile) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value)?;
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // Windows: the config directory is under %APPDATA%, whose profile ACLs
        // already exclude other users (see paths.rs).
        let mut file = options
            .open(&tmp)
            .with_context(|| format!("could not write {}", tmp.display()))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

/// Loopback address the server listens on (tests).
#[cfg(test)]
fn address(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::remote_config::ConfigCache;
    use secretary_shared::event::{EditorSave, Payload, WindowFocus};
    use secretary_shared::{Event, Source};

    struct Fixture {
        _dir: tempfile::TempDir,
        buffer: Arc<Buffer>,
        file: PathBuf,
        port: u16,
        token: String,
        stop: CancellationToken,
        task: tokio::task::JoinHandle<Result<()>>,
    }

    async fn start() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let buffer = Arc::new(Buffer::open(&dir.path().join("buffer.sqlite3")).unwrap());
        let cache = Arc::new(ConfigCache::load(&dir.path().join("agent_config.json")));
        let api = LocalApi {
            pipeline: Arc::new(Pipeline::new(buffer.clone(), cache)),
            status: Arc::new(StatusBoard::default()),
            file: dir.path().join("config").join("local.json"),
        };
        let file = api.file.clone();
        let stop = CancellationToken::new();
        let task = tokio::spawn(api.run(stop.clone()));
        for _ in 0..100 {
            if file.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let info: LocalApiFile = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        Fixture {
            buffer,
            port: info.port,
            token: info.tokens["vscode"].clone(),
            file,
            _dir: dir,
            stop,
            task,
        }
    }

    fn event(source: &str, payload: Payload) -> Value {
        let now = Utc::now();
        let mut event = Event::new(source.parse::<Source>().unwrap(), now, Some(now), payload);
        event.repository = Some(secretary_shared::event::RepositoryRef {
            root_path: Some("C:/work/api".into()),
            remote_url: None,
        });
        serde_json::to_value(event).unwrap()
    }

    fn save() -> Payload {
        Payload::EditorSave(EditorSave {
            file_path: "src/main.rs".into(),
        })
    }

    fn url(f: &Fixture, path: &str) -> String {
        format!("http://{}{path}", address(f.port))
    }

    #[tokio::test]
    async fn accepts_editor_events_and_buffers_them() {
        let f = start().await;
        let client = reqwest::Client::new();
        let body = serde_json::json!({ "events": [
            event("vscode@0.1.0", save()),
            event("agent@0.1.0", save()),
            event("vscode@0.1.0", Payload::WindowFocus(WindowFocus { app: None, title: None })),
            serde_json::json!({ "event_type": "editor.save", "source": "vscode@0.1.0" }),
        ]});
        let response = client
            .post(url(&f, LOCAL_EVENTS_PATH))
            .bearer_auth(&f.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let result: LocalEventsResponse = response.json().await.unwrap();
        assert_eq!(result.accepted, 1);
        let reasons: Vec<_> = result
            .rejected
            .iter()
            .map(|r| (r.index, r.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                (1, "source_mismatch"),
                (2, "forbidden_event_type"),
                (3, "missing_field")
            ]
        );
        assert_eq!(f.buffer.len().unwrap(), 1);

        let state: LocalAgentState = client
            .get(url(&f, LOCAL_STATUS_PATH))
            .bearer_auth(&f.token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!state.paused);

        f.stop.cancel();
        f.task.await.unwrap().unwrap();
        assert!(!f.file.exists(), "local.json is removed on stop");
    }

    #[tokio::test]
    async fn refuses_missing_token_origin_and_foreign_host() {
        let f = start().await;
        let client = reqwest::Client::new();
        let body = serde_json::json!({ "events": [event("vscode@0.1.0", save())] });
        let body = &body;
        let send = |request: reqwest::RequestBuilder| async move {
            let response = request.json(body).send().await.unwrap();
            let status = response.status().as_u16();
            let value: Value = response.json().await.unwrap();
            (
                status,
                value["error"].as_str().unwrap_or_default().to_owned(),
            )
        };
        let events = url(&f, LOCAL_EVENTS_PATH);

        let (status, code) = send(client.post(&events)).await;
        assert_eq!((status, code.as_str()), (401, "invalid_token"));
        let (status, code) = send(client.post(&events).bearer_auth("wrong")).await;
        assert_eq!((status, code.as_str()), (401, "invalid_token"));
        // A web page: refused even with the right token.
        let (status, code) = send(
            client
                .post(&events)
                .bearer_auth(&f.token)
                .header("Origin", "https://evil.example"),
        )
        .await;
        assert_eq!((status, code.as_str()), (403, "origin_not_allowed"));
        let (status, code) = send(
            client
                .post(&events)
                .bearer_auth(&f.token)
                .header("Host", format!("evil.example:{}", f.port)),
        )
        .await;
        assert_eq!((status, code.as_str()), (403, "host_not_allowed"));
        assert_eq!(f.buffer.len().unwrap(), 0);

        // Oversized body.
        let big = "x".repeat(MAX_LOCAL_BODY + 1);
        let response = client
            .post(&events)
            .bearer_auth(&f.token)
            .body(big)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 413);
        f.stop.cancel();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let f = start().await;
        let mode = std::fs::metadata(&f.file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        f.stop.cancel();
    }

    #[test]
    fn tokens_are_long_and_distinct() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(constant_time_eq(a.as_bytes(), a.as_bytes()));
        assert!(!constant_time_eq(a.as_bytes(), b.as_bytes()));
    }
}
