"""HTTP endpoint tests that need no database (WH-1)."""

from __future__ import annotations

import pytest
from fastapi.testclient import TestClient


@pytest.fixture
def client():
  from secretary_orm import Config, Environment

  from secretary_warehouse.http import create_app

  # A pool pointing at a closed port: the app boots, the DB is "down".
  env = Environment(
    Config(dsn='postgresql://nobody:nothing@127.0.0.1:1/none?connect_timeout=1', pool_timeout=1),
    auto_sync=False,
  )
  with TestClient(create_app(env)) as client:
    yield client
  Environment.reset()


def test_healthz_is_ok_even_when_database_is_down(client):
  response = client.get('/healthz')
  assert response.status_code == 200
  assert response.json() == {'status': 'ok'}


def test_readyz_reports_unavailable_when_database_is_down(client):
  response = client.get('/readyz')
  assert response.status_code == 503
  assert response.json() == {'status': 'unavailable', 'reason': 'database'}


def test_unknown_routes_are_404(client):
  assert client.get('/v1/nope').status_code == 404


def test_readyz_is_ready_when_database_is_up(test_db, app_env):
  from secretary_warehouse.http import create_app

  with TestClient(create_app(app_env)) as client:
    response = client.get('/readyz')
  assert response.status_code == 200
  assert response.json() == {'status': 'ready'}
