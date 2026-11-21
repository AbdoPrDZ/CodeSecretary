//! Status for the panel (AG-14): a [`StatusBoard`] the components update in
//! memory, and a [`StatusWriter`] that saves it to `<data>/status.json` every
//! few seconds (`secretary_shared::local`).
//!
//! Cheap by design: a mutex-guarded struct and one small atomic file write
//! per period; no server, no port. Only states, counts and times; never event
//! metadata.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use secretary_shared::local::{AgentStatus, ConnectionState, STATUS_EVERY_SECS, StatusAccount};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::buffer::Buffer;
use crate::state::AgentState;

pub const WRITE_EVERY: Duration = Duration::from_secs(STATUS_EVERY_SECS);

pub struct StatusBoard {
    inner: Mutex<AgentStatus>,
}

impl Default for StatusBoard {
    fn default() -> Self {
        Self {
            inner: Mutex::new(AgentStatus {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                pid: std::process::id(),
                started_at: Some(Utc::now()),
                running: true,
                ..AgentStatus::default()
            }),
        }
    }
}

impl StatusBoard {
    pub fn update(&self, change: impl FnOnce(&mut AgentStatus)) {
        change(&mut self.inner.lock().expect("status lock"));
    }

    pub fn snapshot(&self) -> AgentStatus {
        self.inner.lock().expect("status lock").clone()
    }

    pub fn api_ok(&self) {
        self.update(|s| {
            s.connection.state = ConnectionState::Online;
            s.connection.last_ok_at = Some(Utc::now());
        });
    }

    /// `kind` is short and contains no event data (e.g. `HTTP 503`).
    pub fn api_failed(&self, kind: String) {
        self.update(|s| {
            s.connection.state = ConnectionState::Offline;
            s.connection.last_error_at = Some(Utc::now());
            s.connection.last_error = Some(kind);
        });
    }

    pub fn signed_out(&self) {
        self.update(|s| s.connection.state = ConnectionState::SignedOut);
    }
}

pub struct StatusWriter {
    pub board: Arc<StatusBoard>,
    pub buffer: Arc<Buffer>,
    pub state_file: PathBuf,
    pub out: PathBuf,
    pub every: Duration,
}

impl StatusWriter {
    pub async fn run(self, token: CancellationToken) -> anyhow::Result<()> {
        loop {
            self.write_logged();
            tokio::select! {
                () = token.cancelled() => break,
                () = tokio::time::sleep(self.every) => {}
            }
        }
        // Last word: the panel shows "stopped" instead of waiting to go stale.
        self.board.update(|s| s.running = false);
        self.write_logged();
        Ok(())
    }

    fn write_logged(&self) {
        if let Err(err) = self.write() {
            debug!(error = %format!("{err:#}"), "could not write the status file");
        }
    }

    pub fn write(&self) -> Result<()> {
        let state = AgentState::load(&self.state_file).unwrap_or_default();
        let queued = self.buffer.len().unwrap_or(0);
        let oldest = self.buffer.oldest().ok().flatten();
        self.board.update(|s| {
            s.written_at = Some(Utc::now());
            s.upload.queued = queued;
            s.upload.oldest_queued_at = oldest;
            s.account = state.account.map(|a| StatusAccount {
                email: a.email,
                server_url: a.server_url,
                device_id: a.device_id,
            });
            if s.account.is_none() {
                s.connection.state = ConnectionState::SignedOut;
            }
        });
        write_atomic(
            &self.out,
            &serde_json::to_vec_pretty(&self.board.snapshot())?,
        )
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("could not replace {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_a_live_status_then_a_stopped_one() {
        let dir = tempfile::tempdir().unwrap();
        let board = Arc::new(StatusBoard::default());
        board.api_failed("HTTP 503".into());
        let writer = StatusWriter {
            board: board.clone(),
            buffer: Arc::new(Buffer::open(&dir.path().join("buffer.sqlite3")).unwrap()),
            state_file: dir.path().join("state.json"),
            out: dir.path().join("status.json"),
            every: Duration::from_millis(20),
        };
        let out = writer.out.clone();
        let token = CancellationToken::new();
        let task = tokio::spawn(writer.run(token.clone()));
        tokio::time::sleep(Duration::from_millis(60)).await;

        let live: AgentStatus = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert!(live.is_live(Utc::now()));
        assert_eq!(live.pid, std::process::id());
        // Never signed in (no state.json) wins over the last error.
        assert_eq!(live.connection.state, ConnectionState::SignedOut);
        assert_eq!(live.connection.last_error.as_deref(), Some("HTTP 503"));

        token.cancel();
        task.await.unwrap().unwrap();
        let stopped: AgentStatus = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert!(!stopped.running && !stopped.is_live(Utc::now()));
        assert!(!out.with_extension("json.tmp").exists());
    }
}
