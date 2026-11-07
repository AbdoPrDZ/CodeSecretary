"""Test harness.

Database tests need ``DATABASE_URL`` pointing at a Postgres superuser, e.g. the
docker-compose one: ``postgresql://secretary:secretary@localhost:5432/secretary``.
Each test gets a fresh database built by ``migrate()``. Fixtures are seeded as
the superuser; assertions run as ``secretary_test_app``, a login role in
``secretary_app``, so RLS applies exactly as it does for the real services.
"""

from __future__ import annotations

import os
import uuid
from dataclasses import dataclass

import psycopg
import pytest
from psycopg.conninfo import conninfo_to_dict, make_conninfo
from psycopg.rows import dict_row

SUPER_URL = os.environ.get('DATABASE_URL')
TEST_APP_ROLE = 'secretary_test_app'
TEST_APP_PASSWORD = 'test'


def _with(dsn: str, **overrides) -> str:
  params = conninfo_to_dict(dsn)
  params.update(overrides)
  return make_conninfo(**params)


@dataclass
class TestDb:
  name: str
  owner_dsn: str
  app_dsn: str
  admin: psycopg.Connection  # superuser, autocommit: seeds fixtures


@pytest.fixture
def test_db():
  if not SUPER_URL:
    pytest.skip('set DATABASE_URL (a Postgres superuser) to run database tests')

  from secretary_warehouse.schema import migrate

  name = f'wh_test_{uuid.uuid4().hex[:12]}'
  with psycopg.connect(SUPER_URL, autocommit=True) as conn:
    conn.execute(f'CREATE DATABASE "{name}"')

  owner_dsn = _with(SUPER_URL, dbname=name)
  migrate(owner_dsn)

  with psycopg.connect(owner_dsn, autocommit=True) as conn:
    conn.execute(
      f"""DO $$ BEGIN
            CREATE ROLE {TEST_APP_ROLE} LOGIN NOBYPASSRLS PASSWORD '{TEST_APP_PASSWORD}' IN ROLE secretary_app;
          EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL;
          END $$"""
    )

  admin = psycopg.connect(owner_dsn, autocommit=True, row_factory=dict_row)
  try:
    yield TestDb(
      name=name,
      owner_dsn=owner_dsn,
      app_dsn=_with(SUPER_URL, dbname=name, user=TEST_APP_ROLE, password=TEST_APP_PASSWORD),
      admin=admin,
    )
  finally:
    admin.close()
    with psycopg.connect(SUPER_URL, autocommit=True) as conn:
      conn.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


@pytest.fixture
def app_env(test_db):
  """ORM environment connected as the app role, with ONE pooled connection:
  proves tenant context never leaks between transactions."""

  from secretary_orm import Config, Environment

  import secretary_warehouse.schema.models  # noqa: F401

  env = Environment(Config(dsn=test_db.app_dsn, pool_min=1, pool_max=1), auto_sync=False)
  env.registry().load()
  try:
    yield env
  finally:
    Environment.reset()
