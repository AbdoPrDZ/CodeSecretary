"""Configuration from environment variables.

| Variable | Default | Meaning |
|---|---|---|
| ``DATABASE_URL`` | required | App connection: a login role in ``secretary_app``, never the table owner |
| ``DATABASE_MIGRATION_URL`` | unset | Owner connection; if set, the schema is synced at startup |
| ``WAREHOUSE_BIND`` | ``0.0.0.0:8080`` | Listen address (``host:port``) |
| ``DATABASE_MAX_CONNECTIONS`` | ``10`` | Pool size |
| ``LOG_FORMAT`` | ``text`` | ``text`` or ``json`` |
| ``LOG_LEVEL`` | ``info`` | ``debug``, ``info``, ``warning``, ``error`` |
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Callable, Mapping

LOG_FORMATS = ('text', 'json')
LOG_LEVELS = ('debug', 'info', 'warning', 'error')


class ConfigError(ValueError):
  pass


@dataclass(frozen=True)
class Config:
  host: str
  port: int
  database_url: str
  migration_database_url: str | None
  database_max_connections: int
  log_format: str
  log_level: str

  def __repr__(self) -> str:
    # Never print connection strings: they contain passwords.
    migration = None if self.migration_database_url is None else '<redacted>'
    return (
      f'Config(bind={self.host}:{self.port}, database_url=<redacted>, '
      f'migration_database_url={migration}, '
      f'database_max_connections={self.database_max_connections}, '
      f'log_format={self.log_format!r}, log_level={self.log_level!r})'
    )

  @classmethod
  def from_env(cls) -> 'Config':
    return cls.from_lookup(os.environ.get)

  @classmethod
  def from_lookup(cls, lookup: Callable[[str], str | None] | Mapping[str, str]) -> 'Config':
    get_raw = lookup.get if isinstance(lookup, Mapping) else lookup

    def get(name: str) -> str | None:
      value = get_raw(name)
      return value if value is not None and value.strip() else None

    database_url = get('DATABASE_URL')
    if database_url is None:
      raise ConfigError('DATABASE_URL is required')

    host, port = _parse_bind(get('WAREHOUSE_BIND') or '0.0.0.0:8080')

    raw_max = get('DATABASE_MAX_CONNECTIONS') or '10'
    try:
      max_connections = int(raw_max)
    except ValueError:
      max_connections = 0
    if max_connections <= 0:
      raise ConfigError('DATABASE_MAX_CONNECTIONS is invalid: must be a positive integer')

    log_format = (get('LOG_FORMAT') or 'text').lower()
    if log_format not in LOG_FORMATS:
      raise ConfigError(f'LOG_FORMAT is invalid: expected text or json, got {log_format!r}')

    log_level = (get('LOG_LEVEL') or 'info').lower()
    if log_level not in LOG_LEVELS:
      raise ConfigError(f'LOG_LEVEL is invalid: expected one of {", ".join(LOG_LEVELS)}')

    return cls(
      host=host,
      port=port,
      database_url=database_url,
      migration_database_url=get('DATABASE_MIGRATION_URL'),
      database_max_connections=max_connections,
      log_format=log_format,
      log_level=log_level,
    )


def _parse_bind(value: str) -> tuple[str, int]:
  host, sep, port = value.rpartition(':')
  if not sep or not host:
    raise ConfigError(f'WAREHOUSE_BIND is invalid: expected host:port, got {value!r}')
  try:
    number = int(port)
  except ValueError:
    number = -1
  if not 0 < number < 65536:
    raise ConfigError(f'WAREHOUSE_BIND is invalid: bad port {port!r}')
  return host.strip('[]'), number
