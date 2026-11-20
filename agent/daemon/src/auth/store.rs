//! Where the refresh token lives: the OS keychain (Windows Credential
//! Manager, macOS Keychain, Linux Secret Service). Never on disk in plain
//! text, never in logs. The access token is only ever held in memory.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};

/// Keychain service name for every CodeSecretary entry.
pub const KEYCHAIN_SERVICE: &str = "CodeSecretary";

pub trait TokenStore: Send + Sync {
    fn load(&self, key: &str) -> Result<Option<String>>;
    fn save(&self, key: &str, token: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// Keychain entry name for an installation's refresh token.
pub fn refresh_token_key(installation_id: &str) -> String {
    format!("refresh-token/{installation_id}")
}

pub struct KeyringStore;

impl KeyringStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN_SERVICE, key).context("could not open the OS keychain")
    }
}

impl TokenStore for KeyringStore {
    fn load(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err).context("could not read the refresh token from the OS keychain"),
        }
    }

    fn save(&self, key: &str, token: &str) -> Result<()> {
        Self::entry(key)?
            .set_password(token)
            .context("could not store the refresh token in the OS keychain")
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err).context("could not delete the refresh token from the OS keychain"),
        }
    }
}

/// In-memory store for tests.
#[derive(Default)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct MemoryStore {
    tokens: Mutex<HashMap<String, String>>,
}

impl TokenStore for MemoryStore {
    fn load(&self, key: &str) -> Result<Option<String>> {
        Ok(self.tokens.lock().unwrap().get(key).cloned())
    }

    fn save(&self, key: &str, token: &str) -> Result<()> {
        self.tokens
            .lock()
            .unwrap()
            .insert(key.to_owned(), token.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.tokens.lock().unwrap().remove(key);
        Ok(())
    }
}
