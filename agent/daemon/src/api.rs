//! Warehouse device API client (`/v1/...`, bodies from `secretary_shared::api`).
//!
//! Every call carries the access token from the [`Link`]. On `401` the token
//! is refreshed once and the call retried; a second `401` (e.g.
//! `device_revoked`) signs the agent out. On `403 device_not_registered` the
//! device registers itself and the call is retried once.
//!
//! `GET /v1/agent/config` is conditional: the cached `ETag` goes in
//! `If-None-Match`, and `304` means the cached config is still current.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use flate2::Compression;
use flate2::write::GzEncoder;
use reqwest::StatusCode;
use secretary_shared::api::{
    ApiError as ErrorBody, BatchRequest, BatchResponse, DeviceRegistered, Heartbeat,
    HeartbeatResponse, RegisterDevice, error_code,
};
use secretary_shared::config::AgentConfig;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::info;

use crate::auth::{Link, LinkError};

pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub enum ApiError {
    /// Not signed in (or signed out just now). Wait for `login`.
    SignedOut,
    /// Network, 5xx or 429: retry later.
    Transient(anyhow::Error),
    /// The server refused the request for good (4xx other than auth).
    Refused { status: StatusCode, code: String },
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignedOut => f.write_str("not signed in"),
            Self::Transient(err) => write!(f, "{err:#}"),
            Self::Refused { status, code } => write!(f, "refused: HTTP {status} {code}"),
        }
    }
}

impl From<LinkError> for ApiError {
    fn from(err: LinkError) -> Self {
        match err {
            LinkError::SignedOut => Self::SignedOut,
            LinkError::Transient(err) => Self::Transient(err),
        }
    }
}

/// A request body, prepared once and re-sent on retry.
enum Body {
    /// `GET`, no body.
    None,
    Json(Vec<u8>),
    Gzip(Vec<u8>),
}

/// Result of [`Warehouse::fetch_config`].
#[derive(Debug, PartialEq)]
pub enum ConfigFetch {
    /// `304`: the cached config (with the `ETag` sent) is current.
    NotModified,
    Changed {
        config: AgentConfig,
        etag: Option<String>,
    },
}

pub struct Warehouse {
    http: reqwest::Client,
    link: Arc<Link>,
}

impl Warehouse {
    pub fn new(http: reqwest::Client, link: Arc<Link>) -> Self {
        Self { http, link }
    }

    /// `POST /v1/devices`, then remembers the device id.
    pub async fn register(&self) -> Result<i64, ApiError> {
        let body = RegisterDevice {
            name: device_name(),
            os: std::env::consts::OS.to_owned(),
            agent_version: AGENT_VERSION.to_owned(),
        };
        let registered: DeviceRegistered = self
            .send("/v1/devices", Body::Json(json(&body)?), false)
            .await?;
        self.link
            .set_device_id(registered.device_id)
            .await
            .map_err(ApiError::Transient)?;
        info!(device_id = registered.device_id, "device registered");
        Ok(registered.device_id)
    }

    /// `POST /v1/events:batch` (gzip JSON).
    pub async fn upload(&self, batch: &BatchRequest) -> Result<BatchResponse, ApiError> {
        self.send("/v1/events:batch", Body::Gzip(gzip(&json(batch)?)?), true)
            .await
    }

    /// Compressed size of a batch, to stay under the server's body limit.
    pub fn encoded_len(batch: &BatchRequest) -> Result<usize, ApiError> {
        Ok(gzip(&json(batch)?)?.len())
    }

    /// `POST /v1/agent/heartbeat`.
    pub async fn heartbeat(&self, beat: &Heartbeat) -> Result<HeartbeatResponse, ApiError> {
        self.send("/v1/agent/heartbeat", Body::Json(json(beat)?), true)
            .await
    }

