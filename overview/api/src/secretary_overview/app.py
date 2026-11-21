"""HTTP routes of the dashboard API.

- ``/auth/*``: sign in (Keycloak, PKCE), callback, sign out.
- ``/api/*``: JSON for the web app. Needs the session cookie; mutations also
  need ``X-CSRF-Token`` (from ``/api/me``) and, when sent, a same-origin
  ``Origin``. Errors are ``{"error": code}``.
- Everything else: the built web app (``STATIC_DIR``), with ``index.html`` for
  client-side routes.

Every query runs in the signed-in user's RLS context (tenant identity comes
from the session, never from the request). Raw activity is returned as
stored; this service never computes time.
"""

from __future__ import annotations

import asyncio
import hmac
import logging
import re
from contextlib import asynccontextmanager
from datetime import UTC, date, datetime, time, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from fastapi import Depends, FastAPI, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.exceptions import RequestValidationError
from fastapi.responses import FileResponse, JSONResponse, RedirectResponse, Response
from pydantic import BaseModel, ConfigDict, Field
from secretary_orm import Environment

from secretary_overview import __version__, sessions
from secretary_overview.config import Config
from secretary_overview.oidc import Oidc, SignInError
from secretary_warehouse.db import ping, user_transaction

logger = logging.getLogger('secretary_overview.http')

LOGIN_COOKIE = 'sec_login'
#: Most events returned for one day; the response says when it was cut.
MAX_DAY_EVENTS = 5000
EXCLUSION_KINDS = ('path', 'app', 'domain', 'time_window')
_HH_MM = re.compile(r'^([01]\d|2[0-3]):[0-5]\d$')


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
