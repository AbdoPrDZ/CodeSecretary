-- Runs after the ORM sync (tables, columns, indexes, constraints), as the
-- schema owner, inside the migration transaction. Must be idempotent.
--
-- Security model:
-- * Services connect as login roles that are members of `secretary_app`
--   (NOLOGIN, NOBYPASSRLS). They never own tables, so RLS always applies.
-- * Each transaction sets its tenant with `set_config('app.user_id', <id>, true)`.
--   Without it, `app_current_user_id()` is NULL and every policy matches nothing.
-- * Helpers that read across RLS are SECURITY DEFINER, owned by the schema
--   owner, and only return facts about the current user.
-- * Index names here end in `_ix` / `_ux`: the ORM manages (and drops
--   undeclared) indexes named `<table>_<column>_idx`.

-- ---------------------------------------------------------------------------
-- context and membership helpers

CREATE OR REPLACE FUNCTION app_current_user_id() RETURNS bigint
    LANGUAGE sql STABLE
AS $$
    -- '' happens on pooled connections after a SET LOCAL ends; treat as unset.
    SELECT NULLIF(current_setting('app.user_id', true), '')::bigint
$$;

CREATE OR REPLACE FUNCTION app_workspace_role(ws bigint) RETURNS text
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT role FROM memberships WHERE workspace_id = ws AND user_id = app_current_user_id()
$$;

CREATE OR REPLACE FUNCTION app_is_member(ws bigint) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT app_workspace_role(ws) IS NOT NULL
$$;

CREATE OR REPLACE FUNCTION app_is_admin(ws bigint) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT coalesce(app_workspace_role(ws) IN ('owner', 'admin'), false)
$$;

CREATE OR REPLACE FUNCTION app_workspace_has_members(ws bigint) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (SELECT 1 FROM memberships WHERE workspace_id = ws)
$$;

-- May the current user see other members' non-draft submissions in `ws`?
CREATE OR REPLACE FUNCTION app_can_view_team_submissions(ws bigint) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT CASE app_workspace_role(ws)
        WHEN 'owner'  THEN true
        WHEN 'admin'  THEN true
        WHEN 'viewer' THEN true
        WHEN 'member' THEN (SELECT members_see_submissions FROM workspaces WHERE id = ws)
        ELSE false
    END
$$;

-- Sign-in (D14): Keycloak issuer + subject -> user id. The first sign-in
-- creates the user, the identity, a personal workspace and its owner
-- membership. Runs before a user context exists, hence SECURITY DEFINER.
-- Callers must have validated the Keycloak token first.
CREATE OR REPLACE FUNCTION app_resolve_identity(
    p_issuer text, p_subject text, p_email text, p_display_name text
) RETURNS bigint
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp
AS $$
DECLARE
    uid bigint;
    ws  bigint;
BEGIN
    -- Serialize concurrent first sign-ins of the same account.
    PERFORM pg_advisory_xact_lock(hashtextextended(p_issuer || ' ' || p_subject, 0));

    SELECT user_id INTO uid FROM user_identities WHERE provider = p_issuer AND subject = p_subject;
    IF uid IS NOT NULL THEN
        RETURN uid;
    END IF;

    INSERT INTO users (email, display_name) VALUES (p_email, p_display_name) RETURNING id INTO uid;
    INSERT INTO user_identities (provider, subject, user_id) VALUES (p_issuer, p_subject, uid);
    INSERT INTO workspaces (kind, name, personal_owner_id) VALUES ('personal', 'Personal', uid)
        RETURNING id INTO ws;
    INSERT INTO memberships (workspace_id, user_id, role) VALUES (ws, uid, 'owner');
    RETURN uid;
END
$$;

-- Dashboard cookie -> user (OV-2). Only an unexpired session's user id; the
-- token itself is never stored, only its SHA-256 (hex).
CREATE OR REPLACE FUNCTION app_web_session_user(p_token_hash text) RETURNS bigint
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT user_id FROM web_sessions WHERE token_hash = p_token_hash AND expires_at > now()
$$;

