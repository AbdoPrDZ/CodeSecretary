//! Transactional writer (EN-11): replace one (user, local day)'s derived rows.
//!
//! In the caller's transaction (tenant already set): delete the day's
//! sessions (evidence cascades) and engine assignments, insert the new
//! ones. Every session carries `algorithm_version` and `config_hash`.
//! Re-running a job writes identical rows (ids aside).

use chrono::NaiveDate;
use serde_json::json;
use sqlx::PgConnection;

use crate::ALGORITHM_VERSION;
use crate::model::{Output, Settings};

pub async fn replace_day(
    conn: &mut PgConnection,
    user_id: i64,
    local_day: NaiveDate,
    settings: &Settings,
    out: &Output,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM work_sessions WHERE user_id = $1 AND local_day = $2")
        .bind(user_id)
        .bind(local_day)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM event_assignments WHERE user_id = $1 AND local_day = $2")
        .bind(user_id)
        .bind(local_day)
        .execute(&mut *conn)
        .await?;

    if !out.assignments.is_empty() {
        let rows: Vec<_> = out
            .assignments
            .iter()
            .map(|a| {
                json!({
                    "event_id": a.event_id, "project_id": a.project_id,
                    "confidence": a.confidence, "method": a.method.as_str(), "rule_id": a.rule_id,
                })
            })
            .collect();
        // An event moves to another local day only when the time zone changes.
        sqlx::query(
            "INSERT INTO event_assignments
                 (user_id, event_id, local_day, project_id, confidence, method, rule_id, algorithm_version)
             SELECT $1, r.event_id, $2, r.project_id, r.confidence, r.method, r.rule_id, $4
               FROM jsonb_to_recordset($3) AS r(
                    event_id uuid, project_id bigint, confidence float8, method text, rule_id bigint)
             ON CONFLICT (user_id, event_id) DO UPDATE
                SET local_day = EXCLUDED.local_day, project_id = EXCLUDED.project_id,
                    confidence = EXCLUDED.confidence, method = EXCLUDED.method,
                    rule_id = EXCLUDED.rule_id, algorithm_version = EXCLUDED.algorithm_version,
                    updated_at = now()",
        )
        .bind(user_id)
        .bind(local_day)
        .bind(serde_json::Value::Array(rows))
        .bind(ALGORITHM_VERSION)
        .execute(&mut *conn)
        .await?;
    }

    if !out.sessions.is_empty() {
        let sessions: Vec<_> = out
            .sessions
            .iter()
            .map(|s| {
                json!({
                    "project_id": s.project_id, "kind": s.kind.as_str(),
                    "starts_at": s.start, "ends_at": s.end,
                    "low_confidence": s.low_confidence, "estimated": s.estimated,
                })
            })
            .collect();
        // Sessions of one day never overlap, so the start identifies a session.
        let evidence: Vec<_> = out
            .sessions
            .iter()
            .flat_map(|s| {
                s.evidence
                    .iter()
                    .map(move |id| json!({ "starts_at": s.start, "event_id": id }))
            })
            .collect();
        sqlx::query(
            "WITH s AS (
                 INSERT INTO work_sessions
                     (user_id, local_day, project_id, kind, starts_at, ends_at,
                      low_confidence, estimated, algorithm_version, config_hash)
                 SELECT $1, $2, r.project_id, r.kind, r.starts_at, r.ends_at,
                        r.low_confidence, r.estimated, $5, $6
                   FROM jsonb_to_recordset($3) AS r(
                        project_id bigint, kind text, starts_at timestamptz, ends_at timestamptz,
                        low_confidence boolean, estimated boolean)
                 RETURNING id, starts_at)
             INSERT INTO session_evidence (session_id, user_id, event_id)
             SELECT s.id, $1, e.event_id
               FROM s JOIN jsonb_to_recordset($4) AS e(starts_at timestamptz, event_id uuid)
                 ON e.starts_at = s.starts_at",
        )
        .bind(user_id)
        .bind(local_day)
        .bind(serde_json::Value::Array(sessions))
        .bind(serde_json::Value::Array(evidence))
        .bind(ALGORITHM_VERSION)
        .bind(settings.config_hash())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
