"""Connection settings for the single database the ORM talks to.

Settings come from keyword arguments or from ``ORM_*`` environment variables:

| Variable | Default | Meaning |
|---|---|---|
| ``ORM_DATABASE_URL`` | unset | Full connection string; wins over the separate fields |
| ``ORM_DB_HOST`` | ``localhost`` | Host |
| ``ORM_DB_PORT`` | ``5432`` | Port |
| ``ORM_DB_USER`` | ``postgres`` | Login role |
| ``ORM_DB_PASSWORD`` | empty | Password |
| ``ORM_DB_NAME`` | ``postgres`` | Database name |
| ``ORM_DB_SSLMODE`` | unset | libpq ``sslmode`` |
| ``ORM_DB_SCHEMA`` | ``public`` | Schema the models live in |
| ``ORM_DB_POOL_MIN`` / ``ORM_DB_POOL_MAX`` | ``1`` / ``10`` | Pool size |
"""

from __future__ import annotations

import os
from dataclasses import dataclass

from psycopg.conninfo import make_conninfo


@dataclass(frozen=True)
class Config:
  dsn: str
  schema: str = 'public'
  pool_min: int = 1
  pool_max: int = 10
  pool_timeout: float = 5.0

  def __repr__(self) -> str:
    # Never print the connection string: it can contain a password.
    return (
      f'Config(dsn=<redacted>, schema={self.schema!r}, '
      f'pool_min={self.pool_min}, pool_max={self.pool_max})'
    )

  @classmethod
  def from_params(
    cls,
    *,
    host: str = 'localhost',
    port: int = 5432,
    user: str = 'postgres',
    password: str | None = None,
    dbname: str = 'postgres',
    sslmode: str | None = None,
    connect_timeout: int = 10,
    **options,
  ) -> 'Config':
    params = {
      'host': host,
      'port': str(port),
      'user': user,
      'dbname': dbname,
      'connect_timeout': str(connect_timeout),
    }
    if password:
      params['password'] = password
    if sslmode:
      params['sslmode'] = sslmode
    return cls(dsn=make_conninfo(**params), **options)

  @classmethod
  def from_env(cls, environ=None) -> 'Config':
    env = os.environ if environ is None else environ

    options = {
      'schema': env.get('ORM_DB_SCHEMA', 'public'),
      'pool_min': int(env.get('ORM_DB_POOL_MIN', 1)),
      'pool_max': int(env.get('ORM_DB_POOL_MAX', 10)),
    }
    url = env.get('ORM_DATABASE_URL')
    if url:
      return cls(dsn=url, **options)
    return cls.from_params(
      host=env.get('ORM_DB_HOST', 'localhost'),
      port=int(env.get('ORM_DB_PORT', 5432)),
      user=env.get('ORM_DB_USER', 'postgres'),
      password=env.get('ORM_DB_PASSWORD'),
      dbname=env.get('ORM_DB_NAME', 'postgres'),
      sslmode=env.get('ORM_DB_SSLMODE'),
      **options,
    )
