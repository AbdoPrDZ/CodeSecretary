//! Job queue on `processing_jobs` (EN-2): Postgres `FOR UPDATE SKIP LOCKED`.
//!
//! - **Claim:** the oldest due `queued` job whose (user, day) has no
//!   `running` job. The partial unique index `processing_jobs_running_ux`
//!   makes a second running job for one (user, day) impossible even if two
//!   claims race; the loser sees a unique violation and claims nothing.
//! - **Heartbeat:** a running job's `locked_at` is refreshed every
//!   [`HEARTBEAT`]; a job not refreshed for [`STALE_AFTER_SECS`] is
//!   reclaimed by [`reap`] (the worker died).
//! - **Retry:** a failed job goes back to `queued` with exponential backoff;
//!   after [`MAX_ATTEMPTS`] it becomes `failed` (dead letter) with its error
//!   kind. If new input queued the same (user, day) meanwhile, that job
//!   inherits the attempt count and this one is removed: one recalculation
//!   covers both, since every job recomputes the whole day.
//!
//! Times come from the database clock, so workers on different hosts agree.
//! The table has no RLS (system queue) and holds only ids and days.

use std::time::Duration;

use chrono::NaiveDate;
use sqlx::{PgPool, Row};

/// Attempts before a job is dead-lettered.
pub const MAX_ATTEMPTS: i32 = 5;
/// A running job whose worker has not refreshed it for this long is reclaimed.
pub const STALE_AFTER_SECS: i64 = 300;
/// How often a worker refreshes its running job.
pub const HEARTBEAT: Duration = Duration::from_secs(30);
/// Finished jobs are kept this long for inspection.
pub const KEEP_DONE_DAYS: i64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: i64,
    pub user_id: i64,
    pub local_day: NaiveDate,
    pub reason: String,
    /// Including the current one.
    pub attempts: i32,
}

/// Backoff before attempt `attempts + 1`: 10 s, 20 s, 40 s, … capped at 30 min.
pub fn backoff_secs(attempts: i32) -> i64 {
    let exp = attempts.clamp(1, 16) - 1;
    (10_i64 << exp).min(30 * 60)
}

/// A stable, data-free name for an error (never log driver messages: their
/// details can quote row values).
pub fn error_kind(e: &sqlx::Error) -> String {
    match e {
        sqlx::Error::Database(d) => format!("db:{}", d.code().unwrap_or_default()),
        sqlx::Error::PoolTimedOut => "pool_timeout".into(),
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) => "io".into(),
        sqlx::Error::RowNotFound => "row_not_found".into(),
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode".into(),
        _ => "db".into(),
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.code().as_deref() == Some("23505"))
}

const CLAIM_SQL: &str = "
UPDATE processing_jobs j
   SET status = 'running', locked_at = now(), locked_by = $1,
       attempts = j.attempts + 1, updated_at = now()
  FROM (SELECT q.id FROM processing_jobs q
         WHERE q.status = 'queued' AND q.run_after <= now()
           AND NOT EXISTS (SELECT 1 FROM processing_jobs r
                            WHERE r.user_id = q.user_id AND r.local_day = q.local_day
                              AND r.status = 'running')
         ORDER BY q.run_after, q.id
         LIMIT 1
         FOR UPDATE SKIP LOCKED) c
 WHERE j.id = c.id
RETURNING j.id, j.user_id, j.local_day, j.reason, j.attempts";

