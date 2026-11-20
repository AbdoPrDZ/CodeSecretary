//! `state.json`: what the agent remembers about its sign-in. No secrets: the
//! refresh token lives in the OS keychain, the access token only in memory.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentState {
    /// Random per installation; names the keychain entry, so separate agent
    /// homes (tests, portable setups) never share a token.
    pub installation_id: Option<String>,
    /// Present while signed in.
    pub account: Option<Account>,
}

/// Where and as whom the agent is signed in, cached at login so the daemon
/// can refresh tokens without discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Warehouse the device is registered with.
    pub server_url: String,
    pub issuer: String,
    pub client_id: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    /// `devices.id` returned by `POST /v1/devices`.
    #[serde(default)]
    pub device_id: Option<i64>,
    /// Shown by `status`; the user's own address, never logged.
    #[serde(default)]
    pub email: Option<String>,
}

impl AgentState {
    /// A missing file is the empty state.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("invalid state file {}", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err).with_context(|| format!("could not read {}", path.display())),
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("could not replace {}", path.display()))
    }

    /// The installation id, created on first use.
    pub fn installation_id(&mut self) -> String {
        self.installation_id
            .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_signed_out() {
        let tmp = tempfile::tempdir().unwrap();
        let state = AgentState::load(&tmp.path().join("state.json")).unwrap();
        assert_eq!(state, AgentState::default());
        assert!(state.account.is_none());
    }

    #[test]
    fn round_trip_and_stable_installation_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.json");
        let mut state = AgentState::default();
        let id = state.installation_id();
        assert_eq!(state.installation_id(), id);
        state.account = Some(Account {
            server_url: "http://localhost:8080".into(),
            issuer: "http://localhost:8180/realms/secretary".into(),
            client_id: "secretary-agent".into(),
            token_endpoint: "http://localhost:8180/realms/secretary/protocol/openid-connect/token"
                .into(),
            revocation_endpoint: None,
            device_id: Some(7),
            email: None,
        });
        state.save(&path).unwrap();
        assert_eq!(AgentState::load(&path).unwrap(), state);
        assert!(!path.with_extension("json.tmp").exists());
    }
}
