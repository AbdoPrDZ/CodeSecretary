-- 0003 raw activity events: immutable, insert-only, partitioned by month.
--
-- Partition key is the device-reported `timestamp_start`, not
-- `server_adjusted_start`: Postgres requires unique keys on partitioned
-- tables to include the partition key, and the adjusted time can change
-- between retries of the same event (new clock offset). `timestamp_start`
-- is fixed per event, so (event_id, timestamp_start) deduplicates resends.

CREATE TABLE activity_events (
    event_id              uuid NOT NULL,          -- UUIDv7 from the collector
    user_id               uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,   -- from the device token, never the payload
    device_id             uuid NOT NULL REFERENCES devices (id) ON DELETE CASCADE, -- from the device token
    batch_id              uuid NOT NULL,          -- upload batch; key of the raw archive object
    schema_version        smallint NOT NULL,
    event_type            text NOT NULL,
    source                text NOT NULL,          -- e.g. 'vscode@0.3.1'
    timestamp_start       timestamptz NOT NULL,
    timestamp_end         timestamptz,
    server_adjusted_start timestamptz NOT NULL,
    server_adjusted_end   timestamptz,
    received_at           timestamptz NOT NULL DEFAULT now(),
    project_hint          uuid,
    repository_root       text,
    repository_remote     text,                   -- normalized, e.g. 'github.com/acme/api'
    working_directory     text,
    metadata              jsonb NOT NULL DEFAULT '{}'::jsonb,  -- already redacted by the agent
    PRIMARY KEY (event_id, timestamp_start),
    CONSTRAINT activity_events_end_after_start CHECK (timestamp_end IS NULL OR timestamp_end >= timestamp_start),
    CONSTRAINT activity_events_adjusted_end_after_start
        CHECK (server_adjusted_end IS NULL OR server_adjusted_end >= server_adjusted_start)
) PARTITION BY RANGE (timestamp_start);

CREATE INDEX activity_events_user_time_idx ON activity_events (user_id, server_adjusted_start);
CREATE INDEX activity_events_device_idx ON activity_events (device_id);

ALTER TABLE activity_events ENABLE ROW LEVEL SECURITY;
CREATE POLICY activity_events_select ON activity_events FOR SELECT
    USING (user_id = app_current_user_id());
CREATE POLICY activity_events_insert ON activity_events FOR INSERT
    WITH CHECK (user_id = app_current_user_id());
-- Insert-only: no UPDATE/DELETE for the app role. Deletion only via the
-- privacy-delete procedure (WH-12), which runs as the owner.
GRANT SELECT, INSERT ON activity_events TO secretary_app;

-- Creates missing monthly partitions from `months_back` before the current
-- UTC month to `months_ahead` after it. Returns how many were created.
-- Called here and by partition maintenance (WH-11). No default partition:
-- events outside the range are rejected (validation limits timestamps to
-- ≤ 5 min in the future and the retention window in the past).
CREATE FUNCTION app_ensure_event_partitions(months_back integer, months_ahead integer)
    RETURNS integer
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp
AS $$
DECLARE
    month_start timestamp;
    partition   text;
    created     integer := 0;
BEGIN
    FOR i IN -months_back..months_ahead LOOP
        month_start := date_trunc('month', now() AT TIME ZONE 'UTC') + make_interval(months => i);
        partition := format('activity_events_%s', to_char(month_start, 'YYYY_MM'));
        IF to_regclass(partition) IS NULL THEN
            EXECUTE format(
                'CREATE TABLE %I PARTITION OF activity_events FOR VALUES FROM (%L) TO (%L)',
                partition,
                month_start AT TIME ZONE 'UTC',
                (month_start + interval '1 month') AT TIME ZONE 'UTC'
            );
            created := created + 1;
        END IF;
    END LOOP;
    RETURN created;
END
$$;

REVOKE ALL ON FUNCTION app_ensure_event_partitions(integer, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app_ensure_event_partitions(integer, integer) TO secretary_app;

-- 12 months back (retention window, Git backfill, buffered uploads) + 3 ahead.
SELECT app_ensure_event_partitions(12, 3);
