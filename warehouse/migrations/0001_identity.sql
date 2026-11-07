-- 0001 identity & tenancy: roles, RLS context helpers, users, workspaces, memberships, invitations.
--
-- Security model (applies to every later migration):
-- * Services connect as login roles that are members of `secretary_app`
--   (NOLOGIN, NOBYPASSRLS). They never own tables, so RLS always applies.
-- * Each transaction sets its tenant with `set_config('app.user_id', <uuid>, true)`.
--   Without it, `app_current_user_id()` is NULL and every policy matches nothing.
-- * Helpers that must read across RLS (e.g. "is the current user a member of
--   this workspace?") are SECURITY DEFINER, owned by the migration owner, and
--   only return facts about the current user.

DO $$
BEGIN
    CREATE ROLE secretary_app NOLOGIN NOBYPASSRLS;
EXCEPTION
    WHEN duplicate_object THEN NULL; -- roles are cluster-wide; may already exist
END
$$;

GRANT USAGE ON SCHEMA public TO secretary_app;

CREATE FUNCTION app_current_user_id() RETURNS uuid
    LANGUAGE sql STABLE
AS $$
    -- '' happens on pooled connections after a SET LOCAL ends; treat as unset.
    SELECT NULLIF(current_setting('app.user_id', true), '')::uuid
$$;

CREATE TYPE workspace_kind AS ENUM ('personal', 'team');
CREATE TYPE workspace_role AS ENUM ('owner', 'admin', 'member', 'viewer');

-- users -----------------------------------------------------------------------

CREATE TABLE users (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email        text NOT NULL,
    display_name text,
    time_zone    text NOT NULL DEFAULT 'UTC',  -- IANA name; defines the user's "local day"
    created_at   timestamptz NOT NULL DEFAULT now(),
    deleted_at   timestamptz
);
CREATE UNIQUE INDEX users_email_key ON users (lower(email));

ALTER TABLE users ENABLE ROW LEVEL SECURITY;
-- Sign-up: the service generates the id, sets it as context, then inserts.
CREATE POLICY users_self ON users
    USING (id = app_current_user_id())
    WITH CHECK (id = app_current_user_id());
GRANT SELECT, INSERT, UPDATE ON users TO secretary_app;

CREATE TABLE user_identities (
    provider   text NOT NULL,          -- e.g. 'github', 'google'
    subject    text NOT NULL,          -- provider's stable user id
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, subject)
);
CREATE INDEX user_identities_user_idx ON user_identities (user_id);

ALTER TABLE user_identities ENABLE ROW LEVEL SECURITY;
-- Login lookup by (provider, subject) happens before a user context exists;
-- it will use a SECURITY DEFINER function added with the overview auth task.
CREATE POLICY user_identities_owner ON user_identities
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, INSERT, DELETE ON user_identities TO secretary_app;

-- workspaces & memberships ----------------------------------------------------

CREATE TABLE workspaces (
    id                      uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    kind                    workspace_kind NOT NULL,
    name                    text NOT NULL,
    -- Set only for personal workspaces: exactly one per user.
    personal_owner_id       uuid UNIQUE REFERENCES users (id) ON DELETE CASCADE,
    -- Team policy: may ordinary members see each other's submitted totals?
    members_see_submissions boolean NOT NULL DEFAULT false,
    created_at              timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT workspaces_personal_owner CHECK ((kind = 'personal') = (personal_owner_id IS NOT NULL))
);

CREATE TABLE memberships (
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role         workspace_role NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX memberships_user_idx ON memberships (user_id);

-- Membership helpers (SECURITY DEFINER: read memberships without recursing into its own RLS).
CREATE FUNCTION app_workspace_role(ws uuid) RETURNS workspace_role
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT role FROM memberships WHERE workspace_id = ws AND user_id = app_current_user_id()
$$;

CREATE FUNCTION app_is_member(ws uuid) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT app_workspace_role(ws) IS NOT NULL
$$;

CREATE FUNCTION app_is_admin(ws uuid) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT coalesce(app_workspace_role(ws) IN ('owner', 'admin'), false)
$$;

CREATE FUNCTION app_workspace_has_members(ws uuid) RETURNS boolean
    LANGUAGE sql STABLE SECURITY DEFINER SET search_path = public, pg_temp
AS $$
    SELECT EXISTS (SELECT 1 FROM memberships WHERE workspace_id = ws)
$$;

REVOKE ALL ON FUNCTION app_workspace_role(uuid), app_is_member(uuid), app_is_admin(uuid),
    app_workspace_has_members(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app_workspace_role(uuid), app_is_member(uuid), app_is_admin(uuid),
    app_workspace_has_members(uuid) TO secretary_app;

ALTER TABLE workspaces ENABLE ROW LEVEL SECURITY;
CREATE POLICY workspaces_select ON workspaces FOR SELECT
    USING (app_is_member(id));
CREATE POLICY workspaces_insert ON workspaces FOR INSERT
    WITH CHECK (kind = 'team' OR personal_owner_id = app_current_user_id());
CREATE POLICY workspaces_update ON workspaces FOR UPDATE
    USING (app_is_admin(id)) WITH CHECK (app_is_admin(id));
CREATE POLICY workspaces_delete ON workspaces FOR DELETE
    USING (app_workspace_role(id) = 'owner' AND kind = 'team');
GRANT SELECT, INSERT, UPDATE, DELETE ON workspaces TO secretary_app;

ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
-- Members see who else is in their workspaces (names/roles only; no activity).
CREATE POLICY memberships_select ON memberships FOR SELECT
    USING (app_is_member(workspace_id));
-- The creator becomes owner of a new (empty) workspace; admins add non-owners.
CREATE POLICY memberships_insert ON memberships FOR INSERT
    WITH CHECK (
        (user_id = app_current_user_id() AND role = 'owner' AND NOT app_workspace_has_members(workspace_id))
        OR (app_is_admin(workspace_id) AND role <> 'owner')
    );
CREATE POLICY memberships_update ON memberships FOR UPDATE
    USING (app_is_admin(workspace_id)) WITH CHECK (app_is_admin(workspace_id) AND role <> 'owner');
-- Admins remove members; anyone may leave.
CREATE POLICY memberships_delete ON memberships FOR DELETE
    USING (app_is_admin(workspace_id) OR user_id = app_current_user_id());
GRANT SELECT, INSERT, UPDATE, DELETE ON memberships TO secretary_app;

-- invitations -----------------------------------------------------------------

CREATE TABLE invitations (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    email        text NOT NULL,
    role         workspace_role NOT NULL CHECK (role <> 'owner'),
    token_hash   bytea NOT NULL UNIQUE,   -- SHA-256 of the invite token
    invited_by   uuid REFERENCES users (id) ON DELETE SET NULL,
    expires_at   timestamptz NOT NULL,
    accepted_at  timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX invitations_workspace_idx ON invitations (workspace_id);

ALTER TABLE invitations ENABLE ROW LEVEL SECURITY;
-- Accepting an invite (invitee is not a member yet) will use a SECURITY DEFINER function (Phase 3).
CREATE POLICY invitations_admin ON invitations
    USING (app_is_admin(workspace_id))
    WITH CHECK (app_is_admin(workspace_id));
GRANT SELECT, INSERT, UPDATE, DELETE ON invitations TO secretary_app;
