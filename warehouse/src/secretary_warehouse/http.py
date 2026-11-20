"""HTTP routes: health, and the device API (``/v1/...``, WAREHOUSE plan §4).

Bodies mirror ``secretary_shared::api`` (Rust). Errors are always
``{"error": "<code>"}`` with the codes of ``secretary_shared::api::error_code``;
401 responses carry ``WWW-Authenticate: Bearer``.
"""

from __future__ import annotations

import asyncio
import logging
from contextlib import asynccontextmanager
from datetime import UTC, datetime

from fastapi import FastAPI, Header, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse, Response
from pydantic import BaseModel, ConfigDict, Field
from secretary_orm import Environment

from secretary_warehouse import __version__, agent_config
from secretary_warehouse.auth import (
  AuthError,
  Authenticator,
  AuthSettings,
  AuthUnavailable,
  TokenVerifier,
)
from secretary_warehouse.db import ping, user_transaction
from secretary_warehouse.ingest import MAX_BODY_BYTES, BatchError, decode_body, ingest, iso

logger = logging.getLogger('secretary_warehouse.http')

#: Readiness gives up on the database after this many seconds.
READY_TIMEOUT = 2.0


class RegisterDevice(BaseModel):
  model_config = ConfigDict(extra='ignore')

  name: str = Field(min_length=1, max_length=255)
  os: str = Field(min_length=1, max_length=32)
  agent_version: str = Field(min_length=1, max_length=64)


class Heartbeat(BaseModel):
  model_config = ConfigDict(extra='ignore')

  agent_version: str = Field(min_length=1, max_length=64)
  queue_depth: int = Field(ge=0, le=2**31 - 1)
  clock_offset_ms: int = Field(ge=-(2**31), le=2**31 - 1)


