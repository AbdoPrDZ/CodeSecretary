//! Row-level security and schema tests (WH-2, WH-3).
//!
//! Each test gets a fresh database with all migrations applied (`sqlx::test`).
//! Fixtures are seeded as the superuser; assertions run through `Db` connected
//! as `secretary_test_app`, a login role in `secretary_app` — RLS applies
//! exactly as it does for the real services.
//!
//! Requires `DATABASE_URL` pointing at a Postgres superuser, e.g. the
//! docker-compose one: `postgres://secretary:secretary@localhost:5432/secretary`.

use secretary_warehouse::db::Db;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

/// Tables intentionally without RLS, with the reason.
const NO_RLS: &[&str] = &[
    "_sqlx_migrations", // migration bookkeeping, not granted to the app role
    "processing_jobs",  // system queue; see 0007_jobs.sql
];

// ---------------------------------------------------------------------------
// harness

async fn setup(connect: PgConnectOptions) -> (PgPool, Db) {
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(connect.clone())
        .await
        .expect("superuser connection");
    sqlx::query(
        "DO $$ BEGIN
             CREATE ROLE secretary_test_app LOGIN NOBYPASSRLS PASSWORD 'test' IN ROLE secretary_app;
         EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
         END $$",
    )
    .execute(&admin)
    .await
    .expect("test app role");
    let app = PgPoolOptions::new()
        .max_connections(1) // one connection: proves context never leaks between transactions
        .connect_with(connect.username("secretary_test_app").password("test"))
        .await
        .expect("app role connection");
    (admin, Db::from_pool(app))
}

fn is_rls_violation(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(e) if e.code().as_deref() == Some("42501"))
}

async fn count(tx: &mut Transaction<'static, Postgres>, table: &str) -> i64 {
    // Audited: `table` comes from pg_catalog / test constants, never user input.
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM \"{table}\""
    )))
    .fetch_one(&mut **tx)
    .await
    .unwrap_or_else(|e| panic!("count {table}: {e}"))
}

struct World {
    a: Uuid,
    b: Uuid,
    a_personal_ws: Uuid,
    a_personal_project: Uuid,
    team_ws: Uuid,
    team_project: Uuid,
}

async fn create_user(admin: &PgPool, email: &str) -> (Uuid, Uuid, Uuid) {
    let user: Uuid = sqlx::query_scalar("INSERT INTO users (email) VALUES ($1) RETURNING id")
        .bind(email)
        .fetch_one(admin)
        .await
        .unwrap();
    let ws: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (kind, name, personal_owner_id) VALUES ('personal', 'Personal', $1) RETURNING id",
    )
    .bind(user)
    .fetch_one(admin)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1, $2, 'owner')")
        .bind(ws)
        .bind(user)
        .execute(admin)
        .await
        .unwrap();
    let device: Uuid = sqlx::query_scalar(
        "INSERT INTO devices (user_id, name, os, token_hash, token_prefix)
         VALUES ($1, 'laptop', 'linux', gen_random_uuid()::text::bytea, 'tok_') RETURNING id",
    )
    .bind(user)
    .fetch_one(admin)
    .await
    .unwrap();
    (user, ws, device)
}

async fn insert_event(
    exec: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
    user: Uuid,
    device: Uuid,
) -> Result<u64, sqlx::Error> {
    sqlx::query(
        "INSERT INTO activity_events
             (event_id, user_id, device_id, batch_id, schema_version, event_type, source,
              timestamp_start, server_adjusted_start)
         VALUES ($1, $2, $3, gen_random_uuid(), 1, 'presence.active', 'agent@0.1.0', now(), now())
         ON CONFLICT DO NOTHING",
    )
    .bind(event_id)
    .bind(user)
    .bind(device)
    .execute(exec)
    .await
    .map(|r| r.rows_affected())
}

