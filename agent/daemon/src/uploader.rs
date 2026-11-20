//! Uploader (AG-4): moves buffered events to the warehouse.
//!
//! Every [`UPLOAD_EVERY`] (or as soon as the buffer holds
//! [`UPLOAD_THRESHOLD`](crate::buffer::UPLOAD_THRESHOLD) events) it sends the
//! oldest events in gzip batches of up to [`BATCH_EVENTS`], and deletes them
//! from the buffer only once the warehouse has acknowledged them (accepted,
//! duplicate or rejected). Resends are harmless: the warehouse deduplicates on
//! `event_id`, so a lost response costs a resend, never a duplicate.
//!
//! Network/server errors back off exponentially (1 s → 5 min, with jitter).
//! While signed out, events stay in the buffer.

use std::sync::Arc;
use std::time::Duration;

use secretary_shared::api::{BatchRequest, MAX_BATCH_BYTES};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::api::{ApiError, Warehouse};
use crate::buffer::Buffer;
use crate::heartbeat::ClockOffset;

/// Appendix A: upload every 60 s or at 500 events.
pub const UPLOAD_EVERY: Duration = Duration::from_secs(60);
pub const BATCH_EVENTS: usize = 500;
const BACKOFF_MIN: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(300);

pub struct Uploader {
    pub buffer: Arc<Buffer>,
    pub warehouse: Arc<Warehouse>,
    pub clock: Arc<ClockOffset>,
    pub every: Duration,
}

/// Outcome of one drain.
#[derive(Debug, PartialEq, Eq)]
pub enum Drained {
    /// Buffer empty (or nothing more to send right now).
    Done {
        sent: usize,
    },
    SignedOut,
    /// Stopped on an error; retry after backing off.
    Failed,
}

impl Uploader {
    pub async fn run(self, token: CancellationToken) -> anyhow::Result<()> {
        let mut backoff = Backoff::default();
        // A first attempt shortly after start delivers what an earlier run left behind.
        let mut wait = Duration::from_secs(3);
        loop {
            tokio::select! {
                () = token.cancelled() => return Ok(()),
                () = tokio::time::sleep(wait) => {}
                () = self.buffer.ready.notified() => {}
            }
            let outcome = tokio::select! {
                () = token.cancelled() => return Ok(()),
                outcome = self.drain() => outcome,
            };
            wait = match outcome {
                Drained::Failed => backoff.next(),
                Drained::Done { .. } | Drained::SignedOut => {
                    backoff.reset();
                    self.every
                }
            };
        }
    }

    /// Sends batches until the buffer is empty or an error stops it.
    pub async fn drain(&self) -> Drained {
        let mut sent = 0usize;
        let mut limit = BATCH_EVENTS;
        loop {
            let events = match self.buffer.peek(limit) {
                Ok(events) => events,
                Err(err) => {
                    warn!(error = %format!("{err:#}"), "could not read the event buffer");
                    return Drained::Failed;
                }
            };
            if events.is_empty() {
                if sent > 0 {
                    info!(sent, "events uploaded");
                }
                return Drained::Done { sent };
            }
            let count = events.len();
            let batch = BatchRequest {
                batch_id: uuid::Uuid::new_v4(),
                clock_offset_ms: self.clock.get(),
                events,
            };
            // Stay under the body limit: halve the batch until it fits.
            if count > 1 && Warehouse::encoded_len(&batch).is_ok_and(|n| n > MAX_BATCH_BYTES) {
                limit = (count / 2).max(1);
                continue;
            }

            match self.warehouse.upload(&batch).await {
                Ok(response) => {
                    let acked = response
                        .accepted
                        .iter()
                        .chain(&response.duplicates)
                        .map(|id| id.to_string())
                        .chain(response.rejected.iter().filter_map(|r| r.event_id.clone()))
                        .collect::<Vec<_>>();
                    if !response.rejected.is_empty() {
                        // Reasons only: event contents are never logged.
                        let reasons: Vec<&str> = response
                            .rejected
                            .iter()
                            .map(|r| r.reason.as_str())
                            .collect();
                        warn!(
                            rejected = response.rejected.len(),
                            ?reasons,
                            "warehouse rejected events"
                        );
                    }
                    if let Err(err) = self.buffer.ack(acked.iter().map(String::as_str)) {
                        warn!(error = %format!("{err:#}"), "could not remove uploaded events");
                        return Drained::Failed;
                    }
                    debug!(
                        accepted = response.accepted.len(),
                        duplicates = response.duplicates.len(),
                        "batch acknowledged"
                    );
                    sent += count;
                    if acked.len() < count {
                        // Something unacknowledged (e.g. a rejection without an id): try later.
                        return Drained::Done { sent };
                    }
                    limit = BATCH_EVENTS;
                }
                Err(ApiError::SignedOut) => return Drained::SignedOut,
                Err(ApiError::Refused { status, code }) => {
                    warn!(%status, code, "warehouse refused the batch");
                    if status == reqwest::StatusCode::PAYLOAD_TOO_LARGE && count > 1 {
                        limit = (count / 2).max(1);
                        continue;
                    }
                    return Drained::Failed;
                }
                Err(ApiError::Transient(err)) => {
                    warn!(error = %format!("{err:#}"), "upload failed; will retry");
                    return Drained::Failed;
                }
            }
        }
    }
}

/// Exponential backoff with ±20 % jitter.
#[derive(Debug)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self { next: BACKOFF_MIN }
    }
}

impl Backoff {
    pub fn next(&mut self) -> Duration {
        let base = self.next;
        self.next = (self.next * 2).min(BACKOFF_MAX);
        jitter(base)
    }

    pub fn reset(&mut self) {
        self.next = BACKOFF_MIN;
    }
}

fn jitter(base: Duration) -> Duration {
    // Cheap randomness is enough to spread retries; no need for a CSPRNG.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let factor = 0.8 + f64::from(nanos % 1000) / 1000.0 * 0.4;
    base.mul_f64(factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_five_minutes_and_resets() {
        let mut b = Backoff::default();
        let waits: Vec<Duration> = (0..12).map(|_| b.next()).collect();
        assert!(waits[0] >= Duration::from_millis(800) && waits[0] <= Duration::from_millis(1200));
        assert!(waits[3] >= Duration::from_millis(6400) && waits[3] <= Duration::from_millis(9600));
        assert!(waits[11] <= Duration::from_secs(360) && waits[11] >= Duration::from_secs(240));
        b.reset();
        assert!(b.next() <= Duration::from_millis(1200));
    }
}
