//! Worker loop: claim → process (with a heartbeat) → complete / retry.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::calculate::{check_invariants, run};
use crate::load::{load, set_user};
use crate::queue::{self, Failed, Job};
use crate::write::replace_day;

/// Why a job did not complete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    /// The job was reclaimed by the reaper; our result was discarded.
    #[error("job no longer owned")]
    Lost,
    /// Retry later; the kind is stored in `last_error` (no data in it).
    #[error("job failed: {0}")]
    Failed(String),
}

impl From<sqlx::Error> for JobError {
    fn from(e: sqlx::Error) -> Self {
        JobError::Failed(queue::error_kind(&e))
    }
}

/// Processes one claimed job; must mark it done itself on success
/// ([`queue::complete`]) in the same transaction as its results.
pub trait Processor: Send + Sync + 'static {
    fn process(
        &self,
        pool: &PgPool,
        job: &Job,
        worker: &str,
    ) -> impl Future<Output = Result<(), JobError>> + Send;
}

/// The real engine: load the day, calculate, replace derived rows.
pub struct Calculator;

impl Processor for Calculator {
    async fn process(&self, pool: &PgPool, job: &Job, worker: &str) -> Result<(), JobError> {
        let started = std::time::Instant::now();
        let mut tx = pool.begin().await?;
        set_user(&mut tx, job.user_id).await?;
        let (ctx, stats) = load(&mut tx, job.user_id, job.local_day).await?;
        let out = run(&ctx);
        if check_invariants(&ctx, &out).is_err() {
            return Err(JobError::Failed("invariant".into()));
        }
        if !queue::still_owned(&mut tx, job, worker).await? {
            return Err(JobError::Lost);
        }
        replace_day(&mut tx, job.user_id, job.local_day, &ctx.settings, &out).await?;
        if !queue::complete(&mut tx, job, worker).await? {
            return Err(JobError::Lost);
        }
        tx.commit().await?;
        info!(
            job = job.id,
            user = job.user_id,
            day = %job.local_day,
            events = stats.events,
            skipped = stats.skipped,
            sessions = out.sessions.len(),
            assignments = out.assignments.len(),
            ms = started.elapsed().as_millis() as u64,
            "day calculated"
        );
        Ok(())
    }
}

/// Counters for health and tests.
#[derive(Debug, Default)]
pub struct Stats {
    pub done: AtomicU64,
    pub failed: AtomicU64,
    /// Unix seconds of the last loop iteration (any worker).
    pub last_tick: AtomicU64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Run one worker until `cancel`; the current job is finished first.
pub async fn run_worker<P: Processor>(
    pool: PgPool,
    processor: Arc<P>,
    name: String,
    poll: Duration,
    stats: Arc<Stats>,
    cancel: CancellationToken,
) {
    while !cancel.is_cancelled() {
        stats.last_tick.store(now_secs(), Ordering::Relaxed);
        match queue::claim(&pool, &name).await {
            Ok(Some(job)) => process_one(&pool, processor.as_ref(), &name, &job, &stats).await,
            Ok(None) => {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(poll) => {}
                }
            }
            Err(e) => {
                warn!(worker = %name, error = %queue::error_kind(&e), "claim failed");
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(poll * 5) => {}
                }
            }
        }
    }
    debug!(worker = %name, "worker stopped");
}

async fn process_one<P: Processor>(
    pool: &PgPool,
    processor: &P,
    name: &str,
    job: &Job,
    stats: &Stats,
) {
    let beat = {
        let (pool, job, name) = (pool.clone(), job.clone(), name.to_owned());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(queue::HEARTBEAT).await;
                if !matches!(queue::heartbeat(&pool, &job, &name).await, Ok(true)) {
                    break;
                }
            }
        })
    };
    let result = processor.process(pool, job, name).await;
    beat.abort();
    match result {
        Ok(()) => {
            stats.done.fetch_add(1, Ordering::Relaxed);
        }
        Err(JobError::Lost) => warn!(
            job = job.id,
            "job reclaimed while running; result discarded"
        ),
        Err(JobError::Failed(kind)) => {
            stats.failed.fetch_add(1, Ordering::Relaxed);
            match queue::fail(pool, job, name, &kind).await {
                Ok(outcome) => {
                    let level_dead = outcome == Failed::DeadLetter;
                    if level_dead {
                        warn!(job = job.id, user = job.user_id, day = %job.local_day, error = %kind, "job dead-lettered");
                    } else {
                        warn!(job = job.id, attempts = job.attempts, error = %kind, outcome = ?outcome, "job failed; will retry");
                    }
                }
                Err(e) => {
                    warn!(job = job.id, error = %queue::error_kind(&e), "could not record job failure")
                }
            }
        }
    }
}

/// Reclaim stale jobs every `every` until `cancel`.
pub async fn run_reaper(pool: PgPool, every: Duration, cancel: CancellationToken) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(every) => {}
        }
        match queue::reap(&pool).await {
            Ok(0) => {}
            Ok(n) => warn!(reclaimed = n, "reclaimed stale jobs"),
            Err(e) => warn!(error = %queue::error_kind(&e), "reaper failed"),
        }
    }
}
