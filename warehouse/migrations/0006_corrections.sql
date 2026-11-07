-- 0006 user corrections: inputs to the engine, reapplied on every
-- recalculation. They are never edits to derived rows.

-- Manual project choice for one event (assignment method 'user', confidence 1.0).
CREATE TABLE assignment_overrides (
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    event_id   uuid NOT NULL,
    local_day  date NOT NULL,
    project_id uuid REFERENCES projects (id) ON DELETE CASCADE,  -- NULL = "not project work"
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, event_id)
);
CREATE INDEX assignment_overrides_day_idx ON assignment_overrides (user_id, local_day);

CREATE TYPE adjustment_kind AS ENUM ('add', 'remove', 'reassign');

-- Time-range corrections, anchored to time and project so they survive recalculation.
CREATE TABLE time_adjustments (
    id              uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    local_day       date NOT NULL,
    kind            adjustment_kind NOT NULL,
    starts_at       timestamptz NOT NULL,
    ends_at         timestamptz NOT NULL,
    project_id      uuid REFERENCES projects (id) ON DELETE CASCADE,  -- target of add / reassign
    from_project_id uuid REFERENCES projects (id) ON DELETE CASCADE,  -- optional filter for remove / reassign
    note            text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    deleted_at      timestamptz,                                       -- undo keeps history
    CONSTRAINT time_adjustments_positive CHECK (ends_at > starts_at),
    CONSTRAINT time_adjustments_target CHECK (kind = 'remove' OR project_id IS NOT NULL)
);
CREATE INDEX time_adjustments_day_idx ON time_adjustments (user_id, local_day);

ALTER TABLE assignment_overrides ENABLE ROW LEVEL SECURITY;
CREATE POLICY assignment_overrides_owner ON assignment_overrides
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());

ALTER TABLE time_adjustments ENABLE ROW LEVEL SECURITY;
CREATE POLICY time_adjustments_owner ON time_adjustments
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());

GRANT SELECT, INSERT, UPDATE, DELETE ON assignment_overrides, time_adjustments TO secretary_app;
