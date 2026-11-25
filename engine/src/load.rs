//! Context loader (EN-3): everything one (user, local day) calculation reads:
//! events, rules, overrides and the day's adjustments (EN-10).
//!
//! Runs inside a transaction whose `app.user_id` is the job's user, so RLS
//! limits every query to that user's rows (and the shared rules of their
//! workspaces). Events are loaded for the day widened by
//! [`DAY_MARGIN_SECS`] on both sides, so sessions near midnight see their
//! neighbours; each day still only owns the time inside its own bounds.

use std::collections::BTreeMap;

use chrono::{NaiveDate, TimeDelta};
use secretary_shared::Event;
use secretary_shared::rules::{Rule, Signal};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::model::{
    Adjustment, AdjustmentKind, Context, DAY_MARGIN_SECS, DayEvent, RuleRow, Settings, Time,
    day_bounds, parse_tz,
};

/// Event types the calculation reads.
pub const INPUT_TYPES: &[&str] = &[
    "presence.active",
    "window.focus",
    "editor.workspace_open",
    "editor.file_focus",
    "editor.edit_burst",
    "editor.save",
    "editor.debug_session",
    "git.commit",
    "git.checkout",
    "git.merge",
];

/// Set the tenant for the rest of the transaction (`SET LOCAL` semantics).
pub async fn set_user(conn: &mut PgConnection, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.user_id', $1, true)")
        .bind(user_id.to_string())
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LoadStats {
    pub events: usize,
    /// Stored events this engine version cannot read (counted, not logged).
    pub skipped: usize,
}

pub async fn load(
    conn: &mut PgConnection,
    user_id: i64,
    local_day: NaiveDate,
) -> Result<(Context, LoadStats), sqlx::Error> {
    let time_zone: String = sqlx::query_scalar("SELECT time_zone FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?
        .unwrap_or_else(|| "UTC".to_owned());
    let day = day_bounds(parse_tz(&time_zone), local_day);
    let margin = TimeDelta::seconds(DAY_MARGIN_SECS);
    let (lo, hi) = (day.start - margin, day.end + margin);

    // Intervals are cut by the agent (5 min), so starting a day earlier is
    // ample; the device-time bounds let Postgres prune partitions (clock
    // offsets are at most 24 h).
    let rows = sqlx::query(
        "SELECT event_id, device_id, event_type, source, schema_version,
                server_adjusted_start, server_adjusted_end, project_hint,
                repository_root, repository_remote, working_directory, metadata
           FROM activity_events
          WHERE user_id = $1
            AND server_adjusted_start < $3
            AND server_adjusted_start >= $2 - interval '1 day'
            AND COALESCE(server_adjusted_end, server_adjusted_start) > $2
            AND timestamp_start >= $2 - interval '2 days'
            AND timestamp_start < $3 + interval '1 day'
            AND event_type = ANY($4)
          ORDER BY server_adjusted_start, event_id",
    )
    .bind(user_id)
    .bind(lo)
    .bind(hi)
    .bind(INPUT_TYPES)
    .fetch_all(&mut *conn)
    .await?;

    let mut stats = LoadStats::default();
    let mut events = Vec::with_capacity(rows.len());
    for r in &rows {
        match decode_event(r) {
            Ok(Some(e)) => events.push(e),
            Ok(None) => stats.skipped += 1,
            Err(e) => return Err(e),
        }
    }
    stats.events = events.len();

    let rules = sqlx::query(
        "SELECT m.id, m.project_id, m.signal, m.pattern, m.priority, m.user_id IS NULL AS shared
           FROM project_mappings m
           JOIN projects p ON p.id = m.project_id
          WHERE p.archived_at IS NULL AND (m.user_id = $1 OR m.user_id IS NULL)
          ORDER BY m.id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .filter_map(|r| {
        let signal: String = r.try_get("signal").ok()?;
        let signal: Signal = serde_json::from_value(Value::String(signal)).ok()?;
        Some(RuleRow {
            rule: Rule {
                id: r.try_get("id").ok()?,
                project_id: r.try_get("project_id").ok()?,
                signal,
                pattern: r.try_get("pattern").ok()?,
                priority: r.try_get("priority").ok()?,
            },
            shared: r.try_get("shared").ok()?,
        })
    })
    .collect();

    let overrides: BTreeMap<Uuid, Option<i64>> = sqlx::query(
        "SELECT event_id, project_id FROM assignment_overrides
          WHERE user_id = $1 AND local_day BETWEEN $2 AND $3",
    )
    .bind(user_id)
    .bind(local_day.pred_opt().unwrap_or(local_day))
    .bind(local_day.succ_opt().unwrap_or(local_day))
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .filter_map(|r| Some((r.try_get("event_id").ok()?, r.try_get("project_id").ok()?)))
    .collect();

    // Undone corrections keep their row with `deleted_at` set.
    let adjustments: Vec<Adjustment> = sqlx::query(
        "SELECT id, kind, starts_at, ends_at, project_id, from_project_id FROM time_adjustments
          WHERE user_id = $1 AND local_day = $2 AND deleted_at IS NULL
          ORDER BY id",
    )
    .bind(user_id)
    .bind(local_day)
    .fetch_all(&mut *conn)
    .await?
    .iter()
    .filter_map(|r| {
        let kind = match r.try_get::<String, _>("kind").ok()?.as_str() {
            "add" => AdjustmentKind::Add,
            "remove" => AdjustmentKind::Remove,
            "reassign" => AdjustmentKind::Reassign,
            _ => return None,
        };
        Some(Adjustment {
            id: r.try_get("id").ok()?,
            kind,
            start: r.try_get("starts_at").ok()?,
            end: r.try_get("ends_at").ok()?,
            project_id: r.try_get("project_id").ok()?,
            from_project_id: r.try_get("from_project_id").ok()?,
        })
    })
    .collect();

    Ok((
        Context {
            local_day,
            time_zone,
            events,
            rules,
            overrides,
            adjustments,
            settings: Settings::default(),
        },
        stats,
    ))
}

/// A stored row back into the shared [`Event`] type, with server-adjusted
/// timestamps. `None` when this version cannot parse it.
fn decode_event(r: &sqlx::postgres::PgRow) -> Result<Option<DayEvent>, sqlx::Error> {
    let start: Time = r.try_get("server_adjusted_start")?;
    let end: Option<Time> = r.try_get("server_adjusted_end")?;
    let root: Option<String> = r.try_get("repository_root")?;
    let remote: Option<String> = r.try_get("repository_remote")?;
    let repository = if root.is_some() || remote.is_some() {
        json!({ "root_path": root, "remote_url": remote })
    } else {
        Value::Null
    };
    let wire = json!({
        "event_id": r.try_get::<Uuid, _>("event_id")?,
        "schema_version": r.try_get::<i16, _>("schema_version")?,
        "timestamp_start": start,
        "timestamp_end": end,
        "source": r.try_get::<String, _>("source")?,
        "event_type": r.try_get::<String, _>("event_type")?,
        "project_hint": r.try_get::<Option<String>, _>("project_hint")?,
        "repository": repository,
        "working_directory": r.try_get::<Option<String>, _>("working_directory")?,
        "metadata": r.try_get::<Value, _>("metadata")?,
    });
    let device_id: i64 = r.try_get("device_id")?;
    Ok(serde_json::from_value::<Event>(wire)
        .ok()
        .map(|event| DayEvent { device_id, event }))
}
