//! Local event buffer (AG-3): a SQLite FIFO queue in `<data>/buffer.sqlite3`.
//!
//! - Events are written **after** redaction; raw secrets never touch disk.
//! - Durable: WAL journal, every write is its own transaction, so a crash or
//!   kill loses nothing that was pushed.
//! - Idempotent: `event_id` is unique; pushing the same event twice is a no-op.
//! - Bounded: events older than [`MAX_AGE`] or beyond [`MAX_EVENTS`] are
//!   dropped oldest-first, and an `agent.buffer_overflow` event records how
//!   many.
//! - Rows leave only when the warehouse acknowledges them (accepted,
//!   duplicate or rejected).

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use secretary_shared::event::{AgentBufferOverflow, Payload};
use secretary_shared::{Event, Source};
use tokio::sync::Notify;
use tracing::warn;

/// Appendix A: agent local buffer limit.
pub const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 3600);
/// Hard cap on rows (~250 MB at ~500 B/event).
pub const MAX_EVENTS: u64 = 500_000;
/// Wake the uploader early once this many events are waiting (Appendix A: 500 events).
pub const UPLOAD_THRESHOLD: u64 = 500;

pub struct Buffer {
    conn: Mutex<Connection>,
    max_events: u64,
    max_age: Duration,
    /// Signalled when [`UPLOAD_THRESHOLD`] events are waiting.
    pub ready: Notify,
}

impl Buffer {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_limits(path, MAX_EVENTS, MAX_AGE)
    }

    pub fn open_with_limits(path: &Path, max_events: u64, max_age: Duration) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("could not open the event buffer {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                 seq INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_id TEXT NOT NULL UNIQUE,
                 timestamp_start_ms INTEGER NOT NULL,
                 payload TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS events_start_ix ON events (timestamp_start_ms);",
        )
        .context("could not create the event buffer schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
            max_events,
            max_age,
            ready: Notify::new(),
        })
    }

    /// Appends an (already redacted) event.
    pub fn push(&self, event: &Event) -> Result<()> {
        let payload = serde_json::to_string(event)?;
        let depth = {
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO events (event_id, timestamp_start_ms, payload) VALUES (?1, ?2, ?3)",
                params![
                    event.event_id.to_string(),
                    event.timestamp_start.timestamp_millis(),
                    payload
                ],
            )?;
            let dropped = self.enforce_bounds(&conn, Utc::now())?;
            if dropped > 0 {
                warn!(dropped, "event buffer full; dropped the oldest events");
                let overflow = Event::new(
                    agent_source(),
                    Utc::now(),
                    None,
                    Payload::AgentBufferOverflow(AgentBufferOverflow { dropped }),
                );
                conn.execute(
                    "INSERT OR IGNORE INTO events (event_id, timestamp_start_ms, payload) VALUES (?1, ?2, ?3)",
                    params![
                        overflow.event_id.to_string(),
                        overflow.timestamp_start.timestamp_millis(),
                        serde_json::to_string(&overflow)?
                    ],
                )?;
            }
            count(&conn)?
        };
        if depth >= UPLOAD_THRESHOLD {
            self.ready.notify_one();
        }
        Ok(())
    }

    /// The oldest `limit` events, in push order. Rows that no longer parse
    /// (corruption, an incompatible older agent) are deleted and skipped.
    pub fn peek(&self, limit: usize) -> Result<Vec<Event>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare_cached("SELECT seq, payload FROM events ORDER BY seq LIMIT ?1")?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut events = Vec::with_capacity(rows.len());
        let mut corrupt = 0usize;
        for (seq, payload) in rows {
            match serde_json::from_str::<Event>(&payload) {
                Ok(event) => events.push(event),
                Err(_) => {
                    conn.execute("DELETE FROM events WHERE seq = ?1", [seq])?;
                    corrupt += 1;
                }
            }
        }
        if corrupt > 0 {
            warn!(corrupt, "dropped unreadable events from the buffer");
        }
        Ok(events)
    }

    /// Removes acknowledged events.
    pub fn ack<'a>(&self, event_ids: impl IntoIterator<Item = &'a str>) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut removed = 0;
        {
            let mut stmt = tx.prepare_cached("DELETE FROM events WHERE event_id = ?1")?;
            for id in event_ids {
                removed += stmt.execute([id])?;
            }
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn len(&self) -> Result<u64> {
        count(&self.conn.lock().unwrap())
    }

    /// Oldest buffered event start, for diagnostics.
    #[allow(dead_code)]
    pub fn oldest(&self) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn.lock().unwrap();
        let ms: Option<i64> = conn
            .query_row("SELECT min(timestamp_start_ms) FROM events", [], |r| {
                r.get(0)
            })
            .optional()?
            .flatten();
        Ok(ms.and_then(DateTime::from_timestamp_millis))
    }

    fn enforce_bounds(&self, conn: &Connection, now: DateTime<Utc>) -> Result<u64> {
        let cutoff = now.timestamp_millis() - self.max_age.as_millis() as i64;
        let mut dropped =
            conn.execute("DELETE FROM events WHERE timestamp_start_ms < ?1", [cutoff])? as u64;
        let total = count(conn)?;
        if total > self.max_events {
            dropped += conn.execute(
                "DELETE FROM events WHERE seq IN (SELECT seq FROM events ORDER BY seq LIMIT ?1)",
                [(total - self.max_events) as i64],
            )? as u64;
        }
        Ok(dropped)
    }
}

