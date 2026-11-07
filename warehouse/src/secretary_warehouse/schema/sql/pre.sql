-- Runs before the ORM sync, as the schema owner, inside the migration
-- transaction. Must be idempotent: it runs on every migration.
--
-- Only what the ORM cannot create belongs here: roles and the partitioned
-- shell of activity_events (the ORM adds and checks its other columns).

DO $$
BEGIN
    CREATE ROLE secretary_app NOLOGIN NOBYPASSRLS;
EXCEPTION
    WHEN duplicate_object THEN NULL;  -- roles are cluster-wide; may already exist
END
$$;

GRANT USAGE ON SCHEMA public TO secretary_app;

-- Partitioned by the device timestamp; the primary key must include it.
CREATE TABLE IF NOT EXISTS activity_events (
    id              bigserial,
    timestamp_start timestamptz NOT NULL,
    PRIMARY KEY (id, timestamp_start)
) PARTITION BY RANGE (timestamp_start);
