-- 0002 devices: registered agents and the device authorization flow.

CREATE TABLE devices (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name            text NOT NULL,
    os              text NOT NULL,
    agent_version   text,
    token_hash      bytea NOT NULL UNIQUE,  -- SHA-256 of the device token; the token itself is never stored
    token_prefix    text NOT NULL,          -- first characters, for display only
    clock_offset_ms integer NOT NULL DEFAULT 0,
    queue_depth     integer,
    last_seen_at    timestamptz,
    created_at      timestamptz NOT NULL DEFAULT now(),
    revoked_at      timestamptz
);
CREATE INDEX devices_user_idx ON devices (user_id);

ALTER TABLE devices ENABLE ROW LEVEL SECURITY;
-- Token → device lookup happens before a user context exists; it will use a
-- SECURITY DEFINER function added with the device auth task (WH-4).
CREATE POLICY devices_owner ON devices
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, INSERT, UPDATE, DELETE ON devices TO secretary_app;

CREATE TYPE device_authorization_status AS ENUM ('pending', 'approved', 'denied', 'consumed');

CREATE TABLE device_authorizations (
    id               uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    device_code_hash bytea NOT NULL UNIQUE,  -- SHA-256 of the device_code the agent polls with
    user_code        text NOT NULL UNIQUE,   -- short code the user types/approves in the browser
    status           device_authorization_status NOT NULL DEFAULT 'pending',
    device_name      text NOT NULL,
    device_os        text NOT NULL,
    user_id          uuid REFERENCES users (id) ON DELETE CASCADE,   -- set on approval
    device_id        uuid REFERENCES devices (id) ON DELETE CASCADE, -- set when the token is issued
    created_at       timestamptz NOT NULL DEFAULT now(),
    expires_at       timestamptz NOT NULL,
    approved_at      timestamptz,
    CONSTRAINT device_authorizations_approved_has_user CHECK (status IN ('pending', 'denied') OR user_id IS NOT NULL)
);

ALTER TABLE device_authorizations ENABLE ROW LEVEL SECURITY;
-- Pending authorizations belong to nobody yet. Creating, polling, and
-- approving-by-user-code go through SECURITY DEFINER functions (WH-4), so a
-- signed-in user can never list other people's pending codes.
CREATE POLICY device_authorizations_owner ON device_authorizations
    USING (user_id = app_current_user_id())
    WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, UPDATE ON device_authorizations TO secretary_app;
