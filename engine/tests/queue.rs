//! Queue and end-to-end tests against a real, migrated Postgres (EN-2, EN-11, EN-14).
//!
//! Needs two URLs to one database migrated by `secretary-warehouse migrate`:
//! `ENGINE_TEST_DATABASE_URL` (the owner: seeds rows, reads results) and
//! `ENGINE_TEST_APP_URL` (a login in `secretary_app`: runs the engine, so
//! RLS applies as in production). Skipped when either is unset. The cases
//! run in sequence in one test because they share `processing_jobs`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{NaiveDate, TimeDelta, Utc};
use secretary_engine::queue::{self, Failed, Job};
use secretary_engine::worker::{Calculator, JobError, Processor, Stats, run_worker};
use secretary_engine::{ALGORITHM_VERSION, load};
use sqlx::{PgPool, Row};
use tokio_util::sync::CancellationToken;

async fn pools() -> Option<(PgPool, PgPool)> {
    let owner = std::env::var("ENGINE_TEST_DATABASE_URL").ok()?;
    let app = std::env::var("ENGINE_TEST_APP_URL").ok()?;
    Some((
        PgPool::connect(&owner).await.expect("owner connect"),
        PgPool::connect(&app).await.expect("app connect"),
    ))
}

async fn user(owner: &PgPool, tz: &str) -> i64 {
    let email = format!("{}@engine.test", uuid::Uuid::now_v7());
    sqlx::query_scalar("INSERT INTO users (email, time_zone) VALUES ($1, $2) RETURNING id")
        .bind(email)
        .bind(tz)
        .fetch_one(owner)
        .await
        .unwrap()
}

async fn enqueue(owner: &PgPool, user_id: i64, day: NaiveDate) {
    sqlx::query(
        "INSERT INTO processing_jobs (user_id, local_day, reason, status, run_after, attempts)
         VALUES ($1, $2, 'test', 'queued', now(), 0)
         ON CONFLICT (user_id, local_day) WHERE status = 'queued' DO NOTHING",
    )
    .bind(user_id)
    .bind(day)
    .execute(owner)
    .await
    .unwrap();
}

async fn count(owner: &PgPool, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(owner).await.unwrap()
}

async fn count_for(owner: &PgPool, sql: &'static str, user_id: i64) -> i64 {
    sqlx::query_scalar(sql)
        .bind(user_id)
        .fetch_one(owner)
        .await
        .unwrap()
}

/// Records which (user, day) are being processed; any overlap is a violation.
struct Probe {
    owner: PgPool,
    active: Mutex<HashSet<(i64, NaiveDate)>>,
    violations: Mutex<Vec<(i64, NaiveDate)>>,
    processed: Mutex<usize>,
}

impl Processor for Probe {
    async fn process(&self, pool: &PgPool, job: &Job, worker: &str) -> Result<(), JobError> {
        let key = (job.user_id, job.local_day);
        if !self.active.lock().unwrap().insert(key) {
            self.violations.lock().unwrap().push(key);
        }
        // New input for the same day arrives while it is running.
        enqueue(&self.owner, job.user_id, job.local_day).await;
        tokio::time::sleep(Duration::from_millis(15)).await;
        self.active.lock().unwrap().remove(&key);
        let mut tx = pool.begin().await?;
        let owned = queue::complete(&mut tx, job, worker).await?;
        tx.commit().await?;
        *self.processed.lock().unwrap() += 1;
        if owned { Ok(()) } else { Err(JobError::Lost) }
    }
}

async fn reset(owner: &PgPool) {
    sqlx::query("DELETE FROM processing_jobs")
        .execute(owner)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queue_and_engine_against_postgres() {
    let Some((owner, app)) = pools().await else {
        eprintln!("skipped: set ENGINE_TEST_DATABASE_URL and ENGINE_TEST_APP_URL");
        return;
    };
    reset(&owner).await;
    no_two_workers_share_a_day(&owner, &app).await;
    reset(&owner).await;
    retry_backoff_and_dead_letter(&owner, &app).await;
    reset(&owner).await;
    failure_merges_into_newer_job(&owner, &app).await;
    reset(&owner).await;
    stale_jobs_are_reclaimed(&owner, &app).await;
    reset(&owner).await;
    calculates_a_day_end_to_end(&owner, &app).await;
    reset(&owner).await;
}

/// EN-2 acceptance: 8 workers, 3 users × 4 days, new input re-queues each day
/// while it runs; a (user, day) is never processed twice at once.
async fn no_two_workers_share_a_day(owner: &PgPool, app: &PgPool) {
    let day0: NaiveDate = "2026-11-01".parse().unwrap();
    let mut users = Vec::new();
    for _ in 0..3 {
        let u = user(owner, "UTC").await;
        for d in 0..4 {
            enqueue(owner, u, day0 + TimeDelta::days(d)).await;
        }
        users.push(u);
    }
    let probe = Arc::new(Probe {
        owner: owner.clone(),
        active: Mutex::default(),
        violations: Mutex::default(),
        processed: Mutex::default(),
    });
    let cancel = CancellationToken::new();
    let stats = Arc::new(Stats::default());
    let mut workers = tokio::task::JoinSet::new();
    for n in 0..8 {
        workers.spawn(run_worker(
            app.clone(),
            probe.clone(),
            format!("test:{n}"),
            Duration::from_millis(20),
            stats.clone(),
            cancel.clone(),
        ));
    }
    // Each job re-queues its day once; stop re-queueing after the first round.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if *probe.processed.lock().unwrap() >= 24 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "workers too slow");
    }
    cancel.cancel();
    while workers.join_next().await.is_some() {}
    assert!(
        probe.violations.lock().unwrap().is_empty(),
        "same day processed concurrently"
    );
    assert_eq!(
        count(
            owner,
            "SELECT count(*) FROM processing_jobs WHERE status = 'running'"
        )
        .await,
        0
    );
    assert!(
        count(
            owner,
            "SELECT count(*) FROM processing_jobs WHERE status = 'done'"
        )
        .await
            >= 24
    );
}

