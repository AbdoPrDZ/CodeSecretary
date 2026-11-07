-- 0007 engine job queue (Postgres SKIP LOCKED; no extra infrastructure).
--
-- System table, deliberately WITHOUT RLS: the engine claims jobs across all
-- users before it knows whose day it is processing. It holds only
-- (user_id, local_day, reason), never activity data, and is not exposed by
-- any user-facing endpoint. After claiming a job the engine sets
-- app.user_id to the job's user for all data access.

CREATE TYPE job_status AS ENUM ('queued', 'running', 'done', 'failed');

CREATE TABLE processing_jobs (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    local_day  date NOT NULL,
    reason     text NOT NULL,       -- 'ingest', 'rules_changed', 'correction', 'algorithm_upgrade', …
    status     job_status NOT NULL DEFAULT 'queued',
    run_after  timestamptz NOT NULL DEFAULT now(),  -- debounce: ingest pushes this ~2 min out
    attempts   integer NOT NULL DEFAULT 0,
    locked_at  timestamptz,
    locked_by  text,
    last_error text,                -- error kind only; never event metadata
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- At most one queued job per (user, day): enqueue is an upsert on this index.
CREATE UNIQUE INDEX processing_jobs_queued_key ON processing_jobs (user_id, local_day) WHERE status = 'queued';
CREATE INDEX processing_jobs_ready_idx ON processing_jobs (run_after) WHERE status = 'queued';

GRANT SELECT, INSERT, UPDATE, DELETE ON processing_jobs TO secretary_app;
