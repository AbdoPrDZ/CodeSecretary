"""HTTP routes of the dashboard API.

- ``/auth/*``: sign in (Keycloak, PKCE), callback, sign out.
- ``/api/*``: JSON for the web app. Needs the session cookie; mutations also
  need ``X-CSRF-Token`` (from ``/api/me``) and, when sent, a same-origin
  ``Origin``. Errors are ``{"error": code}``.
- Everything else: the built web app (``STATIC_DIR``), with ``index.html`` for
  client-side routes.

Every query runs in the signed-in user's RLS context (tenant identity comes
from the session, never from the request). Raw activity is returned as
stored; this service never computes time: ``/api/time`` and
``/api/sessions`` only read and add up the engine's work sessions.
"""

from __future__ import annotations

import asyncio
import hmac
import logging
import re
from contextlib import asynccontextmanager
from datetime import UTC, date, datetime, time, timedelta
from decimal import Decimal
from pathlib import Path
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from fastapi import Depends, FastAPI, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.middleware.gzip import GZipMiddleware
from fastapi.exceptions import RequestValidationError
from fastapi.responses import FileResponse, JSONResponse, RedirectResponse, Response
from pydantic import BaseModel, ConfigDict, Field
from secretary_orm import Environment

from secretary_overview import __version__, rules, sessions
from secretary_overview.config import Config
from secretary_overview.oidc import Oidc, SignInError
from secretary_warehouse.db import ping, user_transaction

logger = logging.getLogger('secretary_overview.http')

LOGIN_COOKIE = 'sec_login'
#: Most events returned for one day; the response says when it was cut.
MAX_DAY_EVENTS = 5000
EXCLUSION_KINDS = ('path', 'app', 'domain', 'time_window')
_HH_MM = re.compile(r'^([01]\d|2[0-3]):[0-5]\d$')
_COLOR = re.compile(r'^#[0-9a-fA-F]{6}$')
_CURRENCY = re.compile(r'^[A-Z]{3}$')
#: Days a rule preview looks back, and days re-calculated when rules change
#: (OVERVIEW plan: "last 30 by default").
PREVIEW_DAYS = 30
RECALC_DAYS = 30
#: Longest range ``/api/time`` adds up in one request.
MAX_TIME_RANGE_DAYS = 92
#: Most distinct evidence groups a preview reads; the response says when it was cut.
MAX_PREVIEW_GROUPS = 20000
_ALL_EVIDENCE = [c for s in rules.SIGNALS for c in rules.EVIDENCE_COLUMNS[s]]


class ApiError(Exception):
  def __init__(self, status: int, code: str):
    super().__init__(code)
    self.status, self.code = status, code


class RenameDevice(BaseModel):
  model_config = ConfigDict(extra='forbid')
  name: str = Field(min_length=1, max_length=255)


class NewExclusion(BaseModel):
  model_config = ConfigDict(extra='forbid')
  kind: str
  pattern: str | None = Field(default=None, max_length=500)
  start: str | None = None
  end: str | None = None
  days: list[int] | None = None


class TimeZoneBody(BaseModel):
  model_config = ConfigDict(extra='forbid')
  time_zone: str = Field(min_length=1, max_length=64)


class ProjectFields(BaseModel):
  model_config = ConfigDict(extra='forbid')
  name: str | None = Field(default=None, min_length=1, max_length=200)
  color: str | None = Field(default=None, max_length=16)
  client_name: str | None = Field(default=None, max_length=200)
  billable: bool | None = None
  hourly_rate: Decimal | None = Field(default=None, ge=0, lt=Decimal('1e10'), decimal_places=2)
  currency: str | None = Field(default=None, max_length=3)
  archived: bool | None = None


class NewRule(BaseModel):
  model_config = ConfigDict(extra='forbid')
  signal: str
  pattern: str = Field(max_length=rules.MAX_PATTERN + 50)
  priority: int = Field(default=0, ge=-100, le=100)


class RulePreview(NewRule):
  project_id: int | None = None
  days: int = Field(default=PREVIEW_DAYS, ge=1, le=90)


