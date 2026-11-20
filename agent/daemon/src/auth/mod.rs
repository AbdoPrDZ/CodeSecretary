//! Keycloak sign-in for the agent (D14, AG-2).
//!
//! - `secretary-agent login` runs the OAuth 2.0 device authorization grant
//!   ([`oidc`]): the user approves in a browser, the agent receives an
//!   offline refresh token and keeps it in the OS keychain ([`store`]).
//! - The daemon holds a [`Link`]: it loads that refresh token, keeps a
//!   short-lived access token in memory, refreshes it before it expires
//!   (Keycloak rotates the refresh token every time; the new one replaces the
//!   old one in the keychain), and signs out when Keycloak says the session
//!   is gone (`invalid_grant`, e.g. the device was revoked).
//! - While signed out, the daemon keeps collecting into the local buffer and
//!   re-checks the keychain every [`RELOAD_EVERY`], so a `login` in another
//!   terminal takes effect without a restart.
//!
//! PRIVACY/SECURITY: tokens are never logged, printed, or written to disk.

pub mod oidc;
pub mod store;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::{info, warn};

use crate::state::{Account, AgentState};
use store::{TokenStore, refresh_token_key};

/// Refresh the access token when it has less than this left.
const EXPIRY_MARGIN: Duration = Duration::from_secs(30);
/// While signed out, how often the daemon looks for a new sign-in.
pub const RELOAD_EVERY: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum LinkError {
    /// No usable sign-in. Uploads wait; the user must run `secretary-agent login`.
    SignedOut,
    /// Network/server trouble; retry later.
    Transient(anyhow::Error),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignedOut => f.write_str("not signed in"),
            Self::Transient(err) => write!(f, "{err:#}"),
        }
    }
}

struct Session {
    account: Account,
    key: String,
    refresh_token: String,
    access: Option<(String, Instant)>,
}

struct Inner {
    session: Option<Session>,
    next_reload: Instant,
    /// Logged once per sign-out, not on every reload.
    announced_signed_out: bool,
}

/// The daemon's connection to its sign-in. Shared by the uploader and heartbeat.
pub struct Link {
    state_file: PathBuf,
    store: Arc<dyn TokenStore>,
    http: reqwest::Client,
    inner: Mutex<Inner>,
}

impl Link {
    pub fn new(state_file: PathBuf, store: Arc<dyn TokenStore>, http: reqwest::Client) -> Self {
        Self {
            state_file,
            store,
            http,
            inner: Mutex::new(Inner {
                session: None,
                next_reload: Instant::now(),
                announced_signed_out: false,
            }),
        }
    }

    /// A valid access token and the account it belongs to.
    pub async fn access_token(&self) -> Result<(String, Account), LinkError> {
        let mut inner = self.inner.lock().await;
        self.ensure_session(&mut inner);
        let Some(session) = inner.session.as_mut() else {
            return Err(LinkError::SignedOut);
        };
        if let Some((token, expires)) = &session.access
            && *expires > Instant::now() + EXPIRY_MARGIN
        {
            return Ok((token.clone(), session.account.clone()));
        }
        self.refresh_locked(&mut inner).await
    }

    /// Refreshes even if the current access token looks valid (after a 401).
    pub async fn refresh_now(&self) -> Result<(String, Account), LinkError> {
        let mut inner = self.inner.lock().await;
        self.ensure_session(&mut inner);
        if inner.session.is_none() {
            return Err(LinkError::SignedOut);
        }
        self.refresh_locked(&mut inner).await
    }

    /// Forgets the sign-in: clears the keychain entry and the account in
    /// `state.json`, unless a newer `login` has already replaced them.
    pub async fn sign_out(&self, reason: &'static str) {
        let mut inner = self.inner.lock().await;
        self.sign_out_locked(&mut inner, reason);
    }

