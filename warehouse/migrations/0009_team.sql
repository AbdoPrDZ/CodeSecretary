-- 0009 team features (Phase 3 use): submissions and the audit log.
--
-- Submissions are the ONLY per-user time data another user can ever see
-- ("private until submitted"). Drafts are visible to their owner only.

CREATE TYPE submission_status AS ENUM ('draft', 'submitted', 'approved', 'rejected');

CREATE TABLE submissions (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id    uuid NOT NULL,
    project_id      uuid NOT NULL,
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    period_start    date NOT NULL,
    period_end      date NOT NULL,
    total_seconds   integer NOT NULL CHECK (total_seconds >= 0),
    waiting_seconds integer NOT NULL DEFAULT 0 CHECK (waiting_seconds >= 0),
    summary         text,          -- optional, user-approved
    status          submission_status NOT NULL DEFAULT 'draft',
    revision        integer NOT NULL DEFAULT 1,
    supersedes      uuid REFERENCES submissions (id) ON DELETE SET NULL,
    submitted_at    timestamptz,
    decided_at      timestamptz,
    decided_by      uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (project_id, workspace_id) REFERENCES projects (id, workspace_id) ON DELETE CASCADE,
    CONSTRAINT submissions_period CHECK (period_end >= period_start)
);
CREATE INDEX submissions_workspace_idx ON submissions (workspace_id, period_start);
CREATE INDEX submissions_user_idx ON submissions (user_id);

-- May the current user see other members' non-draft submissions in `ws`?
CREATE FUNCTION app_can_view_team_submissions(ws uuid) RETURNS boolean
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
REVOKE ALL ON FUNCTION app_can_view_team_submissions(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app_can_view_team_submissions(uuid) TO secretary_app;

ALTER TABLE submissions ENABLE ROW LEVEL SECURITY;
CREATE POLICY submissions_select ON submissions FOR SELECT
    USING (
        user_id = app_current_user_id()
        OR (status <> 'draft' AND app_can_view_team_submissions(workspace_id))
    );
CREATE POLICY submissions_insert ON submissions FOR INSERT
    WITH CHECK (
        user_id = app_current_user_id()
        AND app_is_member(workspace_id)
        AND status IN ('draft', 'submitted')
    );
-- Owners edit and (re)submit their own submissions but cannot approve them;
-- admins approve/reject non-drafts.
CREATE POLICY submissions_update ON submissions FOR UPDATE
    USING (user_id = app_current_user_id() OR (status <> 'draft' AND app_is_admin(workspace_id)))
    WITH CHECK (
        (user_id = app_current_user_id() AND status IN ('draft', 'submitted'))
        OR (status <> 'draft' AND app_is_admin(workspace_id))
    );
CREATE POLICY submissions_delete ON submissions FOR DELETE
    USING (user_id = app_current_user_id() AND status = 'draft');
GRANT SELECT, INSERT, UPDATE, DELETE ON submissions TO secretary_app;

-- Admin and security actions. Append-only.
CREATE TABLE audit_log (
    id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    workspace_id  uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    actor_user_id uuid REFERENCES users (id) ON DELETE SET NULL,
    action        text NOT NULL,     -- e.g. 'member.removed', 'submission.approved'
    target_type   text,
    target_id     uuid,
    details       jsonb NOT NULL DEFAULT '{}'::jsonb,  -- never raw activity data
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_workspace_idx ON audit_log (workspace_id, created_at);

ALTER TABLE audit_log ENABLE ROW LEVEL SECURITY;
CREATE POLICY audit_log_select ON audit_log FOR SELECT
    USING (app_is_admin(workspace_id));
CREATE POLICY audit_log_insert ON audit_log FOR INSERT
    WITH CHECK (actor_user_id = app_current_user_id() AND app_is_member(workspace_id));
GRANT SELECT, INSERT ON audit_log TO secretary_app;
