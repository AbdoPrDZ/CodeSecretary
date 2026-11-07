"""HTTP routes. Device API endpoints (``/v1/...``) are added by WH-4...WH-10."""

from __future__ import annotations

import asyncio
import logging
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import JSONResponse
from secretary_orm import Environment

from secretary_warehouse import __version__
from secretary_warehouse.db import ping

logger = logging.getLogger('secretary_warehouse.http')

#: Readiness gives up on the database after this many seconds.
READY_TIMEOUT = 2.0


def create_app(env: Environment) -> FastAPI:
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

  return app


def _unavailable() -> JSONResponse:
  return JSONResponse({'status': 'unavailable', 'reason': 'database'}, status_code=503)
