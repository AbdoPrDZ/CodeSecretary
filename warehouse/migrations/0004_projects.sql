-- 0004 projects and the configuration that maps activity to them.

CREATE TABLE projects (
    id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id          uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name                  text NOT NULL,
    color                 text,
    client_name           text,
    billable              boolean NOT NULL DEFAULT true,
    waiting_time_billable boolean NOT NULL DEFAULT false,
    hourly_rate           numeric(12, 2),
    currency              char(3),
    budget_minutes        integer CHECK (budget_minutes IS NULL OR budget_minutes > 0),
    created_by            uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at            timestamptz NOT NULL DEFAULT now(),
    archived_at           timestamptz,
    UNIQUE (id, workspace_id)  -- target for composite FKs that pin rows to the project's workspace
);
CREATE INDEX projects_workspace_idx ON projects (workspace_id);

ALTER TABLE projects ENABLE ROW LEVEL SECURITY;
CREATE POLICY projects_select ON projects FOR SELECT
    USING (app_is_member(workspace_id));
-- Personal workspace: the user is owner. Team workspace: admins manage shared projects.
CREATE POLICY projects_insert ON projects FOR INSERT
    WITH CHECK (app_is_admin(workspace_id));
CREATE POLICY projects_update ON projects FOR UPDATE
    USING (app_is_admin(workspace_id)) WITH CHECK (app_is_admin(workspace_id));
CREATE POLICY projects_delete ON projects FOR DELETE
    USING (app_is_admin(workspace_id));
GRANT SELECT, INSERT, UPDATE, DELETE ON projects TO secretary_app;

-- A member's link from their own repos/paths to a (usually shared) project.
CREATE TABLE project_links (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_id        uuid NOT NULL,
    workspace_id      uuid NOT NULL,
    repository_remote text,   -- normalized remote URL
    local_path        text,
    created_at        timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (project_id, workspace_id) REFERENCES projects (id, workspace_id) ON DELETE CASCADE,
    CONSTRAINT project_links_has_target CHECK (repository_remote IS NOT NULL OR local_path IS NOT NULL)
);
CREATE INDEX project_links_user_idx ON project_links (user_id);

ALTER TABLE project_links ENABLE ROW LEVEL SECURITY;
CREATE POLICY project_links_owner ON project_links
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id() AND app_is_member(workspace_id));
GRANT SELECT, INSERT, UPDATE, DELETE ON project_links TO secretary_app;

CREATE TYPE mapping_signal AS ENUM ('path', 'remote', 'domain', 'title', 'app');

-- Deterministic rules signal → project. `user_id` set = personal rule (only
-- its owner sees it); NULL = shared team rule (members see, admins manage).
CREATE TABLE project_mappings (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL,
    project_id   uuid NOT NULL,
    user_id      uuid REFERENCES users (id) ON DELETE CASCADE,
    signal       mapping_signal NOT NULL,
    pattern      text NOT NULL,
    priority     integer NOT NULL DEFAULT 0,
    created_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (project_id, workspace_id) REFERENCES projects (id, workspace_id) ON DELETE CASCADE
);
CREATE INDEX project_mappings_user_idx ON project_mappings (user_id);
CREATE INDEX project_mappings_workspace_idx ON project_mappings (workspace_id);

ALTER TABLE project_mappings ENABLE ROW LEVEL SECURITY;
CREATE POLICY project_mappings_select ON project_mappings FOR SELECT
    USING (user_id = app_current_user_id() OR (user_id IS NULL AND app_is_member(workspace_id)));
CREATE POLICY project_mappings_write ON project_mappings FOR ALL
    USING (user_id = app_current_user_id() OR (user_id IS NULL AND app_is_admin(workspace_id)))
    WITH CHECK (
        (user_id = app_current_user_id() AND app_is_member(workspace_id))
        OR (user_id IS NULL AND app_is_admin(workspace_id))
    );
GRANT SELECT, INSERT, UPDATE, DELETE ON project_mappings TO secretary_app;

CREATE TYPE exclusion_kind AS ENUM ('path', 'app', 'domain', 'time_window');

CREATE TABLE exclusions (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind         exclusion_kind NOT NULL,
    pattern      text,                 -- path / app / domain pattern
    window_start time,                 -- time_window: local time of day
    window_end   time,
    days_of_week smallint[],           -- time_window: ISO 1 = Monday … 7 = Sunday; NULL = every day
    created_at   timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT exclusions_shape CHECK (
        (kind = 'time_window' AND window_start IS NOT NULL AND window_end IS NOT NULL)
        OR (kind <> 'time_window' AND pattern IS NOT NULL)
    )
);
CREATE INDEX exclusions_user_idx ON exclusions (user_id);

ALTER TABLE exclusions ENABLE ROW LEVEL SECURITY;
CREATE POLICY exclusions_owner ON exclusions
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, INSERT, UPDATE, DELETE ON exclusions TO secretary_app;

CREATE TYPE proposal_status AS ENUM ('pending', 'accepted', 'rejected');

CREATE TABLE rule_proposals (
    id             uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id        uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    project_id     uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    signal         mapping_signal NOT NULL,
    pattern        text NOT NULL,
    evidence_count integer NOT NULL DEFAULT 0,
    status         proposal_status NOT NULL DEFAULT 'pending',
    created_at     timestamptz NOT NULL DEFAULT now(),
    decided_at     timestamptz,
    UNIQUE (user_id, signal, pattern)  -- rejected proposals are never repeated
);

ALTER TABLE rule_proposals ENABLE ROW LEVEL SECURITY;
CREATE POLICY rule_proposals_owner ON rule_proposals
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, INSERT, UPDATE, DELETE ON rule_proposals TO secretary_app;
