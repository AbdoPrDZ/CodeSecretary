-- 0008 AI (Phase 2 use; created now so tenancy is never retrofitted).

CREATE TABLE ai_classifications (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    input_hash text NOT NULL,   -- hash of the redacted fields sent; cache key
    project_id uuid REFERENCES projects (id) ON DELETE CASCADE,
    confidence real NOT NULL CHECK (confidence >= 0 AND confidence <= 0.8),  -- AI never exceeds 0.8
    model_id   text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_id, input_hash, model_id)
);

-- What was sent to an AI provider, when, and why. Append-only; shown to the user.
CREATE TABLE ai_request_log (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider    text NOT NULL,
    model_id    text NOT NULL,
    purpose     text NOT NULL,       -- 'classify', 'summarize', 'suggest_rule'
    fields_sent text[] NOT NULL,     -- field names, e.g. {domain,title}
    item_count  integer NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ai_request_log_user_idx ON ai_request_log (user_id, created_at);

ALTER TABLE ai_classifications ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_classifications_owner ON ai_classifications
    USING (user_id = app_current_user_id()) WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, INSERT, UPDATE, DELETE ON ai_classifications TO secretary_app;

ALTER TABLE ai_request_log ENABLE ROW LEVEL SECURITY;
CREATE POLICY ai_request_log_select ON ai_request_log FOR SELECT
    USING (user_id = app_current_user_id());
CREATE POLICY ai_request_log_insert ON ai_request_log FOR INSERT
    WITH CHECK (user_id = app_current_user_id());
GRANT SELECT, INSERT ON ai_request_log TO secretary_app;
