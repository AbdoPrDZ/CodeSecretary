//! Heartbeat (AG-5) and clock offset.
//!
//! Every [`HEARTBEAT_EVERY`] the agent reports its version, queue depth and
//! clock offset. The response carries the server clock; the offset is
//! `device clock - server clock`, measured at the midpoint of the round trip,
//! and goes into every upload batch so the warehouse can correct timestamps.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use secretary_shared::api::Heartbeat;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::api::{AGENT_VERSION, ApiError, Warehouse};
use crate::buffer::Buffer;

/// Appendix A: heartbeat interval.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(60);

/// Device clock minus server clock, in milliseconds. Starts at 0.
#[derive(Debug, Default)]
pub struct ClockOffset(AtomicI64);

impl ClockOffset {
    pub fn get(&self) -> i64 {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::Relaxed);
    }
}

/// Offset from one round trip: `sent`/`received` on the device clock,
/// `server` on the server clock.
pub fn measure(sent: DateTime<Utc>, received: DateTime<Utc>, server: DateTime<Utc>) -> i64 {
    let midpoint = sent + (received - sent) / 2;
    (midpoint - server).num_milliseconds()
}

pub struct HeartbeatTask {
    pub warehouse: Arc<Warehouse>,
    pub buffer: Arc<Buffer>,
    pub clock: Arc<ClockOffset>,
    pub every: Duration,
}

impl HeartbeatTask {
    pub async fn run(self, token: CancellationToken) -> anyhow::Result<()> {
        // The first beat comes quickly so uploads get a measured clock offset.
        let mut wait = Duration::from_secs(1);
        loop {
            tokio::select! {
                () = token.cancelled() => return Ok(()),
                () = tokio::time::sleep(wait) => {}
            }
            wait = self.every;
            tokio::select! {
                () = token.cancelled() => return Ok(()),
                () = self.beat() => {}
            }
        }
    }

    pub async fn beat(&self) {
        let queue_depth = self.buffer.len().unwrap_or(0);
        let beat = Heartbeat {
            agent_version: AGENT_VERSION.to_owned(),
            queue_depth,
            clock_offset_ms: self.clock.get(),
        };
        let sent = Utc::now();
        match self.warehouse.heartbeat(&beat).await {
            Ok(response) => {
                let offset = measure(sent, Utc::now(), response.server_time);
                self.clock.set(offset);
                debug!(queue_depth, clock_offset_ms = offset, "heartbeat");
            }
            Err(ApiError::SignedOut) => {}
            Err(err) => warn!(error = %err, "heartbeat failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn offset_uses_round_trip_midpoint() {
        // Device sent at :00.000, got the answer at :00.200; server said :00.000 → +100 ms fast.
        let offset = measure(
            t("2026-11-07T09:00:00.000Z"),
            t("2026-11-07T09:00:00.200Z"),
            t("2026-11-07T09:00:00.000Z"),
        );
        assert_eq!(offset, 100);
        // Device 10 minutes slow.
        let offset = measure(
            t("2026-11-07T08:50:00.000Z"),
            t("2026-11-07T08:50:00.000Z"),
            t("2026-11-07T09:00:00.000Z"),
        );
        assert_eq!(offset, -600_000);
    }
}