    /// Persists the device id after (re-)registration.
    pub async fn set_device_id(&self, device_id: i64) -> anyhow::Result<()> {
        let mut inner = self.inner.lock().await;
        if let Some(session) = inner.session.as_mut() {
            session.account.device_id = Some(device_id);
        }
        let mut state = AgentState::load(&self.state_file)?;
        if let Some(account) = state.account.as_mut() {
            account.device_id = Some(device_id);
            state.save(&self.state_file)?;
        }
        Ok(())
    }

    fn ensure_session(&self, inner: &mut Inner) {
        if inner.session.is_some() || Instant::now() < inner.next_reload {
            return;
        }
        inner.next_reload = Instant::now() + RELOAD_EVERY;
        match self.load_session() {
            Ok(Some(session)) => {
                info!(device_id = ?session.account.device_id, "signed in");
                inner.session = Some(session);
                inner.announced_signed_out = false;
            }
            Ok(None) => {
                if !inner.announced_signed_out {
                    info!("not signed in; run `secretary-agent login` to start uploading");
                    inner.announced_signed_out = true;
                }
            }
            Err(err) => warn!(error = %format!("{err:#}"), "could not load the sign-in"),
        }
    }

    fn load_session(&self) -> anyhow::Result<Option<Session>> {
        let state = AgentState::load(&self.state_file)?;
        let (Some(account), Some(installation_id)) = (state.account, state.installation_id) else {
            return Ok(None); // never signed in here: don't touch the keychain at all
        };
        let key = refresh_token_key(&installation_id);
        Ok(self.store.load(&key)?.map(|refresh_token| Session {
            account,
            key,
            refresh_token,
            access: None,
        }))
    }

    async fn refresh_locked(&self, inner: &mut Inner) -> Result<(String, Account), LinkError> {
        let session = inner.session.as_mut().ok_or(LinkError::SignedOut)?;
        let result = oidc::refresh(
            &self.http,
            &session.account.token_endpoint,
            &session.account.client_id,
            &session.refresh_token,
        )
        .await;
        match result {
            Ok(tokens) => {
                if let Some(rotated) = tokens.refresh_token
                    && rotated != session.refresh_token
                {
                    // Rotation: the old token is now dead, so the new one must be kept.
                    if let Err(err) = self.store.save(&session.key, &rotated) {
                        warn!(error = %format!("{err:#}"), "could not store the rotated refresh token");
                    }
                    session.refresh_token = rotated;
                }
                let expires = Instant::now() + Duration::from_secs(tokens.expires_in);
                session.access = Some((tokens.access_token.clone(), expires));
                Ok((tokens.access_token, session.account.clone()))
            }
            Err(oidc::RefreshError::Revoked) => {
                self.sign_out_locked(inner, "session ended");
                Err(LinkError::SignedOut)
            }
            Err(oidc::RefreshError::Transient(err)) => Err(LinkError::Transient(err)),
        }
    }

    fn sign_out_locked(&self, inner: &mut Inner, reason: &'static str) {
        if let Some(session) = inner.session.take() {
            let replaced = self
                .store
                .load(&session.key)
                .ok()
                .flatten()
                .is_some_and(|stored| stored != session.refresh_token);
            if !replaced {
                if let Err(err) = self.store.delete(&session.key) {
                    warn!(error = %format!("{err:#}"), "could not clear the keychain entry");
                }
                if let Err(err) = clear_account(&self.state_file) {
                    warn!(error = %format!("{err:#}"), "could not update the state file");
                }
            }
            warn!(
                reason,
                "signed out; run `secretary-agent login` to resume uploads"
            );
            inner.announced_signed_out = true;
        }
        inner.next_reload = Instant::now() + RELOAD_EVERY;
    }
}

fn clear_account(state_file: &std::path::Path) -> anyhow::Result<()> {
    let mut state = AgentState::load(state_file)?;
    if state.account.take().is_some() {
        state.save(state_file)?;
    }
    Ok(())
}