async fn claim_only(app: &PgPool) -> Job {
    queue::claim(app, "w1").await.unwrap().expect("a due job")
}

async fn retry_backoff_and_dead_letter(owner: &PgPool, app: &PgPool) {
    let u = user(owner, "UTC").await;
    enqueue(owner, u, "2026-11-02".parse().unwrap()).await;
    for attempt in 1..=queue::MAX_ATTEMPTS {
        let job = claim_only(app).await;
        assert_eq!(job.attempts, attempt);
        // Nothing else is due while it runs or backs off.
        assert!(queue::claim(app, "w2").await.unwrap().is_none());
        let outcome = queue::fail(app, &job, "w1", "load").await.unwrap();
        if attempt < queue::MAX_ATTEMPTS {
            assert_eq!(outcome, Failed::Retry);
            let delay: f64 = sqlx::query_scalar(
                "SELECT extract(epoch FROM run_after - now())::float8 FROM processing_jobs WHERE id = $1",
            )
            .bind(job.id)
            .fetch_one(owner)
            .await
            .unwrap();
            let want = queue::backoff_secs(attempt) as f64;
            assert!(
                (want - 5.0..=want + 1.0).contains(&delay),
                "backoff {delay} vs {want}"
            );
            assert!(
                queue::claim(app, "w1").await.unwrap().is_none(),
                "backing off"
            );
            sqlx::query("UPDATE processing_jobs SET run_after = now() WHERE id = $1")
                .bind(job.id)
                .execute(owner)
                .await
                .unwrap();
        } else {
            assert_eq!(outcome, Failed::DeadLetter);
        }
    }
    let row = sqlx::query("SELECT status, last_error FROM processing_jobs")
        .fetch_one(owner)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>("status"), "failed");
    assert_eq!(row.get::<String, _>("last_error"), "load");
}

async fn failure_merges_into_newer_job(owner: &PgPool, app: &PgPool) {
    let u = user(owner, "UTC").await;
    let day = "2026-11-03".parse().unwrap();
    enqueue(owner, u, day).await;
    let job = claim_only(app).await;
    enqueue(owner, u, day).await; // new input while running
    // A second worker must not take the queued sibling while the first runs.
    assert!(queue::claim(app, "w2").await.unwrap().is_none());
    assert_eq!(
        queue::fail(app, &job, "w1", "write").await.unwrap(),
        Failed::Merged
    );
    let rows = sqlx::query("SELECT status, attempts FROM processing_jobs")
        .fetch_all(owner)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String, _>("status"), "queued");
    assert_eq!(rows[0].get::<i32, _>("attempts"), 1);
    // A failure report from a worker that no longer owns a job changes nothing.
    assert_eq!(
        queue::fail(app, &job, "w1", "write").await.unwrap(),
        Failed::NotOwned
    );
}