def create_app(
  env: Environment,
  auth_settings: AuthSettings | None = None,
  verifier: TokenVerifier | None = None,
) -> FastAPI:
  settings = auth_settings or AuthSettings()
  authenticator = Authenticator(env.db, verifier or TokenVerifier(settings))

  @asynccontextmanager
  async def lifespan(_app: FastAPI):
    yield
    env.close()
    logger.info('database pool closed')

  app = FastAPI(
    title='CodeSecretary Warehouse',
    version=__version__,
    lifespan=lifespan,
    docs_url=None,
    redoc_url=None,
    openapi_url=None,
  )
  app.state.authenticator = authenticator

  @app.exception_handler(AuthError)
  async def _auth_error(_request: Request, exc: AuthError):
    logger.info('request rejected', extra={'status': exc.status, 'error_kind': exc.kind or exc.code})
    headers = {'WWW-Authenticate': f'Bearer error="{exc.code}"'} if exc.status == 401 else None
    return JSONResponse({'error': exc.code}, status_code=exc.status, headers=headers)

  @app.exception_handler(AuthUnavailable)
  async def _auth_unavailable(_request: Request, exc: AuthUnavailable):
    logger.warning('signing keys unavailable', extra={'error_kind': str(exc)})
    return JSONResponse({'error': 'auth_unavailable'}, status_code=503, headers={'Retry-After': '30'})

  @app.exception_handler(BatchError)
  async def _batch_error(_request: Request, exc: BatchError):
    return JSONResponse({'error': exc.code}, status_code=exc.status)

  @app.exception_handler(RequestValidationError)
  async def _invalid_body(_request: Request, _exc: RequestValidationError):
    # FastAPI's default 422 echoes the input back; keep errors uniform and terse.
    return JSONResponse({'error': 'invalid_body'}, status_code=400)

  @app.get('/healthz')
  def healthz():
    """Liveness: the process is up and serving."""

    return {'status': 'ok'}

  @app.get('/readyz')
  async def readyz():
    """Readiness: the database is reachable."""

    try:
      await asyncio.wait_for(run_in_threadpool(ping, env.db), READY_TIMEOUT)
    except TimeoutError:
      logger.warning('readiness check timed out')
      return _unavailable()
    except Exception as exc:  # noqa: BLE001 - report the kind only, never details
      logger.warning('readiness check failed', extra={'error_kind': type(exc).__name__})
      return _unavailable()
    return {'status': 'ready'}

  @app.get('/v1/auth/config')
  def auth_config():
    """Where the agent signs in (Keycloak device grant); no auth."""

    return {'issuer': settings.issuer, 'client_id': settings.client_id}

  @app.post('/v1/devices')
  def register_device(body: RegisterDevice, authorization: str | None = Header(default=None)):
    """Registers the device for the token's Keycloak session. Idempotent per session."""

    identity = authenticator.identify(authorization)
    with user_transaction(env.db, identity.user_id) as conn:
      row = conn.execute(
        """INSERT INTO devices (user_id, name, os, agent_version, auth_session_id, clock_offset_ms)
           VALUES (%s, %s, %s, %s, %s, 0)
           ON CONFLICT (auth_session_id) DO UPDATE
             SET name = EXCLUDED.name, os = EXCLUDED.os,
                 agent_version = EXCLUDED.agent_version, updated_at = now()
             WHERE devices.revoked_at IS NULL
           RETURNING id""",
        [identity.user_id, body.name, body.os, body.agent_version, identity.session_id],
      ).fetchone()
    authenticator.forget()
    if row is None:  # the session's device exists and was revoked
      raise AuthError(401, 'device_revoked', 'revoked')
    logger.info('device registered', extra={'device_id': row['id']})
    return {'device_id': row['id']}

  @app.post('/v1/events:batch')
  async def events_batch(request: Request, authorization: str | None = Header(default=None)):
    """Gzip JSON batch of events (``BatchRequest``)."""

    principal = await run_in_threadpool(authenticator.device, authorization)
    raw = await _read_capped(request, MAX_BODY_BYTES)
    body = await run_in_threadpool(decode_body, raw, request.headers.get('content-encoding'))
    result = await run_in_threadpool(ingest, env.db, principal.user_id, principal.device_id, body)
    logger.info('batch ingested', extra={
      'device_id': principal.device_id, 'accepted': len(result.accepted),
      'duplicates': len(result.duplicates), 'rejected': len(result.rejected),
    })
    return result.as_json(datetime.now(UTC))

  @app.get('/v1/agent/config')
  def get_agent_config(
    authorization: str | None = Header(default=None),
    if_none_match: str | None = Header(default=None),
  ):
    """Exclusions, pause windows, project roots and timings (``AgentConfig``) with an ETag."""

    principal = authenticator.device(authorization)
    config = agent_config.build(env.db, principal.user_id)
    tag = agent_config.etag(config)
    headers = {'ETag': tag, 'Cache-Control': 'private, no-cache'}
    if agent_config.matches(if_none_match, tag):
      return Response(status_code=304, headers=headers)
    return JSONResponse(config, headers=headers)

  @app.post('/v1/agent/heartbeat')
  def heartbeat(body: Heartbeat, authorization: str | None = Header(default=None)):
    """Health, queue depth and clock offset; returns the server clock."""

    principal = authenticator.device(authorization)
    with user_transaction(env.db, principal.user_id) as conn:
      conn.execute(
        """UPDATE devices SET last_seen_at = now(), clock_offset_ms = %s, agent_version = %s,
                              queue_depth = %s, updated_at = now()
           WHERE id = %s""",
        [body.clock_offset_ms, body.agent_version, body.queue_depth, principal.device_id],
      )
    return {'server_time': iso(datetime.now(UTC))}

  return app


async def _read_capped(request: Request, limit: int) -> bytes:
  declared = request.headers.get('content-length')
  if declared is not None and declared.isdigit() and int(declared) > limit:
    raise BatchError(413, 'batch_too_large')
  chunks, size = [], 0
  async for chunk in request.stream():
    size += len(chunk)
    if size > limit:
      raise BatchError(413, 'batch_too_large')
    chunks.append(chunk)
  return b''.join(chunks)


def _unavailable() -> JSONResponse:
  return JSONResponse({'status': 'unavailable', 'reason': 'database'}, status_code=503)
