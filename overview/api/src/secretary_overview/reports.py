"""Reports and CSV export (OV-11).

Every number here adds up the engine's ``work_sessions`` (active kind only), the
same rows the Time page shows, so a CSV total always equals the dashboard total.
Nothing here derives time from events.
"""

from __future__ import annotations

import csv
import io
from datetime import UTC, date, datetime, timedelta
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

#: Longest range a report or export covers.
MAX_REPORT_DAYS = 366
#: Most sessions a session-level export writes (one user-year is far below this).
MAX_EXPORT_SESSIONS = 100_000
GROUPS = ('project', 'day', 'session')
#: Leading characters a spreadsheet would run as a formula (CSV injection).
_FORMULA = ('=', '+', '-', '@', '\t', '\r')


class ReportError(Exception):
  def __init__(self, code: str):
    super().__init__(code)
    self.code = code


def parse_range(start: str, end: str, max_days: int) -> tuple[date, date]:
  try:
    first, last = date.fromisoformat(start), date.fromisoformat(end)
  except ValueError:
    raise ReportError('invalid_range') from None
  if last < first or (last - first).days >= max_days:
    raise ReportError('invalid_range')
  return first, last


def load_summary(conn, user_id: int, first: date, last: date) -> dict:
  """Per project and day totals of local days ``first``..``last`` (RLS-scoped ``conn``)."""

  rows = conn.execute(
    """SELECT local_day, project_id, count(*) AS sessions,
              sum(extract(epoch FROM ends_at - starts_at))::bigint AS seconds
       FROM work_sessions
       WHERE local_day BETWEEN %s AND %s AND kind = 'active'
       GROUP BY local_day, project_id""",
    [first, last],
  ).fetchall()
  projects = {p['id']: p for p in conn.execute('SELECT id, name, color, archived_at FROM projects').fetchall()}
  user = conn.execute('SELECT time_zone FROM users WHERE id = %s', [user_id]).fetchone()
  return summarize(first, last, rows, projects, user['time_zone'], job_state(conn, user_id, first, last))


def job_state(conn, user_id: int, first: date, last: date) -> dict:
  """Engine queue state for the user's days (``processing_jobs`` has no RLS: filter by user)."""

  row = conn.execute(
    """SELECT count(*) FILTER (WHERE status IN ('queued', 'running')) AS pending,
              count(*) FILTER (WHERE status = 'failed') AS failed,
              max(updated_at) FILTER (WHERE status = 'done') AS last_done
       FROM processing_jobs
       WHERE user_id = %s AND local_day BETWEEN %s AND %s""",
    [user_id, first, last],
  ).fetchone()
  return {'pending_days': row['pending'], 'failed_days': row['failed'],
          'calculated_at': _iso(row['last_done'])}


def summarize(first: date, last: date, rows: list[dict], projects: dict, time_zone: str, jobs: dict) -> dict:
  """Adds up the engine's session lengths (reporting); never derives time from events."""

  days: dict[date, dict] = {first + timedelta(days=i): {} for i in range((last - first).days + 1)}
  totals: dict[int | None, dict] = {}
  for r in rows:
    key = r['project_id']
    days[r['local_day']][key] = days[r['local_day']].get(key, 0) + r['seconds']
    t = totals.setdefault(key, {'seconds': 0, 'sessions': 0, 'days': 0})
    t['seconds'] += r['seconds']
    t['sessions'] += r['sessions']
    t['days'] += 1

  def label(pid: int) -> dict:
    p = projects.get(pid) or {}
    return {'project_id': pid, 'name': p.get('name'), 'color': p.get('color'),
            'archived': p.get('archived_at') is not None}

  project_totals = sorted(
    ({**label(pid), **t} for pid, t in totals.items() if pid is not None),
    key=lambda p: (-p['seconds'], p['name'] or ''),
  )
  unassigned = totals.get(None, {'seconds': 0, 'sessions': 0, 'days': 0})
  return {
    'start': first.isoformat(),
    'end': last.isoformat(),
    'time_zone': time_zone,
    'total_seconds': sum(t['seconds'] for t in totals.values()),
    'assigned_seconds': sum(p['seconds'] for p in project_totals),
    'unassigned_seconds': unassigned['seconds'],
    'unassigned_sessions': unassigned['sessions'],
    'unassigned_days': unassigned['days'],
    'projects': project_totals,
    'days': [{
      'day': d.isoformat(),
      'total_seconds': sum(v.values()),
      'unassigned_seconds': v.get(None, 0),
      'projects': [{'project_id': pid, 'seconds': sec} for pid, sec in sorted(
        ((pid, sec) for pid, sec in v.items() if pid is not None), key=lambda x: -x[1])],
    } for d, v in days.items()],
    **jobs,
  }


