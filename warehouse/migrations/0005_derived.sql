-- 0005 derived data, written only by the engine and replaced per (user, local day).
--
-- No foreign keys to activity_events: a FK into a partitioned table must
-- reference its full key (event_id, timestamp_start). Derived rows are
-- rebuilt from events on every recalculation, and privacy delete removes
-- them explicitly.

CREATE TYPE assignment_method AS ENUM ('user', 'rule', 'shared_rule', 'structural', 'temporal', 'ai');

CREATE TABLE event_assignments (
    user_id           uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    event_id          uuid NOT NULL,
    local_day         date NOT NULL,
    project_id        uuid REFERENCES projects (id) ON DELETE CASCADE,  -- NULL = unassigned (inbox)
    confidence        real NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    method            assignment_method NOT NULL,
    rule_id           uuid,
    model_id          text,
    algorithm_version integer NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, event_id)
);
CREATE INDEX event_assignments_day_idx ON event_assignments (user_id, local_day);

CREATE TYPE session_kind AS ENUM ('active', 'waiting');

CREATE TABLE work_sessions (
    id                uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id           uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    local_day         date NOT NULL,
    project_id        uuid REFERENCES projects (id) ON DELETE SET NULL,  -- NULL = unassigned
    kind              session_kind NOT NULL,
    starts_at         timestamptz NOT NULL,
    ends_at           timestamptz NOT NULL,
    low_confidence    boolean NOT NULL DEFAULT false,  -- overlaps a coverage gap
    estimated         boolean NOT NULL DEFAULT false,  -- Git backfill; excluded from accuracy metrics
    algorithm_version integer NOT NULL,
    config_hash       text NOT NULL,
    created_at        timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT work_sessions_positive CHECK (ends_at > starts_at)
);
CREATE INDEX work_sessions_day_idx ON work_sessions (user_id, local_day);
CREATE INDEX work_sessions_project_idx ON work_sessions (project_id, starts_at);

CREATE TABLE session_evidence (
    session_id uuid NOT NULL REFERENCES work_sessions (id) ON DELETE CASCADE,
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    event_id   uuid NOT NULL,
    PRIMARY KEY (session_id, event_id)
);
CREATE INDEX session_evidence_user_idx ON session_evidence (user_id);

CREATE TABLE coverage_gaps (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    device_id  uuid NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
    collector  text NOT NULL,
    local_day  date NOT NULL,
    starts_at  timestamptz NOT NULL,
    ends_at    timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT coverage_gaps_positive CHECK (ends_at > starts_at)
);
CREATE INDEX coverage_gaps_day_idx ON coverage_gaps (user_id, local_day);

ALTER TABLE event_assignments ENABLE ROW LEVEL SECURITY;
CREATE POLICY event_assignments_owner ON event_assignments
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());

ALTER TABLE work_sessions ENABLE ROW LEVEL SECURITY;
CREATE POLICY work_sessions_owner ON work_sessions
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());

ALTER TABLE session_evidence ENABLE ROW LEVEL SECURITY;
CREATE POLICY session_evidence_owner ON session_evidence
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());

ALTER TABLE coverage_gaps ENABLE ROW LEVEL SECURITY;
CREATE POLICY coverage_gaps_owner ON coverage_gaps
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());

GRANT SELECT, INSERT, UPDATE, DELETE ON event_assignments, work_sessions, session_evidence, coverage_gaps
    TO secretary_app;