-- Creates missing monthly partitions from `months_back` before the current
-- UTC month to `months_ahead` after it. Returns how many were created.
-- No default partition: events outside the range are rejected (validation
-- limits timestamps to <= 5 min in the future and the retention window).
CREATE OR REPLACE FUNCTION app_ensure_event_partitions(months_back integer, months_ahead integer)
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

-- Engine startup (EN-14): queue every (user, local day) of the last `p_days`
-- that has activity but no sessions from algorithm `p_version` or later, i.e.
-- days never calculated and days calculated by an older algorithm. The engine
-- cannot list users or events itself (RLS), and this returns only a count.
CREATE OR REPLACE FUNCTION app_enqueue_uncalculated_days(p_days integer, p_version integer, p_reason text)
    RETURNS integer
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp
AS $$
DECLARE
    queued integer;
BEGIN
    WITH days AS (
        SELECT DISTINCT e.user_id,
               (e.server_adjusted_start AT TIME ZONE u.time_zone)::date AS local_day
        FROM activity_events e
        JOIN users u ON u.id = e.user_id
        WHERE e.server_adjusted_start >= now() - make_interval(days => p_days)
          -- partition pruning (device time is within minutes of adjusted time)
          AND e.timestamp_start >= now() - make_interval(days => p_days + 1)
          AND e.event_type NOT IN ('agent.heartbeat', 'agent.buffer_overflow')
    )
    INSERT INTO processing_jobs (user_id, local_day, reason, status, run_after, attempts)
    SELECT d.user_id, d.local_day, p_reason, 'queued', now(), 0
    FROM days d
    WHERE NOT EXISTS (
        SELECT 1 FROM work_sessions s
        WHERE s.user_id = d.user_id AND s.local_day = d.local_day AND s.algorithm_version >= p_version
    )
    ON CONFLICT (user_id, local_day) WHERE status = 'queued' DO NOTHING;
    GET DIAGNOSTICS queued = ROW_COUNT;
    RETURN queued;
END
$$;