def filtered(summary: dict, project_ids: set[int] | None, unassigned: bool) -> dict:
  """The summary limited to some projects (None = all) and with or without
  unassigned time; totals are added up again here, not in the browser."""

  def wanted(pid: int | None) -> bool:
    return unassigned if pid is None else (project_ids is None or pid in project_ids)

  projects = [p for p in summary['projects'] if wanted(p['project_id'])]
  days = []
  for d in summary['days']:
    cells = [c for c in d['projects'] if wanted(c['project_id'])]
    free = d['unassigned_seconds'] if unassigned else 0
    days.append({**d, 'projects': cells, 'unassigned_seconds': free,
                 'total_seconds': sum(c['seconds'] for c in cells) + free})
  assigned = sum(p['seconds'] for p in projects)
  free = summary['unassigned_seconds'] if unassigned else 0
  out = {**summary, 'projects': projects, 'days': days, 'assigned_seconds': assigned,
         'unassigned_seconds': free, 'total_seconds': assigned + free,
         'filter': {'projects': sorted(project_ids) if project_ids is not None else None,
                    'unassigned': unassigned}}
  if not unassigned:
    out['unassigned_sessions'] = out['unassigned_days'] = 0
  return out


def export_csv(conn, user_id: int, first: date, last: date, group: str,
               project_ids: set[int] | None, unassigned: bool) -> str:
  """The report as CSV text: the rows of :func:`filtered`, so the total row is
  the dashboard's total for the same range and filter."""

  if group not in GROUPS:
    raise ReportError('invalid_group')
  summary = filtered(load_summary(conn, user_id, first, last), project_ids, unassigned)
  names = {p['project_id']: p['name'] or 'Deleted project' for p in summary['projects']}

  def wanted(pid: int | None) -> bool:
    return unassigned if pid is None else (project_ids is None or pid in project_ids)

  def name(pid: int | None) -> str:
    return 'Unassigned' if pid is None else names.get(pid, 'Deleted project')

  out = io.StringIO()
  w = csv.writer(out, lineterminator='\r\n')
  total = 0
  if group == 'project':
    w.writerow(['project', 'sessions', 'days', 'hours', 'seconds'])
    rows = [(p['project_id'], p['sessions'], p['days'], p['seconds']) for p in summary['projects']]
    if summary['unassigned_seconds']:
      rows.append((None, summary['unassigned_sessions'], summary['unassigned_days'], summary['unassigned_seconds']))
    for pid, n, d, sec in rows:
      w.writerow([_cell(name(pid)), n, d, _hours(sec), sec])
      total += sec
    w.writerow(['Total', '', '', _hours(total), total])
  elif group == 'day':
    w.writerow(['day', 'project', 'hours', 'seconds'])
    for d in summary['days']:
      cells = [(p['project_id'], p['seconds']) for p in d['projects']]
      if d['unassigned_seconds']:
        cells.append((None, d['unassigned_seconds']))
      for pid, sec in cells:
        w.writerow([d['day'], _cell(name(pid)), _hours(sec), sec])
        total += sec
    w.writerow(['Total', '', _hours(total), total])
  else:
    zone = _zone(summary['time_zone'])
    w.writerow(['day', 'project', 'start', 'end', 'hours', 'seconds', 'manual'])
    rows = conn.execute(
      """SELECT local_day, project_id, starts_at, ends_at, manual,
                extract(epoch FROM ends_at - starts_at)::bigint AS seconds
         FROM work_sessions
         WHERE local_day BETWEEN %s AND %s AND kind = 'active'
         ORDER BY starts_at, id
         LIMIT %s""",
      [first, last, MAX_EXPORT_SESSIONS],
    ).fetchall()
    for r in rows:
      if wanted(r['project_id']):
        w.writerow([r['local_day'].isoformat(), _cell(name(r['project_id'])),
                    _local(r['starts_at'], zone), _local(r['ends_at'], zone),
                    _hours(r['seconds']), r['seconds'], 'yes' if r['manual'] else ''])
        total += r['seconds']
    w.writerow(['Total', '', '', '', _hours(total), total, ''])
  if total != summary['total_seconds']:  # never expected: the rows are the report's own rows
    raise RuntimeError('export rows do not add up to the report total')
  return out.getvalue()


def _hours(seconds: int) -> str:
  return f'{seconds / 3600:.2f}'


def _cell(text: str) -> str:
  """Keeps a user-chosen name from running as a spreadsheet formula."""

  return "'" + text if text.startswith(_FORMULA) else text


def _local(value: datetime, zone: ZoneInfo) -> str:
  return value.astimezone(zone).isoformat(timespec='seconds')


def _zone(name: str | None) -> ZoneInfo:
  try:
    return ZoneInfo(name or 'UTC')
  except (ZoneInfoNotFoundError, ValueError):
    return ZoneInfo('UTC')


def _iso(value):
  if value is None:
    return None
  return value.astimezone(UTC).isoformat().replace('+00:00', 'Z')