def create_app(env: Environment, config: Config, oidc: Oidc | None = None) -> FastAPI:
  oidc = oidc or Oidc(config)
  db = env.db

  @asynccontextmanager
  async def lifespan(_app: FastAPI):
    yield
    env.close()

  app = FastAPI(title='CodeSecretary Overview', version=__version__, lifespan=lifespan,
                docs_url=None, redoc_url=None, openapi_url=None)

  # -- cross-cutting -------------------------------------------------------

  # A day of activity is up to 5,000 events of JSON: compress what is worth it.
  app.add_middleware(GZipMiddleware, minimum_size=1024, compresslevel=5)

  @app.middleware('http')
  async def security_headers(request: Request, call_next):
    response = await call_next(request)
    response.headers.setdefault('X-Content-Type-Options', 'nosniff')
    response.headers.setdefault('Referrer-Policy', 'same-origin')
    response.headers.setdefault('X-Frame-Options', 'DENY')
    response.headers.setdefault(
      'Content-Security-Policy',
      "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; "
      "frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
    )
    if config.secure_cookies:
      response.headers.setdefault('Strict-Transport-Security', 'max-age=31536000')
    if request.url.path.startswith(('/api/', '/auth/')):
      response.headers.setdefault('Cache-Control', 'no-store')
    return response

  @app.exception_handler(ApiError)
  async def _api_error(_request: Request, exc: ApiError):
    return JSONResponse({'error': exc.code}, status_code=exc.status)

  @app.exception_handler(RequestValidationError)
  async def _invalid(_request: Request, _exc: RequestValidationError):
    return JSONResponse({'error': 'invalid_body'}, status_code=400)

  def current(request: Request) -> sessions.Session:
    session = sessions.load(db, request.cookies.get(sessions.COOKIE))
    if session is None:
      raise ApiError(401, 'signed_out')
    return session

  def mutating(request: Request, session: sessions.Session = Depends(current)) -> sessions.Session:
    origin = request.headers.get('origin')
    if origin is not None and origin != config.origin:
      raise ApiError(403, 'bad_origin')
    token = request.headers.get('x-csrf-token', '')
    if not hmac.compare_digest(token, session.csrf_token):
      raise ApiError(403, 'bad_csrf')
    return session

  # -- health --------------------------------------------------------------

  @app.get('/healthz')
  def healthz():
    return {'status': 'ok'}

  @app.get('/readyz')
  async def readyz():
    try:
      await asyncio.wait_for(run_in_threadpool(ping, db), 2.0)
    except Exception as exc:  # noqa: BLE001 - kind only
      logger.warning('readiness check failed', extra={'error_kind': type(exc).__name__})
      return JSONResponse({'status': 'unavailable'}, status_code=503)
    return {'status': 'ready'}

  # -- sign-in -------------------------------------------------------------

  @app.get('/auth/login')
  def login(next: str | None = None):  # noqa: A002 - query parameter name
    try:
      state, cookie = oidc.new_login(next)
      url = oidc.authorize_url(state)
    except SignInError as exc:
      logger.warning('sign-in unavailable', extra={'error_kind': str(exc)})
      return JSONResponse({'error': 'auth_unavailable'}, status_code=503)
    response = RedirectResponse(url, status_code=302)
    response.set_cookie(LOGIN_COOKIE, cookie, max_age=600, path='/auth', httponly=True,
                        secure=config.secure_cookies, samesite='lax')
    return response

  @app.get('/auth/callback')
  def callback(request: Request, code: str | None = None, state: str | None = None,
               error: str | None = None):
    if error:
      return RedirectResponse('/?signin=cancelled', status_code=302)
    try:
      login_state = oidc.read_login(request.cookies.get(LOGIN_COOKIE), state)
      signed_in = oidc.finish(code, login_state)
    except SignInError as exc:
      logger.info('sign-in rejected', extra={'error_kind': str(exc)})
      return JSONResponse({'error': 'sign_in_failed', 'reason': str(exc)}, status_code=400)
    user_id = sessions.resolve_user(db, signed_in.issuer, signed_in.subject, signed_in.email,
                                    signed_in.name)
    token = sessions.create(db, user_id, config.session_hours, signed_in.session_id,
                            signed_in.id_token)
    logger.info('signed in', extra={'user_id': user_id})
    response = RedirectResponse(login_state.next, status_code=302)
    response.delete_cookie(LOGIN_COOKIE, path='/auth')
    response.set_cookie(sessions.COOKIE, token, max_age=config.session_hours * 3600, path='/',
                        httponly=True, secure=config.secure_cookies, samesite='lax')
    return response

  @app.post('/auth/logout')
  def logout(session: sessions.Session = Depends(mutating)):
    sessions.delete(db, session)
    response = JSONResponse({'logout_url': oidc.end_session_url(session.id_token)})
    response.delete_cookie(sessions.COOKIE, path='/')
    return response

  # -- API -----------------------------------------------------------------

  @app.get('/api/me')
  def me(session: sessions.Session = Depends(current)):
    with user_transaction(db, session.user_id) as conn:
      user = conn.execute('SELECT email, display_name, time_zone FROM users WHERE id = %s',
                          [session.user_id]).fetchone()
    return {**user, 'csrf_token': session.csrf_token}

  @app.put('/api/me/time_zone')
  def set_time_zone(body: TimeZoneBody, session: sessions.Session = Depends(mutating)):
    """The zone the engine cuts local days in (OV-13); recalculates recent days."""

    try:
      ZoneInfo(body.time_zone)
    except (ZoneInfoNotFoundError, ValueError):
      raise ApiError(400, 'invalid_time_zone') from None
    with user_transaction(db, session.user_id) as conn:
      old = conn.execute('SELECT time_zone FROM users WHERE id = %s', [session.user_id]).fetchone()
      if old['time_zone'] != body.time_zone:
        conn.execute('UPDATE users SET time_zone = %s, updated_at = now() WHERE id = %s',
                     [body.time_zone, session.user_id])
        _enqueue_recalc(conn, session.user_id, 'time_zone_changed')
    return {'time_zone': body.time_zone}

  @app.get('/api/time')
  def time_summary(start: str, end: str, session: sessions.Session = Depends(current)):
    """The engine's results per project and day for local days ``start``..``end``."""

    try:
      first, last = date.fromisoformat(start), date.fromisoformat(end)
    except ValueError:
      raise ApiError(400, 'invalid_range') from None
    if last < first or (last - first).days >= MAX_TIME_RANGE_DAYS:
      raise ApiError(400, 'invalid_range')
    with user_transaction(db, session.user_id) as conn:
      rows = conn.execute(
        """SELECT local_day, project_id, count(*) AS sessions,
                  sum(extract(epoch FROM ends_at - starts_at))::bigint AS seconds
           FROM work_sessions
           WHERE local_day BETWEEN %s AND %s AND kind = 'active'
           GROUP BY local_day, project_id""",
        [first, last],
      ).fetchall()
      projects = {p['id']: p for p in conn.execute(
        'SELECT id, name, color, archived_at FROM projects').fetchall()}
      user = conn.execute('SELECT time_zone FROM users WHERE id = %s', [session.user_id]).fetchone()
      jobs = _job_state(conn, session.user_id, first, last)
    return _time_summary(first, last, rows, projects, user['time_zone'], jobs)

  @app.get('/api/sessions')
  def day_sessions(day: str, session: sessions.Session = Depends(current)):
    """The engine's sessions of one local day (user's time zone), in time order."""

    try:
      local_day = date.fromisoformat(day)
    except ValueError:
      raise ApiError(400, 'invalid_day') from None
    with user_transaction(db, session.user_id) as conn:
      user = conn.execute('SELECT time_zone FROM users WHERE id = %s', [session.user_id]).fetchone()
      rows = conn.execute(
        """SELECT s.id, s.project_id, p.name AS project_name, p.color AS project_color, s.kind,
                  s.starts_at, s.ends_at, s.low_confidence, s.estimated,
                  (SELECT count(*) FROM session_evidence e WHERE e.session_id = s.id) AS evidence
           FROM work_sessions s LEFT JOIN projects p ON p.id = s.project_id
           WHERE s.local_day = %s
           ORDER BY s.starts_at""",
        [local_day],
      ).fetchall()
      jobs = _job_state(conn, session.user_id, local_day, local_day)
    zone = _zone(user['time_zone'])
    start = datetime.combine(local_day, time.min, zone).astimezone(UTC)
    end = datetime.combine(local_day + timedelta(days=1), time.min, zone).astimezone(UTC)
    return {
      'day': local_day.isoformat(),
      'time_zone': user['time_zone'],
      'range': {'start': _iso(start), 'end': _iso(end)},
      'sessions': [{
        'id': r['id'], 'project_id': r['project_id'], 'project_name': r['project_name'],
        'project_color': r['project_color'], 'kind': r['kind'],
        'start': _iso(r['starts_at']), 'end': _iso(r['ends_at']),
        'seconds': int((r['ends_at'] - r['starts_at']).total_seconds()),
        'low_confidence': r['low_confidence'], 'estimated': r['estimated'], 'evidence': r['evidence'],
      } for r in rows],
      **jobs,
    }

  @app.get('/api/devices')
  def devices(session: sessions.Session = Depends(current)):
    with user_transaction(db, session.user_id) as conn:
      rows = conn.execute(
        """SELECT id, name, os, agent_version, queue_depth, clock_offset_ms,
                  last_seen_at, revoked_at, created_at
           FROM devices ORDER BY revoked_at IS NOT NULL, last_seen_at DESC NULLS LAST, id DESC"""
      ).fetchall()
    return {'devices': [_jsonable(r) for r in rows]}

  @app.patch('/api/devices/{device_id}')
  def rename_device(device_id: int, body: RenameDevice, session: sessions.Session = Depends(mutating)):
    with user_transaction(db, session.user_id) as conn:
      row = conn.execute('UPDATE devices SET name = %s, updated_at = now() WHERE id = %s RETURNING id',
                         [body.name.strip(), device_id]).fetchone()
    if row is None:
      raise ApiError(404, 'not_found')
    return {'ok': True}

  @app.post('/api/devices/{device_id}/revoke')
  def revoke_device(device_id: int, session: sessions.Session = Depends(mutating)):
    with user_transaction(db, session.user_id) as conn:
      row = conn.execute(
        """UPDATE devices SET revoked_at = COALESCE(revoked_at, now()), updated_at = now()
           WHERE id = %s RETURNING revoked_at""",
        [device_id],
      ).fetchone()
    if row is None:
      raise ApiError(404, 'not_found')
    logger.info('device revoked', extra={'device_id': device_id})
    return {'revoked_at': _iso(row['revoked_at'])}

  @app.get('/api/activity')
  def activity(day: str, tz: str = 'UTC', session: sessions.Session = Depends(current)):
    try:
      zone = ZoneInfo(tz)
      local_day = date.fromisoformat(day)
    except (ZoneInfoNotFoundError, ValueError):
      raise ApiError(400, 'invalid_day_or_tz') from None
    start = datetime.combine(local_day, time.min, zone).astimezone(UTC)
    end = datetime.combine(local_day + timedelta(days=1), time.min, zone).astimezone(UTC)
    with user_transaction(db, session.user_id) as conn:
      rows = conn.execute(
        """SELECT event_id, device_id, event_type, source,
                  server_adjusted_start AS start, server_adjusted_end AS "end",
                  project_hint, repository_remote, metadata
           FROM activity_events
           WHERE server_adjusted_start < %(end)s
             AND COALESCE(server_adjusted_end, server_adjusted_start) >= %(start)s
             -- partition pruning: device time is within a day of server time
             AND timestamp_start >= %(start)s - interval '1 day'
             AND timestamp_start < %(end)s + interval '1 day'
           ORDER BY server_adjusted_start, event_id
           LIMIT %(limit)s""",
        {'start': start, 'end': end, 'limit': MAX_DAY_EVENTS + 1},
      ).fetchall()
      device_rows = conn.execute('SELECT id, name, os, revoked_at FROM devices ORDER BY id').fetchall()
    truncated = len(rows) > MAX_DAY_EVENTS
    return {
      'day': local_day.isoformat(),
      'tz': tz,
      'range': {'start': _iso(start), 'end': _iso(end)},
      'devices': [_jsonable(d) for d in device_rows],
      'events': [_jsonable(r) for r in rows[:MAX_DAY_EVENTS]],
      'truncated': truncated,
    }

  @app.get('/api/exclusions')
  def exclusions(session: sessions.Session = Depends(current)):
    with user_transaction(db, session.user_id) as conn:
      rows = conn.execute(
        """SELECT id, kind, pattern, window_start, window_end, days_of_week, created_at
           FROM exclusions ORDER BY kind, id"""
      ).fetchall()
    return {'exclusions': [_exclusion(r) for r in rows]}

  @app.post('/api/exclusions', status_code=201)
  def add_exclusion(body: NewExclusion, session: sessions.Session = Depends(mutating)):
    if body.kind not in EXCLUSION_KINDS:
      raise ApiError(400, 'invalid_kind')
    if body.kind == 'time_window':
      if not (body.start and body.end and _HH_MM.match(body.start) and _HH_MM.match(body.end)):
        raise ApiError(400, 'invalid_window')
      if body.start == body.end:
        raise ApiError(400, 'invalid_window')
      days = sorted(set(body.days or []))
      if any(d < 1 or d > 7 for d in days):
        raise ApiError(400, 'invalid_days')
      values = [session.user_id, 'time_window', None, body.start, body.end,
                _json(days) if days else None]
    else:
      pattern = (body.pattern or '').strip()
      if not pattern:
        raise ApiError(400, 'invalid_pattern')
      values = [session.user_id, body.kind, pattern, None, None, None]
    with user_transaction(db, session.user_id) as conn:
      row = conn.execute(
        """INSERT INTO exclusions (user_id, kind, pattern, window_start, window_end, days_of_week)
           VALUES (%s, %s, %s, %s, %s, %s::jsonb)
           RETURNING id, kind, pattern, window_start, window_end, days_of_week, created_at""",
        values,
      ).fetchone()
    return _exclusion(row)

  @app.delete('/api/exclusions/{exclusion_id}')
  def delete_exclusion(exclusion_id: int, session: sessions.Session = Depends(mutating)):
    with user_transaction(db, session.user_id) as conn:
      row = conn.execute('DELETE FROM exclusions WHERE id = %s RETURNING id', [exclusion_id]).fetchone()
    if row is None:
      raise ApiError(404, 'not_found')
    return {'ok': True}

  # -- projects and rules (OV-5, D17) --------------------------------------
  # Personal rules (user_id = me) only for now; shared team rules are Phase 3.

  @app.get('/api/projects')
  def projects(session: sessions.Session = Depends(current)):
    with user_transaction(db, session.user_id) as conn:
      rows = conn.execute(
        """SELECT id, workspace_id, name, color, client_name, billable, hourly_rate, currency,
                  archived_at, created_at
           FROM projects ORDER BY archived_at IS NOT NULL, lower(name), id"""
      ).fetchall()
      rule_rows = _rules(conn, include_archived=True)
    by_project: dict[int, list] = {}
    for r in rule_rows:
      by_project.setdefault(r['project_id'], []).append(_rule(r))
    return {'projects': [{**_project(p), 'rules': by_project.get(p['id'], [])} for p in rows]}

  @app.post('/api/projects', status_code=201)
  def add_project(body: ProjectFields, session: sessions.Session = Depends(mutating)):
    values = _project_values(body)
    if 'name' not in values:
      raise ApiError(400, 'invalid_name')
    values.pop('archived_at', None)
    with user_transaction(db, session.user_id) as conn:
      workspace = _personal_workspace(conn, session.user_id)
      columns = ['workspace_id', 'created_by', *values]
      row = conn.execute(
        f"""INSERT INTO projects ({', '.join(columns)})
            VALUES ({', '.join(['%s'] * len(columns))})
            RETURNING id, workspace_id, name, color, client_name, billable, hourly_rate, currency,
                      archived_at, created_at""",
        [workspace, session.user_id, *values.values()],
      ).fetchone()
    return {**_project(row), 'rules': []}

  @app.patch('/api/projects/{project_id}')
  def edit_project(project_id: int, body: ProjectFields, session: sessions.Session = Depends(mutating)):
    values = _project_values(body)
    if not values:
      raise ApiError(400, 'invalid_body')
    with user_transaction(db, session.user_id) as conn:
      sets = ', '.join(f'{c} = %s' for c in values)
      row = conn.execute(
        f'UPDATE projects SET {sets}, updated_at = now() WHERE id = %s RETURNING id',
        [*values.values(), project_id],
      ).fetchone()
      if row is None:
        raise ApiError(404, 'not_found')
      if 'archived_at' in values:
        _enqueue_recalc(conn, session.user_id, 'rules_changed')
    return {'ok': True}

  @app.delete('/api/projects/{project_id}')
  def delete_project(project_id: int, session: sessions.Session = Depends(mutating)):
    with user_transaction(db, session.user_id) as conn:
      row = conn.execute('DELETE FROM projects WHERE id = %s RETURNING id', [project_id]).fetchone()
      if row is None:
        raise ApiError(404, 'not_found')
      _enqueue_recalc(conn, session.user_id, 'rules_changed')
    return {'ok': True}

  @app.post('/api/projects/{project_id}/rules', status_code=201)
  def add_rule(project_id: int, body: NewRule, session: sessions.Session = Depends(mutating)):
    signal, pattern = _checked_rule(body)
    with user_transaction(db, session.user_id) as conn:
      project = conn.execute('SELECT id, workspace_id FROM projects WHERE id = %s',
                             [project_id]).fetchone()
      if project is None:
        raise ApiError(404, 'not_found')
      duplicate = conn.execute(
        """SELECT 1 FROM project_mappings
           WHERE user_id = %s AND project_id = %s AND signal = %s AND pattern = %s""",
        [session.user_id, project_id, signal, pattern],
      ).fetchone()
      if duplicate:
        raise ApiError(409, 'duplicate_rule')
      row = conn.execute(
        """INSERT INTO project_mappings (workspace_id, project_id, user_id, signal, pattern, priority,
                                         created_by)
           VALUES (%s, %s, %s, %s, %s, %s, %s)
           RETURNING id, project_id, user_id, signal, pattern, priority, created_at""",
        [project['workspace_id'], project_id, session.user_id, signal, pattern, body.priority,
         session.user_id],
      ).fetchone()
      _enqueue_recalc(conn, session.user_id, 'rules_changed')
    return _rule(row)

  @app.delete('/api/rules/{rule_id}')
  def delete_rule(rule_id: int, session: sessions.Session = Depends(mutating)):
    with user_transaction(db, session.user_id) as conn:
      row = conn.execute('DELETE FROM project_mappings WHERE id = %s AND user_id = %s RETURNING id',
                         [rule_id, session.user_id]).fetchone()
      if row is None:
        raise ApiError(404, 'not_found')
      _enqueue_recalc(conn, session.user_id, 'rules_changed')
    return {'ok': True}

  @app.post('/api/rules/preview')
  def preview_rule(body: RulePreview, session: sessions.Session = Depends(mutating)):
    """What a rule would match in the user's recent events, before it is saved.

    Counts events (not time: only the engine computes time), lists the matched
    values, and names other projects whose rules match the same events.
    """

    signal, pattern = _checked_rule(body)
    since = datetime.now(UTC) - timedelta(days=body.days)
    own = rules.EVIDENCE_COLUMNS[signal]
    with user_transaction(db, session.user_id) as conn:
      groups = conn.execute(
        f"""SELECT {', '.join(f'{c} AS c{i}' for i, c in enumerate(_ALL_EVIDENCE))},
                   count(*) AS events, max(server_adjusted_start) AS last_seen
            FROM activity_events
            WHERE timestamp_start >= %(since)s
              AND ({' OR '.join(f'{c} IS NOT NULL' for c in own)})
            GROUP BY {', '.join(str(i + 1) for i in range(len(_ALL_EVIDENCE)))}
            ORDER BY events DESC
            LIMIT %(limit)s""",
        {'since': since, 'limit': MAX_PREVIEW_GROUPS + 1},
      ).fetchall()
      others = [r for r in _rules(conn) if r['project_id'] != body.project_id]
      names = {p['id']: p['name'] for p in conn.execute('SELECT id, name FROM projects').fetchall()}
    truncated = len(groups) > MAX_PREVIEW_GROUPS
    matched, examples, conflicts = 0, {}, {}
    for group in groups[:MAX_PREVIEW_GROUPS]:
      evidence = _evidence(group)
      hits = [v for v in evidence[signal] if rules.matches_value(signal, pattern, v)]
      if not hits:
        continue
      matched += group['events']
      for value in dict.fromkeys(hits):
        example = examples.setdefault(value, {'value': value, 'events': 0, 'last_seen': None})
        example['events'] += group['events']
        example['last_seen'] = max(filter(None, (example['last_seen'], group['last_seen'])))
      winner = rules.resolve(others, evidence)
      if winner is not None:
        conflicts[winner['project_id']] = conflicts.get(winner['project_id'], 0) + group['events']
    top = sorted(examples.values(), key=lambda e: (-e['events'], e['value']))[:10]
    return {
      'signal': signal,
      'pattern': pattern,
      'days': body.days,
      'matched_events': matched,
      'examples': [{**e, 'last_seen': _iso(e['last_seen'])} for e in top],
      'distinct_values': len(examples),
      'conflicts': [{'project_id': pid, 'name': names.get(pid), 'events': n}
                    for pid, n in sorted(conflicts.items(), key=lambda kv: -kv[1])],
      'truncated': truncated,
    }

  @app.get('/api/rules/suggestions')
  def rule_suggestions(session: sessions.Session = Depends(current)):
    """Detected repositories no rule covers yet: optional one-click projects (D17)."""

    since = datetime.now(UTC) - timedelta(days=PREVIEW_DAYS)
    with user_transaction(db, session.user_id) as conn:
      repos = conn.execute(
        """SELECT repository_remote AS remote, min(repository_root) AS root, count(*) AS events,
                  max(server_adjusted_start) AS last_seen
           FROM activity_events
           WHERE timestamp_start >= %s AND repository_remote IS NOT NULL
           GROUP BY repository_remote ORDER BY events DESC LIMIT 200""",
        [since],
      ).fetchall()
      existing = _rules(conn)
    out = []
    for repo in repos:
      evidence = {'remote': [repo['remote']], 'path': [repo['root']] if repo['root'] else []}
      if rules.resolve(existing, evidence) is None:
        name = repo['remote'].rsplit('/', 1)[-1]
        out.append({'remote': repo['remote'], 'root': repo['root'], 'name': name,
                    'events': repo['events'], 'last_seen': _iso(repo['last_seen'])})
    return {'repositories': out[:20]}

  @app.api_route('/api/{rest:path}', methods=['GET', 'POST', 'PUT', 'PATCH', 'DELETE'])
  def api_not_found(rest: str):
    raise ApiError(404, 'not_found')

  # -- web app -------------------------------------------------------------

  static = Path(config.static_dir).resolve() if config.static_dir else None
  if static is not None and (static / 'index.html').is_file():

    @app.get('/{path:path}', include_in_schema=False)
    def web(path: str):
      candidate = (static / path).resolve()
      if path and candidate.is_file() and candidate.is_relative_to(static):
        headers = {'Cache-Control': 'public, max-age=31536000, immutable'} if path.startswith('assets/') else None
        return FileResponse(candidate, headers=headers)
      return FileResponse(static / 'index.html', headers={'Cache-Control': 'no-cache'})
  else:

    @app.get('/', include_in_schema=False)
    def no_web():
      return Response('CodeSecretary dashboard API. Build overview/web and set STATIC_DIR.',
                      media_type='text/plain')

  return app