-- Privacy delete (WH-12): removes the caller's collected activity of one local
-- day, one device, or everything, and what was derived from it, then queues
-- the affected days so the engine rewrites their sessions. It runs as the
-- owner because the app role cannot delete events. The user comes from
-- app_current_user_id(), never from a parameter.
--   day:    events starting in the local day (user's zone); the day and its
--           neighbours are recalculated (intervals can cross midnight).
--   device: all events and coverage gaps of one of the caller's devices.
--   all:    every event and every derived row and correction; projects,
--           rules, exclusions, devices and the account stay.
-- No raw archive exists yet (WH-6): when it does, its objects go here too.
CREATE OR REPLACE FUNCTION app_privacy_delete(p_scope text, p_day date, p_device_id bigint)
    RETURNS TABLE (deleted_events bigint, queued_days integer)
    LANGUAGE plpgsql SECURITY DEFINER SET search_path = public, pg_temp
AS $$
DECLARE
    uid bigint := app_current_user_id();
    tz text;
    ids uuid[] := '{}';
    affected date[] := '{}';
    n bigint := 0;
    q integer := 0;
BEGIN
    IF uid IS NULL THEN
        RAISE EXCEPTION 'no current user' USING ERRCODE = '42501';
    END IF;
    SELECT coalesce(time_zone, 'UTC') INTO tz FROM users WHERE id = uid;

    IF p_scope = 'day' THEN
        IF p_day IS NULL THEN
            RAISE EXCEPTION 'day required' USING ERRCODE = '22023';
        END IF;
        WITH gone AS (
            DELETE FROM activity_events
            WHERE user_id = uid
              AND server_adjusted_start >= (p_day::timestamp AT TIME ZONE tz)
              AND server_adjusted_start < ((p_day + 1)::timestamp AT TIME ZONE tz)
            RETURNING event_id
        )
        SELECT coalesce(array_agg(event_id), '{}') INTO ids FROM gone;
        affected := ARRAY[p_day - 1, p_day, p_day + 1];
    ELSIF p_scope = 'device' THEN
        IF NOT EXISTS (SELECT 1 FROM devices WHERE id = p_device_id AND user_id = uid) THEN
            RAISE EXCEPTION 'unknown device' USING ERRCODE = 'P0002';
        END IF;
        WITH gone AS (
            DELETE FROM activity_events
            WHERE user_id = uid AND device_id = p_device_id
            RETURNING event_id, (server_adjusted_start AT TIME ZONE tz)::date AS local_day
        )
        SELECT coalesce(array_agg(event_id), '{}'), coalesce(array_agg(DISTINCT local_day), '{}')
        INTO ids, affected FROM gone;
        DELETE FROM coverage_gaps WHERE user_id = uid AND device_id = p_device_id;
    ELSIF p_scope = 'all' THEN
        DELETE FROM activity_events WHERE user_id = uid;
        GET DIAGNOSTICS n = ROW_COUNT;
        DELETE FROM work_sessions WHERE user_id = uid;  -- session_evidence cascades
        DELETE FROM event_assignments WHERE user_id = uid;
        DELETE FROM coverage_gaps WHERE user_id = uid;
        DELETE FROM assignment_overrides WHERE user_id = uid;
        DELETE FROM time_adjustments WHERE user_id = uid;
        DELETE FROM ai_classifications WHERE user_id = uid;
        DELETE FROM rule_proposals WHERE user_id = uid;
        DELETE FROM processing_jobs WHERE user_id = uid AND status <> 'running';
        -- A job running now may have read events before this delete: run its
        -- day again so it ends empty.
        SELECT coalesce(array_agg(DISTINCT local_day), '{}') INTO affected
        FROM processing_jobs WHERE user_id = uid AND status = 'running';
    ELSE
        RAISE EXCEPTION 'invalid scope' USING ERRCODE = '22023';
    END IF;

    IF p_scope <> 'all' THEN
        n := cardinality(ids);
        DELETE FROM event_assignments WHERE user_id = uid AND event_id = ANY(ids);
        DELETE FROM session_evidence WHERE user_id = uid AND event_id = ANY(ids);
        DELETE FROM assignment_overrides WHERE user_id = uid AND event_id = ANY(ids);
    END IF;

    IF cardinality(affected) > 0 THEN
        INSERT INTO processing_jobs (user_id, local_day, reason, status, run_after, attempts)
        SELECT uid, d, 'privacy_delete', 'queued', now(), 0 FROM unnest(affected) AS d
        ON CONFLICT (user_id, local_day) WHERE status = 'queued'
        DO UPDATE SET run_after = LEAST(processing_jobs.run_after, EXCLUDED.run_after),
                      reason = EXCLUDED.reason, updated_at = now();
        q := cardinality(affected);
    END IF;
    RETURN QUERY SELECT n, q;
END
$$;

DO $$
DECLARE
    fn text;
BEGIN
    FOREACH fn IN ARRAY ARRAY[
        'app_workspace_role(bigint)', 'app_is_member(bigint)', 'app_is_admin(bigint)',
        'app_workspace_has_members(bigint)', 'app_can_view_team_submissions(bigint)',
        'app_resolve_identity(text, text, text, text)', 'app_ensure_event_partitions(integer, integer)',
        'app_web_session_user(text)', 'app_enqueue_uncalculated_days(integer, integer, text)',
        'app_privacy_delete(text, date, bigint)'
    ] LOOP
        EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', fn);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO secretary_app', fn);
    END LOOP;
END
$$;

-- 12 months back (retention window, Git backfill, buffered uploads) + 3 ahead.
SELECT app_ensure_event_partitions(12, 3);

-- ---------------------------------------------------------------------------
-- indexes the ORM cannot declare (expressions, partial, multi-column)

CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_ux ON users (lower(email));
CREATE INDEX IF NOT EXISTS activity_events_user_time_ix ON activity_events (user_id, server_adjusted_start);
CREATE INDEX IF NOT EXISTS event_assignments_day_ix ON event_assignments (user_id, local_day);
CREATE INDEX IF NOT EXISTS work_sessions_day_ix ON work_sessions (user_id, local_day);
CREATE INDEX IF NOT EXISTS work_sessions_project_ix ON work_sessions (project_id, starts_at);
CREATE INDEX IF NOT EXISTS coverage_gaps_day_ix ON coverage_gaps (user_id, local_day);
CREATE INDEX IF NOT EXISTS assignment_overrides_day_ix ON assignment_overrides (user_id, local_day);
CREATE INDEX IF NOT EXISTS time_adjustments_day_ix ON time_adjustments (user_id, local_day);
CREATE INDEX IF NOT EXISTS ai_request_log_user_ix ON ai_request_log (user_id, created_at);
CREATE INDEX IF NOT EXISTS submissions_workspace_period_ix ON submissions (workspace_id, period_start);
CREATE INDEX IF NOT EXISTS audit_log_workspace_time_ix ON audit_log (workspace_id, created_at);
-- At most one queued job per (user, day): enqueue is an upsert on this index.
CREATE UNIQUE INDEX IF NOT EXISTS processing_jobs_queued_ux
    ON processing_jobs (user_id, local_day) WHERE status = 'queued';
CREATE INDEX IF NOT EXISTS processing_jobs_ready_ix ON processing_jobs (run_after) WHERE status = 'queued';
-- At most one running job per (user, day): two engine workers can never
-- calculate the same day at once (EN-2), even if a claim races.
CREATE UNIQUE INDEX IF NOT EXISTS processing_jobs_running_ux
    ON processing_jobs (user_id, local_day) WHERE status = 'running';

-- ---------------------------------------------------------------------------
-- row-level security (every tenant table; processing_jobs is the system queue)

DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'users', 'user_identities', 'workspaces', 'memberships', 'invitations', 'devices',
        'activity_events', 'projects', 'project_links', 'project_mappings', 'exclusions',
        'rule_proposals', 'event_assignments', 'work_sessions', 'session_evidence',
        'coverage_gaps', 'assignment_overrides', 'time_adjustments', 'ai_classifications',
        'ai_request_log', 'submissions', 'audit_log', 'web_sessions'
    ] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', t);
    END LOOP;
END
$$;

-- users: a user sees and edits only themself. Sign-up is app_resolve_identity().
DROP POLICY IF EXISTS users_self ON users;
CREATE POLICY users_self ON users
    USING (id = app_current_user_id())
    WITH CHECK (id = app_current_user_id());

DROP POLICY IF EXISTS user_identities_owner ON user_identities;
CREATE POLICY user_identities_owner ON user_identities
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());

DROP POLICY IF EXISTS workspaces_select ON workspaces;
CREATE POLICY workspaces_select ON workspaces FOR SELECT
    USING (app_is_member(id));
DROP POLICY IF EXISTS workspaces_insert ON workspaces;
CREATE POLICY workspaces_insert ON workspaces FOR INSERT
    WITH CHECK (kind = 'team' OR personal_owner_id = app_current_user_id());
DROP POLICY IF EXISTS workspaces_update ON workspaces;
CREATE POLICY workspaces_update ON workspaces FOR UPDATE
    USING (app_is_admin(id)) WITH CHECK (app_is_admin(id));
DROP POLICY IF EXISTS workspaces_delete ON workspaces;
CREATE POLICY workspaces_delete ON workspaces FOR DELETE
    USING (app_workspace_role(id) = 'owner' AND kind = 'team');

-- Members see who else is in their workspaces (names/roles only; no activity).
DROP POLICY IF EXISTS memberships_select ON memberships;
CREATE POLICY memberships_select ON memberships FOR SELECT
    USING (app_is_member(workspace_id));
-- The creator becomes owner of a new (empty) workspace; admins add non-owners.
DROP POLICY IF EXISTS memberships_insert ON memberships;
CREATE POLICY memberships_insert ON memberships FOR INSERT
    WITH CHECK (
        (user_id = app_current_user_id() AND role = 'owner' AND NOT app_workspace_has_members(workspace_id))
        OR (app_is_admin(workspace_id) AND role <> 'owner')
    );
DROP POLICY IF EXISTS memberships_update ON memberships;
CREATE POLICY memberships_update ON memberships FOR UPDATE
    USING (app_is_admin(workspace_id)) WITH CHECK (app_is_admin(workspace_id) AND role <> 'owner');
