//! Database access with per-transaction tenant context (row-level security).
//!
//! All tenant data is protected by Postgres RLS policies that compare rows
//! against `app.user_id`. This module is the only place that sets it, and
//! only with `SET LOCAL` semantics (`set_config(..., true)`): the value
//! disappears when the transaction ends, so a pooled connection can never
//! carry one user's context into another request.
//!
//! - [`Db::begin_for_user`] — the normal path: everything the transaction
//!   reads or writes is limited to that user's rows.
//! - [`Db::begin_unscoped`] — no tenant. Tenant tables return nothing; use it
//!   only for system tables (`processing_jobs`) and SECURITY DEFINER entry
//!   points (e.g. token lookup) that run before the user is known.
//!
//! Never call `set_config('app.user_id', …, false)` (session level) anywhere.

use std::time::Duration;

use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Every migration for every service, embedded at compile time.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    /// Creates a pool without connecting yet, so the service can boot (and
    /// report not-ready) while the database is still starting.
    pub fn connect_lazy(url: &str, max_connections: u32) -> Result<Self, sqlx::Error> {
        let options: PgConnectOptions = url.parse()?;
        Ok(Self::from_pool(
            pool_options(max_connections).connect_lazy_with(options),
        ))
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Starts a transaction scoped to `user_id`.
    pub async fn begin_for_user(
        &self,
        user_id: Uuid,
    ) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    /// Starts a transaction with no tenant context (see module docs).
    pub async fn begin_unscoped(&self) -> Result<Transaction<'static, Postgres>, sqlx::Error> {
        self.pool.begin().await
    }

    /// Readiness check.
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map(|_| ())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

fn pool_options(max_connections: u32) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
}

/// Applies pending migrations using an owner connection (not the app role).
pub async fn migrate(owner_url: &str) -> Result<(), MigrateError> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect(owner_url)
        .await?;
    let result = MIGRATOR.run(&pool).await;
    pool.close().await;
    result
}
