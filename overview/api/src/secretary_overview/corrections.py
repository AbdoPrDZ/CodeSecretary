"""Session detail, corrections and the inbox (OV-8, OV-9, OV-10).

Corrections are stored as engine inputs and never as edits to derived rows:

- **Assign events** → ``assignment_overrides`` (event → project, or "not
  project work"). Assigning a session assigns its evidence events.
- **Time-range corrections** → ``time_adjustments``: ``reassign`` (move or
  split a session), ``remove`` (trim or delete), ``add`` (manual time).
  Anchored to time and project, so they survive recalculation; undo sets
  ``deleted_at``.

Every change queues its local days to run now; the engine recalculates them
and the dashboard shows "recalculating" until it has. All queries run in the
caller's RLS transaction.
"""

from __future__ import annotations

from collections import Counter
from datetime import date, datetime, time, timedelta
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

ADJUSTMENT_KINDS = ('add', 'remove', 'reassign')
#: Most events one assign request may touch.
MAX_ASSIGN_EVENTS = 5000
#: Most evidence events a session detail returns.
MAX_DETAIL_EVENTS = 500
#: Most unassigned sessions the inbox reads.
MAX_INBOX_SESSIONS = 1000
#: Longest single correction.
MAX_ADJUSTMENT = timedelta(hours=24)
#: Device timestamps (the partition key) are within a day of adjusted ones;
#: evidence intervals may start a little before the session.
_PARTITION_SLACK = timedelta(days=2)


class CorrectionError(Exception):
  def __init__(self, status: int, code: str):
    super().__init__(code)
    self.status, self.code = status, code


def user_zone(conn, user_id: int) -> ZoneInfo:
  row = conn.execute('SELECT time_zone FROM users WHERE id = %s', [user_id]).fetchone()
  try:
    return ZoneInfo((row or {}).get('time_zone') or 'UTC')
  except (ZoneInfoNotFoundError, ValueError):
    return ZoneInfo('UTC')


def enqueue_days(conn, user_id: int, days, reason: str) -> None:
  """Queue these local days to run now (a queued job is pulled forward)."""

  days = sorted(set(days))
  if not days:
    return
  conn.execute(
    """INSERT INTO processing_jobs (user_id, local_day, reason, status, run_after, attempts)
       SELECT %s, d, %s, 'queued', now(), 0 FROM unnest(%s::date[]) AS d
       ON CONFLICT (user_id, local_day) WHERE status = 'queued'
       DO UPDATE SET run_after = LEAST(processing_jobs.run_after, EXCLUDED.run_after),
                     reason = EXCLUDED.reason, updated_at = now()""",
    [user_id, reason, days],
  )


def _projects_exist(conn, ids) -> bool:
  ids = {i for i in ids if i is not None}
  if not ids:
    return True
  row = conn.execute('SELECT count(*) AS n FROM projects WHERE id = ANY(%s)', [list(ids)]).fetchone()
  return row['n'] == len(ids)


# -- session detail (OV-8) ----------------------------------------------------


def session_detail(conn, session_id: int) -> dict | None:
  s = conn.execute(
    """SELECT s.id, s.local_day, s.project_id, p.name AS project_name, p.color AS project_color,
              s.kind, s.starts_at, s.ends_at, s.low_confidence, s.estimated, s.manual, s.algorithm_version
       FROM work_sessions s LEFT JOIN projects p ON p.id = s.project_id
       WHERE s.id = %s""",
    [session_id],
  ).fetchone()
  if s is None:
    return None
  total = conn.execute('SELECT count(*) AS n FROM session_evidence WHERE session_id = %s',
                       [session_id]).fetchone()['n']
  events = conn.execute(
    """SELECT e.event_id, e.device_id, d.name AS device_name, e.event_type,
              e.server_adjusted_start AS start, e.server_adjusted_end AS "end",
              e.repository_root, e.repository_remote, e.working_directory, e.metadata,
              a.project_id AS assigned_project_id, a.method, a.confidence, a.rule_id,
              (o.event_id IS NOT NULL) AS overridden
       FROM session_evidence se
       JOIN activity_events e ON e.event_id = se.event_id AND e.user_id = se.user_id
            AND e.timestamp_start >= %(lo)s AND e.timestamp_start < %(hi)s
       LEFT JOIN devices d ON d.id = e.device_id
       LEFT JOIN event_assignments a ON a.user_id = e.user_id AND a.event_id = e.event_id
       LEFT JOIN assignment_overrides o ON o.user_id = e.user_id AND o.event_id = e.event_id
       WHERE se.session_id = %(sid)s
       ORDER BY e.server_adjusted_start, e.event_id
       LIMIT %(limit)s""",
    {'sid': session_id, 'lo': s['starts_at'] - _PARTITION_SLACK, 'hi': s['ends_at'] + _PARTITION_SLACK,
     'limit': MAX_DETAIL_EVENTS},
  ).fetchall()
  rule_ids = sorted({e['rule_id'] for e in events if e['rule_id'] is not None})
  rules = conn.execute(
    'SELECT id, project_id, signal, pattern FROM project_mappings WHERE id = ANY(%s)', [rule_ids],
  ).fetchall() if rule_ids else []
  methods = Counter(e['method'] or 'unassigned' for e in events)
  return {
    'session': s,
    'events': events,
    'evidence_total': total,
    'truncated': total > len(events),
    'methods': dict(methods),
    'rules': rules,
  }


