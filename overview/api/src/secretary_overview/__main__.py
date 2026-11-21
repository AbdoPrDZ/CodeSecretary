"""Command line: ``secretary-overview`` serves the dashboard API (and web app).

The schema is the warehouse's (D15); start the warehouse first so it has
migrated. Exit codes: 0 ok, 1 failure, 2 configuration error.
"""

from __future__ import annotations

import logging
import sys

from secretary_overview import __version__
from secretary_overview.config import Config, ConfigError
from secretary_warehouse import telemetry

logger = logging.getLogger('secretary_overview')


def main(argv=None) -> int:
  try:
    config = Config.from_env()
  except ConfigError as exc:
    print(f'secretary-overview: {exc}', file=sys.stderr)
    return 2
  telemetry.init(config.log_format, config.log_level)
  try:
    import uvicorn
    from secretary_orm import Config as OrmConfig, Environment

    from secretary_overview.app import create_app
    from secretary_warehouse.schema import models  # noqa: F401  (registers every model)

    logger.info('overview starting', extra={'version': __version__})
    env = Environment(
      OrmConfig(dsn=config.database_url, pool_min=1, pool_max=config.database_max_connections),
      auto_sync=False,  # the warehouse owns and migrates the schema
    )
    env.registry().load()
    logger.info('listening', extra={'host': config.host, 'port': config.port})
    uvicorn.run(create_app(env, config), host=config.host, port=config.port,
                log_config=None, access_log=False)
    return 0
  except Exception:
    logger.exception('overview failed')
    return 1


if __name__ == '__main__':
  sys.exit(main())