-- Admins remove members; anyone may leave.
DROP POLICY IF EXISTS memberships_delete ON memberships;
CREATE POLICY memberships_delete ON memberships FOR DELETE
    USING (app_is_admin(workspace_id) OR user_id = app_current_user_id());

-- Accepting an invite (invitee is not a member yet) will use a SECURITY DEFINER function (Phase 3).
DROP POLICY IF EXISTS invitations_admin ON invitations;
CREATE POLICY invitations_admin ON invitations
    USING (app_is_admin(workspace_id))
    WITH CHECK (app_is_admin(workspace_id));

-- Token -> device lookup (WH-4) happens before a user context exists and will
-- use a SECURITY DEFINER function.
DROP POLICY IF EXISTS devices_owner ON devices;
CREATE POLICY devices_owner ON devices
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());

-- Dashboard sessions (OV-2): the owner manages their own; lookup by token
-- before a context exists is app_web_session_user().
DROP POLICY IF EXISTS web_sessions_owner ON web_sessions;
CREATE POLICY web_sessions_owner ON web_sessions
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());

-- Insert-only for the app role (see grants). Deletion only via the
-- privacy-delete procedure (WH-12), which runs as the owner.
DROP POLICY IF EXISTS activity_events_select ON activity_events;
CREATE POLICY activity_events_select ON activity_events FOR SELECT
    USING (user_id = app_current_user_id());
DROP POLICY IF EXISTS activity_events_insert ON activity_events;
CREATE POLICY activity_events_insert ON activity_events FOR INSERT
    WITH CHECK (user_id = app_current_user_id());

DROP POLICY IF EXISTS projects_select ON projects;
CREATE POLICY projects_select ON projects FOR SELECT
    USING (app_is_member(workspace_id));
-- Personal workspace: the user is owner. Team workspace: admins manage shared projects.
DROP POLICY IF EXISTS projects_insert ON projects;
CREATE POLICY projects_insert ON projects FOR INSERT
    WITH CHECK (app_is_admin(workspace_id));
DROP POLICY IF EXISTS projects_update ON projects;
CREATE POLICY projects_update ON projects FOR UPDATE
    USING (app_is_admin(workspace_id)) WITH CHECK (app_is_admin(workspace_id));
DROP POLICY IF EXISTS projects_delete ON projects;
CREATE POLICY projects_delete ON projects FOR DELETE
    USING (app_is_admin(workspace_id));

DROP POLICY IF EXISTS project_links_owner ON project_links;
CREATE POLICY project_links_owner ON project_links
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id() AND app_is_member(workspace_id));

-- Personal rules: owner only. Shared rules (user_id NULL): members see, admins manage.
DROP POLICY IF EXISTS project_mappings_select ON project_mappings;
CREATE POLICY project_mappings_select ON project_mappings FOR SELECT
    USING (user_id = app_current_user_id() OR (user_id IS NULL AND app_is_member(workspace_id)));
DROP POLICY IF EXISTS project_mappings_write ON project_mappings;
CREATE POLICY project_mappings_write ON project_mappings FOR ALL
    USING (user_id = app_current_user_id() OR (user_id IS NULL AND app_is_admin(workspace_id)))
    WITH CHECK (
        (user_id = app_current_user_id() AND app_is_member(workspace_id))
        OR (user_id IS NULL AND app_is_admin(workspace_id))
    );

-- Tables visible to and writable by their owning user only.
DO $$
DECLARE
    t text;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'exclusions', 'rule_proposals', 'event_assignments', 'work_sessions', 'session_evidence',
        'coverage_gaps', 'assignment_overrides', 'time_adjustments', 'ai_classifications'
    ] LOOP
        EXECUTE format('DROP POLICY IF EXISTS %I ON %I', t || '_owner', t);
        EXECUTE format(
            'CREATE POLICY %I ON %I USING (user_id = app_current_user_id()) '
            'WITH CHECK (user_id = app_current_user_id())',
            t || '_owner', t
        );
    END LOOP;
END
$$;