async fn stale_jobs_are_reclaimed(owner: &PgPool, app: &PgPool) {
    let u = user(owner, "UTC").await;
    enqueue(owner, u, "2026-11-04".parse().unwrap()).await;
    let job = claim_only(app).await;
    assert!(queue::heartbeat(app, &job, "w1").await.unwrap());
    sqlx::query("UPDATE processing_jobs SET locked_at = now() - interval '10 minutes'")
        .execute(owner)
        .await
        .unwrap();
    assert_eq!(queue::reap(app).await.unwrap(), 1);
    assert!(
        !queue::heartbeat(app, &job, "w1").await.unwrap(),
        "lock lost after reaping"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM processing_jobs")
        .fetch_one(owner)
        .await
        .unwrap();
    assert_eq!(status, "queued");
}

/// Ingest-shaped rows → job → sessions, as the RLS-bound app role (EN-11, EN-14).
async fn calculates_a_day_end_to_end(owner: &PgPool, app: &PgPool) {
    let u = user(owner, "Europe/Paris").await;
    let other = user(owner, "UTC").await;
    let ws: i64 = sqlx::query_scalar(
        "INSERT INTO workspaces (kind, name, personal_owner_id) VALUES ('personal', 'me', $1) RETURNING id",
    )
    .bind(u)
    .fetch_one(owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(ws)
        .bind(u)
        .execute(owner)
        .await
        .unwrap();
    let project: i64 = sqlx::query_scalar(
        "INSERT INTO projects (workspace_id, name) VALUES ($1, 'API') RETURNING id",
    )
    .bind(ws)
    .fetch_one(owner)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_mappings (workspace_id, project_id, user_id, signal, pattern, priority)
         VALUES ($1, $2, $3, 'path', '/home/dev/api', 0)",
    )
    .bind(ws)
    .bind(project)
    .bind(u)
    .execute(owner)
    .await
    .unwrap();
    let mut devices = Vec::new();
    for uid in [u, other] {
        let d: i64 = sqlx::query_scalar(
            "INSERT INTO devices (user_id, name, os, auth_session_id) VALUES ($1, 'laptop', 'linux', $2) RETURNING id",
        )
        .bind(uid)
        .bind(uuid::Uuid::now_v7().to_string())
        .fetch_one(owner)
        .await
        .unwrap();
        devices.push(d);
    }

    // Three days ago at 10:00 Paris time (inside the partitions and the 30-day window).
    let day = (Utc::now() - TimeDelta::days(3)).date_naive();
    let start =
        secretary_engine::model::day_bounds(secretary_engine::model::parse_tz("Europe/Paris"), day)
            .start
            + TimeDelta::hours(10);
    let insert = |uid: i64,
                  dev: i64,
                  kind: &'static str,
                  s: chrono::DateTime<Utc>,
                  e: chrono::DateTime<Utc>,
                  root: Option<&'static str>,
                  md: serde_json::Value| {
        let owner = owner.clone();
        async move {
            sqlx::query(
                "INSERT INTO activity_events (event_id, user_id, device_id, batch_id, schema_version, event_type,
                     source, timestamp_start, timestamp_end, server_adjusted_start, server_adjusted_end,
                     repository_root, metadata)
                 VALUES ($1, $2, $3, $4, 1, $5, 'agent@0.1.0', $6, $7, $6, $7, $8, $9)",
            )
            .bind(uuid::Uuid::now_v7())
            .bind(uid)
            .bind(dev)
            .bind(uuid::Uuid::new_v4())
            .bind(kind)
            .bind(s)
            .bind(e)
            .bind(root)
            .bind(md)
            .execute(&owner)
            .await
            .unwrap();
        }
    };
    let h = |m: i64| start + TimeDelta::minutes(m);
    insert(
        u,
        devices[0],
        "presence.active",
        h(0),
        h(30),
        None,
        serde_json::json!({}),
    )
    .await;
    insert(
        u,
        devices[0],
        "editor.file_focus",
        h(0),
        h(25),
        Some("/home/dev/api"),
        serde_json::json!({"file_path": "src/main.rs"}),
    )
    .await;
    insert(
        u,
        devices[0],
        "window.focus",
        h(25),
        h(30),
        None,
        serde_json::json!({"app": "slack.exe", "title": "general"}),
    )
    .await;
    // Another user's activity at the same time must never be read.
    insert(
        other,
        devices[1],
        "presence.active",
        h(0),
        h(30),
        None,
        serde_json::json!({}),
    )
    .await;
    insert(
        other,
        devices[1],
        "editor.file_focus",
        h(0),
        h(30),
        Some("/home/dev/api"),
        serde_json::json!({"file_path": "src/main.rs"}),
    )
    .await;

    // EN-14: the uncalculated day (for both users) is found.
    assert!(
        queue::enqueue_uncalculated(app, 30, ALGORITHM_VERSION)
            .await
            .unwrap()
            >= 1
    );
    sqlx::query("DELETE FROM processing_jobs WHERE user_id <> $1")
        .bind(u)
        .execute(owner)
        .await
        .unwrap();
    let local_day: NaiveDate =
        sqlx::query_scalar("SELECT local_day FROM processing_jobs WHERE user_id = $1")
            .bind(u)
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(local_day, day);

    let snapshot = || async {
        sqlx::query(
            "SELECT s.project_id, s.kind, s.starts_at, s.ends_at, s.algorithm_version, s.config_hash,
                    (SELECT count(*) FROM session_evidence e WHERE e.session_id = s.id) AS evidence
               FROM work_sessions s WHERE s.user_id = $1 ORDER BY s.starts_at",
        )
        .bind(u)
        .fetch_all(owner)
        .await
        .unwrap()
        .iter()
        .map(|r| {
            format!(
                "{:?}|{}|{}|{}|{}|{}|{}",
                r.get::<Option<i64>, _>("project_id"),
                r.get::<String, _>("kind"),
                r.get::<chrono::DateTime<Utc>, _>("starts_at").to_rfc3339(),
                r.get::<chrono::DateTime<Utc>, _>("ends_at").to_rfc3339(),
                r.get::<i32, _>("algorithm_version"),
                r.get::<String, _>("config_hash"),
                r.get::<i64, _>("evidence"),
            )
        })
        .collect::<Vec<_>>()
    };

    // EN-11: a re-run replaces the day with identical rows.
    let calc = Calculator;
    let mut runs = Vec::new();
    for round in 0..2 {
        if round == 1 {
            enqueue(owner, u, day).await;
        }
        let job = claim_only(app).await;
        calc.process(app, &job, "w1").await.expect("calculated");
        runs.push(snapshot().await);
    }
    assert_eq!(runs[0], runs[1], "re-run changed the output");
    let rows = snapshot().await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(rows[0].starts_with(&format!(
        "Some({project})|active|{}|{}|{ALGORITHM_VERSION}|",
        h(0).to_rfc3339(),
        h(25).to_rfc3339()
    )));
    assert!(rows[1].starts_with(&format!(
        "None|active|{}|{}|{ALGORITHM_VERSION}|",
        h(25).to_rfc3339(),
        h(30).to_rfc3339()
    )));
    assert_eq!(
        count(
            owner,
            "SELECT count(*) FROM processing_jobs WHERE status = 'done'"
        )
        .await,
        2
    );
    assert_eq!(
        count_for(
            owner,
            "SELECT count(*) FROM event_assignments WHERE user_id = $1",
            u
        )
        .await,
        1
    );
    // Nothing was written for the other user, and the day now counts as calculated.
    assert_eq!(
        count_for(
            owner,
            "SELECT count(*) FROM work_sessions WHERE user_id = $1",
            other
        )
        .await,
        0
    );
    sqlx::query("DELETE FROM processing_jobs")
        .execute(owner)
        .await
        .unwrap();
    queue::enqueue_uncalculated(app, 30, ALGORITHM_VERSION)
        .await
        .unwrap();
    assert_eq!(
        count_for(
            owner,
            "SELECT count(*) FROM processing_jobs WHERE user_id = $1",
            u
        )
        .await,
        0
    );
    // EN-14: a newer algorithm version recalculates the day.
    queue::enqueue_uncalculated(app, 30, ALGORITHM_VERSION + 1)
        .await
        .unwrap();
    assert_eq!(
        count_for(
            owner,
            "SELECT count(*) FROM processing_jobs WHERE user_id = $1 AND reason = 'algorithm_upgrade'",
            u
        )
        .await,
        1
    );
    sqlx::query("DELETE FROM processing_jobs")
        .execute(owner)
        .await
        .unwrap();

    // The loader under RLS sees only this user's events.
    let mut tx = app.begin().await.unwrap();
    load::set_user(&mut tx, u).await.unwrap();
    let (ctx, stats) = load::load(&mut tx, u, day).await.unwrap();
    assert_eq!((stats.events, stats.skipped, ctx.rules.len()), (3, 0, 1));
    assert_eq!(ctx.time_zone, "Europe/Paris");
    tx.rollback().await.unwrap();

    // EN-10: the unassigned tail moved to the project survives every
    // recalculation; an undone correction no longer applies.
    let adjustment: i64 = sqlx::query_scalar(
        "INSERT INTO time_adjustments (user_id, local_day, kind, starts_at, ends_at, project_id)
         VALUES ($1, $2, 'reassign', $3, $4, $5) RETURNING id",
    )
    .bind(u)
    .bind(day)
    .bind(h(25))
    .bind(h(30))
    .bind(project)
    .fetch_one(owner)
    .await
    .unwrap();
    for _ in 0..2 {
        enqueue(owner, u, day).await;
        let job = claim_only(app).await;
        calc.process(app, &job, "w1").await.expect("calculated");
        let rows = snapshot().await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].starts_with(&format!(
            "Some({project})|active|{}|{}|{ALGORITHM_VERSION}|",
            h(0).to_rfc3339(),
            h(30).to_rfc3339()
        )));
    }
    sqlx::query("UPDATE time_adjustments SET deleted_at = now() WHERE id = $1")
        .bind(adjustment)
        .execute(owner)
        .await
        .unwrap();
    enqueue(owner, u, day).await;
    let job = claim_only(app).await;
    calc.process(app, &job, "w1").await.expect("calculated");
    assert_eq!(snapshot().await.len(), 2, "undone correction still applied");
}