/// Claim the next due job for `worker`, if any.
pub async fn claim(pool: &PgPool, worker: &str) -> Result<Option<Job>, sqlx::Error> {
    match sqlx::query(CLAIM_SQL)
        .bind(worker)
        .fetch_optional(pool)
        .await
    {
        Ok(row) => row.map(|r| job_from_row(&r)).transpose(),
        // Another worker started this (user, day) between our check and update.
        Err(e) if is_unique_violation(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

fn job_from_row(r: &sqlx::postgres::PgRow) -> Result<Job, sqlx::Error> {
    Ok(Job {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        local_day: r.try_get("local_day")?,
        reason: r.try_get("reason")?,
        attempts: r.try_get("attempts")?,
    })
}

/// Refresh the lock; `false` when the job is no longer ours (reaped).
pub async fn heartbeat(pool: &PgPool, job: &Job, worker: &str) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE processing_jobs SET locked_at = now()
          WHERE id = $1 AND locked_by = $2 AND status = 'running'",
    )
    .bind(job.id)
    .bind(worker)
    .execute(pool)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Mark a job done (inside the writer's transaction, so results and
/// completion commit together). `false` when the job is no longer ours.
pub async fn complete(
    conn: &mut sqlx::PgConnection,
    job: &Job,
    worker: &str,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE processing_jobs
            SET status = 'done', locked_at = NULL, last_error = NULL, updated_at = now()
          WHERE id = $1 AND locked_by = $2 AND status = 'running'",
    )
    .bind(job.id)
    .bind(worker)
    .execute(conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Lock the job row for the writer; `false` when the job is no longer ours.
pub async fn still_owned(
    conn: &mut sqlx::PgConnection,
    job: &Job,
    worker: &str,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id FROM processing_jobs
          WHERE id = $1 AND locked_by = $2 AND status = 'running' FOR UPDATE",
    )
    .bind(job.id)
    .bind(worker)
    .fetch_optional(conn)
    .await?;
    Ok(row.is_some())
}

/// What [`fail`] did with the job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failed {
    /// Back in the queue after a backoff.
    Retry,
    /// A newer queued job for the same day took it over.
    Merged,
    /// Out of attempts: `failed`.
    DeadLetter,
    /// Not ours any more; nothing changed.
    NotOwned,
}

/// Retry or dead-letter a running job owned by `owner`. `kind` is stored in
/// `last_error` and must be a short, data-free error name.
pub async fn fail(
    pool: &PgPool,
    job: &Job,
    owner: &str,
    kind: &str,
) -> Result<Failed, sqlx::Error> {
    let kind: String = kind.chars().take(128).collect();
    let backoff = backoff_secs(job.attempts) as f64;
    // A concurrent ingest can queue the same day between our statements; the
    // queued unique index then refuses the requeue, and we try again.
    for _ in 0..3 {
        let mut tx = pool.begin().await?;
        let outcome = if job.attempts >= MAX_ATTEMPTS {
            let r = sqlx::query(
                "UPDATE processing_jobs
                    SET status = 'failed', locked_at = NULL, last_error = $3, updated_at = now()
                  WHERE id = $1 AND locked_by = $2 AND status = 'running'",
            )
            .bind(job.id)
            .bind(owner)
            .bind(&kind)
            .execute(&mut *tx)
            .await?;
            if r.rows_affected() == 1 {
                Failed::DeadLetter
            } else {
                Failed::NotOwned
            }
        } else {
            let owned = sqlx::query(
                "SELECT id FROM processing_jobs
                  WHERE id = $1 AND locked_by = $2 AND status = 'running' FOR UPDATE",
            )
            .bind(job.id)
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
            if !owned {
                Failed::NotOwned
            } else {
                let sibling = sqlx::query(
                    "UPDATE processing_jobs
                        SET attempts = GREATEST(attempts, $3),
                            run_after = GREATEST(run_after, now() + make_interval(secs => $4)),
                            last_error = $5, updated_at = now()
                      WHERE user_id = $1 AND local_day = $2 AND status = 'queued'
                    RETURNING id",
                )
                .bind(job.user_id)
                .bind(job.local_day)
                .bind(job.attempts)
                .bind(backoff)
                .bind(&kind)
                .fetch_optional(&mut *tx)
                .await?;
                if sibling.is_some() {
                    sqlx::query("DELETE FROM processing_jobs WHERE id = $1")
                        .bind(job.id)
                        .execute(&mut *tx)
                        .await?;
                    Failed::Merged
                } else {
                    let r = sqlx::query(
                        "UPDATE processing_jobs
                            SET status = 'queued', run_after = now() + make_interval(secs => $2),
                                locked_at = NULL, locked_by = NULL, last_error = $3, updated_at = now()
                          WHERE id = $1",
                    )
                    .bind(job.id)
                    .bind(backoff)
                    .bind(&kind)
                    .execute(&mut *tx)
                    .await;
                    match r {
                        Ok(_) => Failed::Retry,
                        Err(e) if is_unique_violation(&e) => {
                            tx.rollback().await?;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        };
        tx.commit().await?;
        return Ok(outcome);
    }
    Ok(Failed::NotOwned)
}

/// Reclaim running jobs whose worker stopped refreshing them, and drop old
/// finished jobs. Returns how many jobs were reclaimed.
pub async fn reap(pool: &PgPool) -> Result<usize, sqlx::Error> {
    let stale = sqlx::query(
        "SELECT id, user_id, local_day, reason, attempts, locked_by FROM processing_jobs
          WHERE status = 'running' AND locked_at < now() - make_interval(secs => $1)
          ORDER BY id LIMIT 100",
    )
    .bind(STALE_AFTER_SECS as f64)
    .fetch_all(pool)
    .await?;
    let mut reclaimed = 0;
    for row in &stale {
        let job = job_from_row(row)?;
        let owner: Option<String> = row.try_get("locked_by")?;
        if fail(
            pool,
            &job,
            owner.as_deref().unwrap_or_default(),
            "stale_lock",
        )
        .await?
            != Failed::NotOwned
        {
            reclaimed += 1;
        }
    }
    sqlx::query(
        "DELETE FROM processing_jobs
          WHERE status = 'done' AND updated_at < now() - make_interval(days => $1)",
    )
    .bind(KEEP_DONE_DAYS as i32)
    .execute(pool)
    .await?;
    Ok(reclaimed)
}

/// Queue the last `days` days that have activity but no sessions from this
/// algorithm version (EN-14). Returns how many jobs were queued.
pub async fn enqueue_uncalculated(
    pool: &PgPool,
    days: i32,
    version: i32,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT app_enqueue_uncalculated_days($1, $2, 'algorithm_upgrade')")
        .bind(days)
        .bind(version)
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(
            (1..=6).map(backoff_secs).collect::<Vec<_>>(),
            [10, 20, 40, 80, 160, 320]
        );
        assert_eq!(backoff_secs(40), 1800);
    }
}