def _iso(value):
  if isinstance(value, datetime):
    return value.astimezone(UTC).isoformat().replace('+00:00', 'Z')
  if isinstance(value, time):
    return value.strftime('%H:%M')
  return value


def _jsonable(row: dict) -> dict:
  return {k: (str(v) if k == 'event_id' else _iso(v)) for k, v in row.items()}


def _json(value) -> str:
  import json
  return json.dumps(value)


def _exclusion(row: dict) -> dict:
  days = row['days_of_week']
  return {
    'id': row['id'],
    'kind': row['kind'],
    'pattern': row['pattern'],
    'start': _iso(row['window_start']),
    'end': _iso(row['window_end']),
    'days': days if isinstance(days, list) else [],
    'created_at': _iso(row['created_at']),
  }


# -- projects and rules helpers ---------------------------------------------

def _personal_workspace(conn, user_id: int) -> int:
  row = conn.execute('SELECT id FROM workspaces WHERE personal_owner_id = %s', [user_id]).fetchone()
  if row is None:
    raise ApiError(409, 'no_workspace')
  return row['id']


def _project_values(body: ProjectFields) -> dict:
  """The columns a create/edit body sets, validated."""

  values: dict = {}
  given = body.model_fields_set
  if 'name' in given:
    name = (body.name or '').strip()
    if not name:
      raise ApiError(400, 'invalid_name')
    values['name'] = name
  if 'color' in given:
    if body.color and not _COLOR.match(body.color):
      raise ApiError(400, 'invalid_color')
    values['color'] = body.color or None
  if 'client_name' in given:
    values['client_name'] = (body.client_name or '').strip() or None
  if 'billable' in given and body.billable is not None:
    values['billable'] = body.billable
  if 'hourly_rate' in given:
    values['hourly_rate'] = body.hourly_rate
  if 'currency' in given:
    currency = (body.currency or '').strip().upper()
    if currency and not _CURRENCY.match(currency):
      raise ApiError(400, 'invalid_currency')
    values['currency'] = currency or None
  if 'archived' in given and body.archived is not None:
    values['archived_at'] = datetime.now(UTC) if body.archived else None
  return values


