//! OAuth 2.0 / OIDC calls to Keycloak (D14): discovery, device authorization
//! grant (RFC 8628), refresh with rotation, revocation.
//!
//! Tokens are never logged or included in error messages.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub issuer: String,
    pub device_authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive() // tokens stay out of Debug output
    }
}

#[derive(Debug, Deserialize)]
struct OAuthError {
    error: String,
}

/// Why a refresh failed.
#[derive(Debug)]
pub enum RefreshError {
    /// `invalid_grant`: the session was revoked or expired. Sign in again.
    Revoked,
    /// Network or server trouble; retry later with the same token.
    Transient(anyhow::Error),
}

pub async fn discover(http: &reqwest::Client, issuer: &str) -> Result<Discovery> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let discovery: Discovery = http
        .get(&url)
        .send()
        .await
        .context("could not reach the identity provider")?
        .error_for_status()
        .context("identity provider discovery failed")?
        .json()
        .await
        .context("invalid identity provider discovery document")?;
    if discovery.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
        bail!("the identity provider reports a different issuer than the server configured");
    }
    Ok(discovery)
}

pub async fn request_device_code(
    http: &reqwest::Client,
    discovery: &Discovery,
    client_id: &str,
) -> Result<DeviceCode> {
    http.post(&discovery.device_authorization_endpoint)
        .form(&[("client_id", client_id), ("scope", "openid offline_access")])
        .send()
        .await
        .context("could not reach the identity provider")?
        .error_for_status()
        .context("the identity provider refused the device login")?
        .json()
        .await
        .context("invalid device authorization response")
}

/// Polls until the user approves (or denies, or the code expires).
pub async fn poll_device_token(
    http: &reqwest::Client,
    discovery: &Discovery,
    client_id: &str,
    code: &DeviceCode,
) -> Result<TokenResponse> {
    let mut interval = Duration::from_secs(code.interval.max(1));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(code.expires_in);
    loop {
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "the sign-in code expired before it was approved; run `secretary-agent login` again"
            );
        }
        let response = http
            .post(&discovery.token_endpoint)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", code.device_code.as_str()),
                ("client_id", client_id),
            ])
            .send()
            .await
            .context("could not reach the identity provider")?;
        if response.status().is_success() {
            return response.json().await.context("invalid token response");
        }
        let error = response
            .json::<OAuthError>()
            .await
            .map(|e| e.error)
            .unwrap_or_default();
        match error.as_str() {
            "authorization_pending" => {}
            "slow_down" => interval += Duration::from_secs(5),
            "access_denied" => bail!("sign-in was denied in the browser"),
            "expired_token" => bail!("the sign-in code expired; run `secretary-agent login` again"),
            "" => bail!("sign-in failed: unknown error"),
            other => bail!("sign-in failed: {other}"),
        }
    }
}

pub async fn refresh(
    http: &reqwest::Client,
    token_endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse, RefreshError> {
    let transient = |err: reqwest::Error| {
        RefreshError::Transient(anyhow::Error::new(err).context("token refresh"))
    };
    let response = http
        .post(token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(transient)?;
    let status = response.status();
    if status.is_success() {
        return response.json().await.map_err(transient);
    }
    let error = response
        .json::<OAuthError>()
        .await
        .map(|e| e.error)
        .unwrap_or_default();
    if error == "invalid_grant" || error == "unauthorized_client" {
        Err(RefreshError::Revoked)
    } else {
        Err(RefreshError::Transient(anyhow::anyhow!(
            "token refresh failed: HTTP {status} {error}"
        )))
    }
}

/// Best effort: ends the Keycloak session behind the refresh token.
pub async fn revoke(
    http: &reqwest::Client,
    revocation_endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<()> {
    http.post(revocation_endpoint)
        .form(&[
            ("token", refresh_token),
            ("token_type_hint", "refresh_token"),
            ("client_id", client_id),
        ])
        .send()
        .await
        .context("could not reach the identity provider")?
        .error_for_status()
        .context("token revocation failed")?;
    Ok(())
}

/// Reads one string claim from a JWT payload WITHOUT verifying it. Only for
/// showing the user their own account; the warehouse does the verification.
pub fn unverified_claim(jwt: &str, claim: &str) -> Option<String> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = base64url_decode(payload)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.get(claim)?.as_str().map(str::to_owned)
}

/// Minimal unpadded base64url decoding (avoids a dependency for one claim).
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unverified_claim_reads_payload() {
        // {"alg":"none"} . {"email":"dev@example.test","n":1} .
        let jwt = "eyJhbGciOiJub25lIn0.eyJlbWFpbCI6ImRldkBleGFtcGxlLnRlc3QiLCJuIjoxfQ.";
        assert_eq!(
            unverified_claim(jwt, "email").as_deref(),
            Some("dev@example.test")
        );
        assert_eq!(
            unverified_claim(jwt, "n"),
            None,
            "non-string claims are ignored"
        );
        assert_eq!(unverified_claim("garbage", "email"), None);
    }

    #[test]
    fn token_response_debug_hides_tokens() {
        let response = TokenResponse {
            access_token: "secret-access".into(),
            expires_in: 300,
            refresh_token: Some("secret-refresh".into()),
        };
        let debug = format!("{response:?}");
        assert!(!debug.contains("secret"), "{debug}");
    }
}