    /// `GET /v1/agent/config`, conditional on the cached `etag`.
    pub async fn fetch_config(&self, etag: Option<&str>) -> Result<ConfigFetch, ApiError> {
        let response = self
            .exchange("/v1/agent/config", &Body::None, etag, true)
            .await?;
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(ConfigFetch::NotModified);
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let config = response.json().await.map_err(|err| {
            ApiError::Transient(anyhow::Error::new(err).context("invalid agent config"))
        })?;
        Ok(ConfigFetch::Changed { config, etag })
    }

    async fn send<R: DeserializeOwned>(
        &self,
        path: &str,
        body: Body,
        may_register: bool,
    ) -> Result<R, ApiError> {
        let response = self.exchange(path, &body, None, may_register).await?;
        response.json().await.map_err(|err| {
            ApiError::Transient(anyhow::Error::new(err).context("invalid warehouse response"))
        })
    }

    /// One call with the auth retries; returns any 2xx or `304` response.
    async fn exchange(
        &self,
        path: &str,
        body: &Body,
        if_none_match: Option<&str>,
        may_register: bool,
    ) -> Result<reqwest::Response, ApiError> {
        let (mut token, mut account) = self.link.access_token().await?;
        let mut refreshed = false;
        let mut registered = !may_register;
        loop {
            let url = format!("{}{path}", account.server_url.trim_end_matches('/'));
            let request = match body {
                Body::None => self.http.get(&url),
                _ => self.http.post(&url),
            }
            .bearer_auth(&token);
            let request = match if_none_match {
                Some(tag) => request.header(reqwest::header::IF_NONE_MATCH, tag),
                None => request,
            };
            let request = match body {
                Body::None => request,
                Body::Json(bytes) => request
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(bytes.clone()),
                Body::Gzip(bytes) => request
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .header(reqwest::header::CONTENT_ENCODING, "gzip")
                    .body(bytes.clone()),
            };
            let response = request.send().await.map_err(|err| {
                ApiError::Transient(anyhow::Error::new(err).context("warehouse unreachable"))
            })?;
            let status = response.status();
            if status.is_success() || status == StatusCode::NOT_MODIFIED {
                return Ok(response);
            }
            let code = response
                .json::<ErrorBody>()
                .await
                .map(|e| e.error)
                .unwrap_or_default();

            match status {
                StatusCode::UNAUTHORIZED if !refreshed => {
                    refreshed = true;
                    (token, account) = self.link.refresh_now().await?;
                }
                StatusCode::UNAUTHORIZED => {
                    let reason = if code == error_code::DEVICE_REVOKED {
                        "device revoked"
                    } else {
                        "warehouse rejected the token"
                    };
                    self.link.sign_out(reason).await;
                    return Err(ApiError::SignedOut);
                }
                StatusCode::FORBIDDEN
                    if code == error_code::DEVICE_NOT_REGISTERED && !registered =>
                {
                    registered = true;
                    Box::pin(self.register()).await?;
                }
                StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT => {
                    return Err(ApiError::Transient(anyhow::anyhow!(
                        "warehouse busy: HTTP {status}"
                    )));
                }
                s if s.is_server_error() => {
                    return Err(ApiError::Transient(anyhow::anyhow!(
                        "warehouse error: HTTP {status}"
                    )));
                }
                _ => return Err(ApiError::Refused { status, code }),
            }
        }
    }
}

/// HTTP client shared by auth and the warehouse calls.
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("secretary-agent/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .context("could not create the HTTP client")
}

/// Device name shown in the dashboard's device list (the machine's hostname).
pub fn device_name() -> String {
    let name = gethostname::gethostname()
        .to_string_lossy()
        .trim()
        .to_owned();
    if name.is_empty() {
        "unnamed device".to_owned()
    } else {
        name.chars().take(255).collect()
    }
}

fn json<T: Serialize>(value: &T) -> Result<Vec<u8>, ApiError> {
    serde_json::to_vec(value).map_err(|err| ApiError::Transient(err.into()))
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, ApiError> {
    let mut encoder = GzEncoder::new(Vec::with_capacity(bytes.len() / 4), Compression::default());
    encoder
        .write_all(bytes)
        .and_then(|()| encoder.finish())
        .map_err(|err| ApiError::Transient(err.into()))
}