# -- assignments (OV-9: assign events / sessions) ------------------------------


def assign(conn, user_id: int, event_ids: list[str], session_ids: list[int], project_id: int | None,
           clear: bool) -> dict:
  """Override events (and sessions' evidence events) to ``project_id``; ``None``
  = not project work; ``clear`` = back to automatic."""

  if not clear and not _projects_exist(conn, [project_id]):
    raise CorrectionError(404, 'not_found')
  ids = set(event_ids)
  if session_ids:
    ids.update(str(r['event_id']) for r in conn.execute(
      'SELECT event_id FROM session_evidence WHERE session_id = ANY(%s)', [session_ids]).fetchall())
  if len(ids) > MAX_ASSIGN_EVENTS:
    raise CorrectionError(400, 'too_many_events')
  if not ids:
    return {'events': 0}
  # Only the caller's events exist under RLS; each override belongs to the
  # event's local day in the user's zone.
  rows = conn.execute(
    """SELECT e.event_id, min((e.server_adjusted_start AT TIME ZONE u.time_zone)::date) AS local_day
       FROM activity_events e JOIN users u ON u.id = e.user_id
       WHERE e.event_id = ANY(%s::uuid[])
       GROUP BY e.event_id""",
    [sorted(ids)],
  ).fetchall()
  if not rows:
    return {'events': 0}
  found = [str(r['event_id']) for r in rows]
  days = {r['local_day'] for r in rows}
  # The previous override's day may differ (time zone changed since).
  days.update(r['local_day'] for r in conn.execute(
    'SELECT local_day FROM assignment_overrides WHERE event_id = ANY(%s::uuid[])', [found]).fetchall())
  if clear:
    conn.execute('DELETE FROM assignment_overrides WHERE event_id = ANY(%s::uuid[])', [found])
  else:
    conn.execute(
      """INSERT INTO assignment_overrides (user_id, event_id, local_day, project_id)
         SELECT %s, r.event_id, r.local_day, %s
         FROM unnest(%s::uuid[], %s::date[]) AS r(event_id, local_day)
         ON CONFLICT (user_id, event_id) DO UPDATE
            SET project_id = EXCLUDED.project_id, local_day = EXCLUDED.local_day, updated_at = now()""",
      [user_id, project_id, found, [r['local_day'] for r in rows]],
    )
  # Neighbouring days see the event through their margin.
  enqueue_days(conn, user_id, {d + timedelta(days=k) for d in days for k in (-1, 0, 1)}, 'correction')
  return {'events': len(found)}


# -- time-range corrections (OV-9: reassign, split, trim, delete, add) ---------


def list_adjustments(conn, local_day: date) -> list[dict]:
  return conn.execute(
    """SELECT a.id, a.local_day, a.kind, a.starts_at, a.ends_at, a.project_id, p.name AS project_name,
              a.from_project_id, fp.name AS from_project_name, a.note, a.created_at
       FROM time_adjustments a
       LEFT JOIN projects p ON p.id = a.project_id
       LEFT JOIN projects fp ON fp.id = a.from_project_id
       WHERE a.local_day = %s AND a.deleted_at IS NULL
       ORDER BY a.id""",
    [local_day],
  ).fetchall()