def _checked_rule(body: NewRule) -> tuple[str, str]:
  if body.signal not in rules.SIGNALS:
    raise ApiError(400, 'invalid_signal')
  pattern = rules.normalize_pattern(body.signal, body.pattern)
  if pattern is None:
    raise ApiError(400, 'invalid_pattern')
  return body.signal, pattern


def _rules(conn, include_archived: bool = False) -> list[dict]:
  """The user's own rules (RLS also hides others'); archived projects' rules
  are inactive, so matching leaves them out."""

  archived = '' if include_archived else 'AND p.archived_at IS NULL'
  return conn.execute(
    f"""SELECT m.id, m.project_id, m.user_id, m.signal, m.pattern, m.priority, m.created_at
        FROM project_mappings m JOIN projects p ON p.id = m.project_id
        WHERE m.user_id = app_current_user_id() {archived}
        ORDER BY m.project_id, m.id"""
  ).fetchall()


def _evidence(group: dict) -> dict[str, list[str]]:
  out, i = {}, 0
  for signal in rules.SIGNALS:
    n = len(rules.EVIDENCE_COLUMNS[signal])
    out[signal] = rules.evidence_values(signal, tuple(group[f'c{j}'] for j in range(i, i + n)))
    i += n
  return out


def _enqueue_recalc(conn, user_id: int, reason: str) -> None:
  """Queue the engine for the user's last RECALC_DAYS local days (one job per day)."""

  row = conn.execute('SELECT time_zone FROM users WHERE id = %s', [user_id]).fetchone()
  try:
    zone = ZoneInfo((row or {}).get('time_zone') or 'UTC')
  except ZoneInfoNotFoundError:
    zone = ZoneInfo('UTC')
  today = datetime.now(zone).date()
  conn.execute(
    """INSERT INTO processing_jobs (user_id, local_day, reason, status, run_after, attempts)
       SELECT %s, d::date, %s, 'queued', now(), 0
       FROM generate_series(%s::date, %s::date, interval '1 day') AS d
       ON CONFLICT (user_id, local_day) WHERE status = 'queued' DO NOTHING""",
    [user_id, reason, today - timedelta(days=RECALC_DAYS - 1), today],
  )