/// Users A and B, each with a personal workspace, device, and 2 events.
/// A owns team workspace T (B is a member) with one shared project.
/// A has a personal project, a personal rule and a shared rule on T's project.
/// B has a draft and a submitted submission for T's project.
async fn seed(admin: &PgPool) -> World {
    let (a, a_ws, a_dev) = create_user(admin, "a@example.test").await;
    let (b, _b_ws, b_dev) = create_user(admin, "b@example.test").await;
    for (user, device) in [(a, a_dev), (a, a_dev), (b, b_dev), (b, b_dev)] {
        insert_event(admin, Uuid::now_v7(), user, device)
            .await
            .unwrap();
    }

    let team_ws: Uuid = sqlx::query_scalar(
        "INSERT INTO workspaces (kind, name) VALUES ('team', 'Acme') RETURNING id",
    )
    .fetch_one(admin)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (workspace_id, user_id, role) VALUES ($1, $2, 'owner'), ($1, $3, 'member')",
    )
    .bind(team_ws)
    .bind(a)
    .bind(b)
    .execute(admin)
    .await
    .unwrap();

    let project = |ws: Uuid, name: &'static str| {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (workspace_id, name) VALUES ($1, $2) RETURNING id",
        )
        .bind(ws)
        .bind(name)
        .fetch_one(admin)
    };
    let a_personal_project = project(a_ws, "Side project").await.unwrap();
    let team_project = project(team_ws, "Client Acme").await.unwrap();

    sqlx::query(
        "INSERT INTO project_mappings (workspace_id, project_id, user_id, signal, pattern)
         VALUES ($1, $2, $3, 'path', '/home/a/acme/*'), ($1, $2, NULL, 'remote', 'github.com/acme/*')",
    )
    .bind(team_ws)
    .bind(team_project)
    .bind(a)
    .execute(admin)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO submissions (workspace_id, project_id, user_id, period_start, period_end, total_seconds, status)
         VALUES ($1, $2, $3, '2026-11-02', '2026-11-08', 3600, 'draft'),
                ($1, $2, $3, '2026-10-26', '2026-11-01', 7200, 'submitted')",
    )
    .bind(team_ws)
    .bind(team_project)
    .bind(b)
    .execute(admin)
    .await
    .unwrap();

    World {
        a,
        b,
        a_personal_ws: a_ws,
        a_personal_project,
        team_ws,
        team_project,
    }
}

async fn tenant_tables(admin: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND NOT c.relispartition
         ORDER BY 1",
    )
    .fetch_all(admin)
    .await
    .unwrap()
    .into_iter()
    .filter(|t: &String| !NO_RLS.contains(&t.as_str()))
    .collect()
}

// ---------------------------------------------------------------------------
// schema-wide guarantees

