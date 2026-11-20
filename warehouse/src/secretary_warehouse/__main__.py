"""Command line: ``secretary-warehouse [serve|migrate|plan]``.

* ``serve`` (default) -- sync the schema if ``DATABASE_MIGRATION_URL`` is set,
  then serve HTTP on ``WAREHOUSE_BIND``.
* ``migrate`` -- sync the schema (needs ``DATABASE_MIGRATION_URL``) and exit.
* ``plan`` -- print the ORM part of a migration without applying it.

Exit codes: 0 ok, 1 failure, 2 configuration error.
"""

from __future__ import annotations

import argparse
import logging
import sys

from secretary_warehouse import __version__, telemetry
from secretary_warehouse.config import Config, ConfigError

logger = logging.getLogger('secretary_warehouse')


def main(argv=None) -> int:
  parser = argparse.ArgumentParser(prog='secretary-warehouse')
  parser.add_argument('command', nargs='?', default='serve', choices=['serve', 'migrate', 'plan'])
  parser.add_argument('--allow-drop', action='store_true',
                      help='migrate/plan: drop columns that are no longer declared')
  args = parser.parse_args(argv)

  try:
    config = Config.from_env()
  except ConfigError as exc:
    print(f'secretary-warehouse: {exc}', file=sys.stderr)
    return 2
  telemetry.init(config.log_format, config.log_level)

  try:
    if args.command == 'plan':
      return _plan(config, args.allow_drop)
    if args.command == 'migrate':
      if not config.migration_database_url:
        print('secretary-warehouse: DATABASE_MIGRATION_URL is required for migrate', file=sys.stderr)
        return 2
      _migrate(config, args.allow_drop)
      return 0
    _serve(config)
    return 0
  except Exception:
    logger.exception('warehouse failed')
    return 1


def _migrate(config: Config, allow_drop: bool) -> None:
  from secretary_warehouse.schema import migrate

  migrate(config.migration_database_url, allow_drop=allow_drop)


def _plan(config: Config, allow_drop: bool) -> int:
  from secretary_orm.schema import format_plan

  from secretary_warehouse.schema import preview

  if not config.migration_database_url:
    print('secretary-warehouse: DATABASE_MIGRATION_URL is required for plan', file=sys.stderr)
    return 2
  plan = preview(config.migration_database_url, allow_drop=allow_drop)
  print(format_plan(plan))
  return 1 if plan.blocked else 0


def _serve(config: Config) -> None:
  import uvicorn
  from secretary_orm import Config as OrmConfig, Environment

  from secretary_warehouse.auth import AuthSettings
  from secretary_warehouse.http import create_app
  from secretary_warehouse.schema import models  # noqa: F401  (registers every model)

  logger.info('warehouse starting', extra={'version': __version__})
  if config.migration_database_url:
    _migrate(config, allow_drop=False)

  env = Environment(
    OrmConfig(dsn=config.database_url, pool_min=1, pool_max=config.database_max_connections),
    auto_sync=False,  # the schema is owned by migrate(), never by a request
  )
  env.registry().load()

  logger.info('listening', extra={'host': config.host, 'port': config.port})
  uvicorn.run(
    create_app(env, AuthSettings.from_config(config)),
    host=config.host,
    port=config.port,
    log_config=None,  # keep telemetry's handlers (text or JSON)
    access_log=False,  # request lines can carry identifiers; add structured access logs deliberately
  )
  logger.info('warehouse stopped')


if __name__ == '__main__':
  sys.exit(main())