pub fn agent_source() -> Source {
    Source::new("agent", env!("CARGO_PKG_VERSION"))
}

fn count(conn: &Connection) -> Result<u64> {
    let n: i64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
    Ok(n as u64)
}

#[cfg(test)]
mod tests {
    use secretary_shared::event::Empty;

    use super::*;

    fn presence(start: DateTime<Utc>) -> Event {
        Event::new(
            agent_source(),
            start,
            Some(start + chrono::Duration::seconds(30)),
            Payload::PresenceActive(Empty {}),
        )
    }

    #[test]
    fn fifo_dedupe_and_ack() {
        let tmp = tempfile::tempdir().unwrap();
        let buffer = Buffer::open(&tmp.path().join("b.sqlite3")).unwrap();
        let now = Utc::now();
        let events: Vec<Event> = (0..5)
            .map(|i| presence(now - chrono::Duration::minutes(10 - i)))
            .collect();
        for e in &events {
            buffer.push(e).unwrap();
        }
        buffer.push(&events[0]).unwrap(); // resent by a collector: ignored
        assert_eq!(buffer.len().unwrap(), 5);

        let peeked = buffer.peek(3).unwrap();
        assert_eq!(peeked, events[..3]);

        let ids: Vec<String> = peeked.iter().map(|e| e.event_id.to_string()).collect();
        assert_eq!(buffer.ack(ids.iter().map(String::as_str)).unwrap(), 3);
        assert_eq!(buffer.peek(10).unwrap(), events[3..]);
    }

    #[test]
    fn survives_reopen_like_a_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("b.sqlite3");
        let event = presence(Utc::now());
        {
            let buffer = Buffer::open(&path).unwrap();
            buffer.push(&event).unwrap();
            std::mem::forget(buffer); // no clean close, as if the process died
        }
        let buffer = Buffer::open(&path).unwrap();
        assert_eq!(buffer.peek(10).unwrap(), vec![event]);
    }

    #[test]
    fn overflow_drops_oldest_and_records_it() {
        let tmp = tempfile::tempdir().unwrap();
        let buffer = Buffer::open_with_limits(&tmp.path().join("b.sqlite3"), 3, MAX_AGE).unwrap();
        let now = Utc::now();
        let events: Vec<Event> = (0..4)
            .map(|i| presence(now - chrono::Duration::minutes(10 - i)))
            .collect();
        for e in &events {
            buffer.push(e).unwrap();
        }
        let left = buffer.peek(10).unwrap();
        // Oldest dropped; the overflow marker is appended after the newest.
        assert_eq!(left[..3], events[1..]);
        assert_eq!(left.len(), 4);
        match &left[3].payload {
            Payload::AgentBufferOverflow(o) => assert_eq!(o.dropped, 1),
            other => panic!("expected overflow event, got {other:?}"),
        }
    }

    #[test]
    fn events_older_than_max_age_are_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let buffer = Buffer::open_with_limits(
            &tmp.path().join("b.sqlite3"),
            MAX_EVENTS,
            Duration::from_secs(3600),
        )
        .unwrap();
        buffer
            .push(&presence(Utc::now() - chrono::Duration::hours(2)))
            .unwrap();
        let fresh = presence(Utc::now());
        buffer.push(&fresh).unwrap();
        // The stale event is dropped as soon as it is pushed, then the fresh one follows.
        let left = buffer.peek(10).unwrap();
        assert_eq!(left.len(), 2);
        assert!(matches!(left[0].payload, Payload::AgentBufferOverflow(_)));
        assert_eq!(left[1], fresh);
    }

    #[test]
    fn unreadable_rows_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("b.sqlite3");
        let buffer = Buffer::open(&path).unwrap();
        let good = presence(Utc::now());
        buffer
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO events (event_id, timestamp_start_ms, payload) VALUES ('x', ?1, '{bad')",
                [Utc::now().timestamp_millis()],
            )
            .unwrap();
        buffer.push(&good).unwrap();
        assert_eq!(buffer.peek(10).unwrap(), vec![good]);
        assert_eq!(buffer.len().unwrap(), 1);
    }
}