#[sqlx::test(migrations = "./migrations")]
async fn every_tenant_table_has_rls_and_a_policy(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, _db) = setup(connect).await;
    let tables = tenant_tables(&admin).await;
    assert!(tables.len() >= 20, "expected all tables, got {tables:?}");
    for table in tables {
        let row = sqlx::query(
            "SELECT c.relrowsecurity, (SELECT count(*) FROM pg_policies p WHERE p.tablename = c.relname)
             FROM pg_class c WHERE c.relname = $1 AND c.relnamespace = 'public'::regnamespace",
        )
        .bind(&table)
        .fetch_one(&admin)
        .await
        .unwrap();
        let (rls, policies): (bool, i64) = (row.get(0), row.get(1));
        assert!(rls, "{table}: RLS not enabled");
        assert!(policies > 0, "{table}: no policy");
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn without_context_every_tenant_table_is_empty(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    seed(&admin).await;
    let mut tx = db.begin_unscoped().await.unwrap();
    for table in tenant_tables(&admin).await {
        if table == "_sqlx_migrations" {
            continue;
        }
        assert_eq!(
            count(&mut tx, &table).await,
            0,
            "{table} visible without context"
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn context_does_not_leak_to_the_next_transaction(
    _: PgPoolOptions,
    connect: PgConnectOptions,
) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    let mut tx = db.begin_for_user(world.a).await.unwrap();
    assert_eq!(count(&mut tx, "activity_events").await, 2);
    tx.commit().await.unwrap();

    // Same single pooled connection, new transaction, no context.
    let mut tx = db.begin_unscoped().await.unwrap();
    assert_eq!(count(&mut tx, "activity_events").await, 0);
}

// ---------------------------------------------------------------------------
// user-scoped data

#[sqlx::test(migrations = "./migrations")]
async fn users_see_only_their_own_rows(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    for user in [world.a, world.b] {
        let mut tx = db.begin_for_user(user).await.unwrap();
        assert_eq!(count(&mut tx, "users").await, 1);
        assert_eq!(count(&mut tx, "devices").await, 1);
        assert_eq!(count(&mut tx, "activity_events").await, 2);
        let others: i64 =
            sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE user_id <> $1")
                .bind(user)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(others, 0);
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn cannot_write_rows_for_another_user(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;
    let b_device: Uuid = sqlx::query_scalar("SELECT id FROM devices WHERE user_id = $1")
        .bind(world.b)
        .fetch_one(&admin)
        .await
        .unwrap();

    let mut tx = db.begin_for_user(world.a).await.unwrap();
    let err = insert_event(&mut *tx, Uuid::now_v7(), world.b, b_device)
        .await
        .unwrap_err();
    assert!(is_rls_violation(&err), "{err}");
}

#[sqlx::test(migrations = "./migrations")]
async fn activity_events_are_insert_only(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    for sql in [
        "UPDATE activity_events SET event_type = 'x'",
        "DELETE FROM activity_events",
    ] {
        let mut tx = db.begin_for_user(world.a).await.unwrap();
        let err = sqlx::query(sql).execute(&mut *tx).await.unwrap_err();
        assert!(is_rls_violation(&err), "{sql}: {err}"); // 42501 = insufficient_privilege
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn resent_events_are_deduplicated(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;
    let device: Uuid = sqlx::query_scalar("SELECT id FROM devices WHERE user_id = $1")
        .bind(world.a)
        .fetch_one(&admin)
        .await
        .unwrap();

    let event_id = Uuid::now_v7();
    let mut tx = db.begin_for_user(world.a).await.unwrap();
    let fixed_time = "2026-11-06T10:00:00Z";
    for expected in [1u64, 0] {
        let affected = sqlx::query(
            "INSERT INTO activity_events
                 (event_id, user_id, device_id, batch_id, schema_version, event_type, source,
                  timestamp_start, server_adjusted_start)
             VALUES ($1, $2, $3, gen_random_uuid(), 1, 'presence.active', 'agent@0.1.0',
                     $4::timestamptz, $4::timestamptz + interval '3 seconds')
             ON CONFLICT DO NOTHING",
        )
        .bind(event_id)
        .bind(world.a)
        .bind(device)
        .bind(fixed_time)
        .execute(&mut *tx)
        .await
        .unwrap()
        .rows_affected();
        assert_eq!(affected, expected);
    }
}

// ---------------------------------------------------------------------------
// workspaces, projects, rules

#[sqlx::test(migrations = "./migrations")]
async fn workspace_data_is_visible_to_members_only(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    // A: own personal workspace + team.
    let mut tx = db.begin_for_user(world.a).await.unwrap();
    assert_eq!(count(&mut tx, "workspaces").await, 2);
    assert_eq!(count(&mut tx, "projects").await, 2);
    tx.rollback().await.unwrap();

    // B: own personal workspace + team; never A's personal project.
    let mut tx = db.begin_for_user(world.b).await.unwrap();
    let projects: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM projects")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(projects, vec![world.team_project]);
    let sees_a_personal: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE id = $1")
        .bind(world.a_personal_ws)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(sees_a_personal, 0);
    assert_ne!(world.a_personal_project, world.team_project);
}

#[sqlx::test(migrations = "./migrations")]
async fn only_admins_manage_shared_projects(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    let mut tx = db.begin_for_user(world.b).await.unwrap();
    let err = sqlx::query("INSERT INTO projects (workspace_id, name) VALUES ($1, 'Rogue')")
        .bind(world.team_ws)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(is_rls_violation(&err), "{err}");
    tx.rollback().await.unwrap();

    let mut tx = db.begin_for_user(world.a).await.unwrap();
    sqlx::query("INSERT INTO projects (workspace_id, name) VALUES ($1, 'Second client')")
        .bind(world.team_ws)
        .execute(&mut *tx)
        .await
        .expect("owner can add shared projects");
}

#[sqlx::test(migrations = "./migrations")]
async fn personal_rules_stay_private_shared_rules_are_visible(
    _: PgPoolOptions,
    connect: PgConnectOptions,
) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    let mut tx = db.begin_for_user(world.a).await.unwrap();
    assert_eq!(count(&mut tx, "project_mappings").await, 2);
    tx.rollback().await.unwrap();

    let mut tx = db.begin_for_user(world.b).await.unwrap();
    let visible: Vec<Option<Uuid>> = sqlx::query_scalar("SELECT user_id FROM project_mappings")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(visible, vec![None], "B sees only the shared rule");
}

// ---------------------------------------------------------------------------
// private until submitted

#[sqlx::test(migrations = "./migrations")]
async fn drafts_are_private_submitted_totals_reach_admins(
    _: PgPoolOptions,
    connect: PgConnectOptions,
) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    // B (owner of the submissions) sees both.
    let mut tx = db.begin_for_user(world.b).await.unwrap();
    assert_eq!(count(&mut tx, "submissions").await, 2);
    tx.rollback().await.unwrap();

    // A (team owner) sees only the submitted one.
    let mut tx = db.begin_for_user(world.a).await.unwrap();
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status::text FROM submissions")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(statuses, vec!["submitted".to_owned()]);
    // ...and still nothing of B's raw activity or devices.
    let b_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM activity_events WHERE user_id = $1")
            .bind(world.b)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(b_events, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn members_cannot_approve_their_own_submissions(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    let mut tx = db.begin_for_user(world.b).await.unwrap();
    let err = sqlx::query("UPDATE submissions SET status = 'approved' WHERE status = 'submitted'")
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert!(is_rls_violation(&err), "{err}");
    tx.rollback().await.unwrap();

    let mut tx = db.begin_for_user(world.a).await.unwrap();
    let approved =
        sqlx::query("UPDATE submissions SET status = 'approved' WHERE status = 'submitted'")
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected();
    assert_eq!(approved, 1, "team owner approves");
}

// ---------------------------------------------------------------------------
// partitions and jobs

#[sqlx::test(migrations = "./migrations")]
async fn event_partitions_cover_retention_and_three_months_ahead(
    _: PgPoolOptions,
    connect: PgConnectOptions,
) {
    let (admin, db) = setup(connect).await;
    let missing: Vec<String> = sqlx::query_scalar(
        "SELECT to_char(m, 'YYYY_MM') FROM generate_series(
             date_trunc('month', now() AT TIME ZONE 'UTC') - interval '12 months',
             date_trunc('month', now() AT TIME ZONE 'UTC') + interval '3 months',
             interval '1 month') AS m
         WHERE to_regclass('activity_events_' || to_char(m, 'YYYY_MM')) IS NULL",
    )
    .fetch_all(&admin)
    .await
    .unwrap();
    assert!(missing.is_empty(), "missing partitions: {missing:?}");

    // Maintenance is idempotent and callable by the app role.
    let mut tx = db.begin_unscoped().await.unwrap();
    let created: i32 = sqlx::query_scalar("SELECT app_ensure_event_partitions(12, 3)")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(created, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn one_queued_job_per_user_day(_: PgPoolOptions, connect: PgConnectOptions) {
    let (admin, db) = setup(connect).await;
    let world = seed(&admin).await;

    let mut tx = db.begin_unscoped().await.unwrap();
    for _ in 0..3 {
        sqlx::query(
            "INSERT INTO processing_jobs (user_id, local_day, reason, run_after)
             VALUES ($1, '2026-11-06', 'ingest', now() + interval '2 minutes')
             ON CONFLICT (user_id, local_day) WHERE status = 'queued'
             DO UPDATE SET run_after = EXCLUDED.run_after, updated_at = now()",
        )
        .bind(world.a)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    assert_eq!(count(&mut tx, "processing_jobs").await, 1);
}
