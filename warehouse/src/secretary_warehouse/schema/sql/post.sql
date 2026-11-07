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

DO $$
DECLARE
    fn text;
BEGIN
    FOREACH fn IN ARRAY ARRAY[
        'app_workspace_role(bigint)', 'app_is_member(bigint)', 'app_is_admin(bigint)',
        'app_workspace_has_members(bigint)', 'app_can_view_team_submissions(bigint)',
        'app_resolve_identity(text, text, text, text)', 'app_ensure_event_partitions(integer, integer)'
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
        'ai_request_log', 'submissions', 'audit_log'
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
        'ai_request_log', 'submissions', 'audit_log', 'processing_jobs'
    ] LOOP
        -- pg_get_serial_sequence returns an already-quoted, qualified name.
        EXECUTE format('GRANT USAGE ON SEQUENCE %s TO secretary_app', pg_get_serial_sequence(t, 'id'));
    END LOOP;
END
$$;