-- Append-only, shown to the user.
DROP POLICY IF EXISTS ai_request_log_select ON ai_request_log;
CREATE POLICY ai_request_log_select ON ai_request_log FOR SELECT
    USING (user_id = app_current_user_id());
DROP POLICY IF EXISTS ai_request_log_insert ON ai_request_log;
CREATE POLICY ai_request_log_insert ON ai_request_log FOR INSERT
    WITH CHECK (user_id = app_current_user_id());

-- Private until submitted: drafts are owner-only; non-drafts reach admins
-- (and members when the workspace allows it).
DROP POLICY IF EXISTS submissions_select ON submissions;
CREATE POLICY submissions_select ON submissions FOR SELECT
    USING (
        user_id = app_current_user_id()
        OR (status <> 'draft' AND app_can_view_team_submissions(workspace_id))
    );
DROP POLICY IF EXISTS submissions_insert ON submissions;
CREATE POLICY submissions_insert ON submissions FOR INSERT
    WITH CHECK (
        user_id = app_current_user_id()
        AND app_is_member(workspace_id)
        AND status IN ('draft', 'submitted')
    );
-- Owners edit and (re)submit their own submissions but cannot approve them;
-- admins approve/reject non-drafts.
DROP POLICY IF EXISTS submissions_update ON submissions;
CREATE POLICY submissions_update ON submissions FOR UPDATE
    USING (user_id = app_current_user_id() OR (status <> 'draft' AND app_is_admin(workspace_id)))
    WITH CHECK (
        (user_id = app_current_user_id() AND status IN ('draft', 'submitted'))
        OR (status <> 'draft' AND app_is_admin(workspace_id))
    );
DROP POLICY IF EXISTS submissions_delete ON submissions;
CREATE POLICY submissions_delete ON submissions FOR DELETE
    USING (user_id = app_current_user_id() AND status = 'draft');

DROP POLICY IF EXISTS audit_log_select ON audit_log;
CREATE POLICY audit_log_select ON audit_log FOR SELECT
    USING (app_is_admin(workspace_id));
DROP POLICY IF EXISTS audit_log_insert ON audit_log;
CREATE POLICY audit_log_insert ON audit_log FOR INSERT
    WITH CHECK (actor_user_id = app_current_user_id() AND app_is_member(workspace_id));

-- ---------------------------------------------------------------------------
-- grants: reset, then grant exactly what the app role may do

REVOKE ALL ON ALL TABLES IN SCHEMA public FROM secretary_app;
REVOKE ALL ON ALL SEQUENCES IN SCHEMA public FROM secretary_app;

GRANT SELECT, UPDATE ON users TO secretary_app;
GRANT SELECT, INSERT, DELETE ON user_identities TO secretary_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON workspaces, memberships, invitations, devices TO secretary_app;
GRANT SELECT, INSERT ON activity_events TO secretary_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON
    projects, project_links, project_mappings, exclusions, rule_proposals,
    event_assignments, work_sessions, session_evidence, coverage_gaps,
    assignment_overrides, time_adjustments, ai_classifications, submissions,
    processing_jobs
    TO secretary_app;
GRANT SELECT, INSERT ON ai_request_log, audit_log TO secretary_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON web_sessions TO secretary_app;
-- sys_model / sys_field (ORM schema bookkeeping): owner only, no grants.

DO $$
DECLARE
    t text;
BEGIN
    -- bigserial ids: inserting needs the table's sequence.
    FOREACH t IN ARRAY ARRAY[
        'user_identities', 'workspaces', 'memberships', 'invitations', 'devices',
        'activity_events', 'projects', 'project_links', 'project_mappings', 'exclusions',
        'rule_proposals', 'event_assignments', 'work_sessions', 'session_evidence',
        'coverage_gaps', 'assignment_overrides', 'time_adjustments', 'ai_classifications',
        'ai_request_log', 'submissions', 'audit_log', 'processing_jobs', 'web_sessions'
    ] LOOP
        -- pg_get_serial_sequence returns an already-quoted, qualified name.
        EXECUTE format('GRANT USAGE ON SEQUENCE %s TO secretary_app', pg_get_serial_sequence(t, 'id'));
    END LOOP;
END
$$;