def _zone(name: str | None) -> ZoneInfo:
  try:
    return ZoneInfo(name or 'UTC')
  except (ZoneInfoNotFoundError, ValueError):
    return ZoneInfo('UTC')


def _job_state(conn, user_id: int, first: date, last: date) -> dict:
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


def _time_summary(first: date, last: date, rows: list[dict], projects: dict, time_zone: str,
                  jobs: dict) -> dict:
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
  unassigned = totals.get(None, {'seconds': 0})
  return {
    'start': first.isoformat(),
    'end': last.isoformat(),
    'time_zone': time_zone,
    'total_seconds': sum(t['seconds'] for t in totals.values()),
    'assigned_seconds': sum(p['seconds'] for p in project_totals),
    'unassigned_seconds': unassigned['seconds'],
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


def _project(row: dict) -> dict:
  rate = row['hourly_rate']
  return {
    'id': row['id'],
    'workspace_id': row['workspace_id'],
    'name': row['name'],
    'color': row['color'],
    'client_name': row['client_name'],
    'billable': row['billable'],
    'hourly_rate': None if rate is None else f'{rate:.2f}',
    'currency': row['currency'],
    'archived_at': _iso(row['archived_at']),
    'created_at': _iso(row['created_at']),
  }


def _rule(row: dict) -> dict:
  return {
    'id': row['id'],
    'project_id': row['project_id'],
    'signal': row['signal'],
    'pattern': row['pattern'],
    'priority': row['priority'],
    'shared': row['user_id'] is None,
    'created_at': _iso(row['created_at']),
  }