def add_adjustment(conn, user_id: int, kind: str, start: datetime, end: datetime,
                   project_id: int | None, from_project_id: int | None, note: str | None) -> list[int]:
  """Store a correction, split at local midnights (one row per local day)."""

  if kind not in ADJUSTMENT_KINDS:
    raise CorrectionError(400, 'invalid_kind')
  if start.tzinfo is None or end.tzinfo is None or end <= start or end - start > MAX_ADJUSTMENT:
    raise CorrectionError(400, 'invalid_range')
  if kind in ('add', 'reassign') and project_id is None:
    raise CorrectionError(400, 'project_required')
  if kind == 'remove':
    project_id = None
  if kind == 'add':
    from_project_id = None
  if not _projects_exist(conn, [project_id, from_project_id]):
    raise CorrectionError(404, 'not_found')
  zone = user_zone(conn, user_id)
  ids, days = [], []
  cursor = start
  while cursor < end:
    day = cursor.astimezone(zone).date()
    midnight = datetime.combine(day + timedelta(days=1), time.min, zone)
    stop = min(end, midnight)
    row = conn.execute(
      """INSERT INTO time_adjustments (user_id, local_day, kind, starts_at, ends_at, project_id,
                                       from_project_id, note)
         VALUES (%s, %s, %s, %s, %s, %s, %s, %s) RETURNING id""",
      [user_id, day, kind, cursor, stop, project_id, from_project_id, note],
    ).fetchone()
    ids.append(row['id'])
    days.append(day)
    cursor = stop
  enqueue_days(conn, user_id, days, 'correction')
  return ids


def undo_adjustment(conn, user_id: int, adjustment_id: int) -> bool:
  row = conn.execute(
    """UPDATE time_adjustments SET deleted_at = now(), updated_at = now()
       WHERE id = %s AND deleted_at IS NULL RETURNING local_day""",
    [adjustment_id],
  ).fetchone()
  if row is None:
    return False
  enqueue_days(conn, user_id, [row['local_day']], 'correction')
  return True


# -- inbox (OV-10) -------------------------------------------------------------


def _label(evidence: list[dict]) -> tuple[str, str]:
  """What an unassigned session is "about": its most common folder, else app.

  The pair is also what a rule for it would use (signal, pattern).
  """

  paths = Counter(p for e in evidence for p in (e['root'], e['workspace']) if p)
  if paths:
    return 'path', paths.most_common(1)[0][0]
  apps = Counter(e['app'] for e in evidence if e['app'])
  if apps:
    return 'app', apps.most_common(1)[0][0]
  return 'other', evidence[0]['event_type'] if evidence else 'unknown'


def inbox(conn, first: date, last: date) -> dict:
  """Unassigned sessions of the range, grouped by what they are about."""

  sessions = conn.execute(
    """SELECT id, local_day, starts_at, ends_at
       FROM work_sessions
       WHERE project_id IS NULL AND kind = 'active' AND NOT manual AND local_day BETWEEN %s AND %s
       ORDER BY starts_at
       LIMIT %s""",
    [first, last, MAX_INBOX_SESSIONS + 1],
  ).fetchall()
  truncated = len(sessions) > MAX_INBOX_SESSIONS
  sessions = sessions[:MAX_INBOX_SESSIONS]
  if not sessions:
    return {'groups': [], 'total_seconds': 0, 'sessions': 0, 'truncated': False}
  lo = min(s['starts_at'] for s in sessions) - _PARTITION_SLACK
  hi = max(s['ends_at'] for s in sessions) + _PARTITION_SLACK
  evidence: dict[int, list[dict]] = {}
  for r in conn.execute(
    """SELECT se.session_id, e.event_type, e.metadata->>'app' AS app, e.metadata->>'title' AS title,
              e.repository_root AS root, e.metadata->>'workspace_path' AS workspace
       FROM session_evidence se
       JOIN activity_events e ON e.event_id = se.event_id AND e.user_id = se.user_id
            AND e.timestamp_start >= %s AND e.timestamp_start < %s
       WHERE se.session_id = ANY(%s)""",
    [lo, hi, [s['id'] for s in sessions]],
  ).fetchall():
    evidence.setdefault(r['session_id'], []).append(r)

  groups: dict[tuple[str, str], dict] = {}
  for s in sessions:
    ev = evidence.get(s['id'], [])
    signal, value = _label(ev)
    seconds = int((s['ends_at'] - s['starts_at']).total_seconds())
    g = groups.setdefault((signal, value), {'signal': signal, 'value': value, 'seconds': 0,
                                            'sessions': [], 'titles': Counter()})
    g['seconds'] += seconds
    g['titles'].update(e['title'] for e in ev if e['title'])
    g['sessions'].append({'id': s['id'], 'day': s['local_day'], 'start': s['starts_at'],
                          'end': s['ends_at'], 'seconds': seconds})
  ordered = sorted(groups.values(), key=lambda g: (-g['seconds'], g['value']))
  for g in ordered:
    g['titles'] = [t for t, _ in g['titles'].most_common(5)]
  return {
    'groups': ordered,
    'total_seconds': sum(g['seconds'] for g in ordered),
    'sessions': len(sessions),
    'truncated': truncated,
  }
