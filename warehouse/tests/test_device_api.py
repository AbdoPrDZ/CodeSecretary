"""Device API (WH-4 token validation + registration, WH-5 ingest, WH-8 heartbeat).

Tokens are minted here with a throwaway RSA key and served through a static
key resolver, so the checks run without a Keycloak instance. The real
Keycloak flow is covered by ``deploy/keycloak/smoke_device_flow.py`` and the
agent end-to-end run.
"""

from __future__ import annotations

import gzip
import json
import os
import time
import uuid
from datetime import UTC, datetime, timedelta

import jwt
import pytest
from cryptography.hazmat.primitives.asymmetric import rsa
from fastapi.testclient import TestClient

from secretary_warehouse.auth import AuthSettings, TokenVerifier
from secretary_warehouse.ingest import Rejected, validate_event

SETTINGS = AuthSettings(issuer='http://kc.test/realms/secretary', jwks_url='http://unused.test')
KEY = rsa.generate_private_key(public_exponent=65537, key_size=2048)
OTHER_KEY = rsa.generate_private_key(public_exponent=65537, key_size=2048)


class StaticKeys:
  """Stands in for PyJWKClient: one key, id ``k1``."""

  class _Signing:
    def __init__(self, key):
      self.key = key

  def get_signing_key_from_jwt(self, token):
    if jwt.get_unverified_header(token).get('kid') != 'k1':
      raise jwt.PyJWKClientError('unknown kid')
    return self._Signing(KEY.public_key())


def mint(*, key=KEY, kid='k1', algorithm='RS256', drop=(), **overrides) -> str:
  now = int(time.time())
  claims = {
    'iss': SETTINGS.issuer,
    'aud': ['secretary-warehouse', 'account'],
    'azp': 'secretary-agent',
    'typ': 'Bearer',
    'sub': 'kc-user-1',
    'sid': 'session-1',
    'email': 'dev@example.test',
    'name': 'Dev User',
    'iat': now,
    'exp': now + 300,
  }
  claims.update(overrides)
  for name in drop:
    claims.pop(name)
  return jwt.encode(claims, key, algorithm=algorithm, headers={'kid': kid})


def uuid7() -> str:
  """RFC 9562 UUIDv7 (``uuid.uuid7`` arrives only in Python 3.14)."""

  ms = time.time_ns() // 1_000_000
  rand = int.from_bytes(os.urandom(10), 'big')
  value = (ms << 80) | (0x7 << 76) | ((rand >> 62) & 0xFFF) << 64 | (0b10 << 62) | (rand & ((1 << 62) - 1))
  return str(uuid.UUID(int=value))


def auth(token: str) -> dict:
  return {'Authorization': f'Bearer {token}'}


def event(**overrides) -> dict:
  now = datetime.now(UTC).replace(microsecond=0)
  body = {
    'event_id': uuid7(),
    'schema_version': 1,
    'timestamp_start': (now - timedelta(minutes=5)).isoformat().replace('+00:00', 'Z'),
    'timestamp_end': (now - timedelta(minutes=1)).isoformat().replace('+00:00', 'Z'),
    'source': 'agent@0.1.0',
    'event_type': 'presence.active',
    'project_hint': None,
    'repository': None,
    'working_directory': None,
    'metadata': {},
  }
  body.update(overrides)
  return body


def batch(events, offset=0) -> dict:
  return {'batch_id': str(uuid.uuid4()), 'clock_offset_ms': offset, 'events': events}


def gz(body: dict) -> tuple[bytes, dict]:
  return gzip.compress(json.dumps(body).encode()), {'Content-Encoding': 'gzip', 'Content-Type': 'application/json'}


DEVICE = {'name': 'dev-laptop', 'os': 'windows', 'agent_version': '0.1.0'}


# ---------------------------------------------------------------- no database


@pytest.fixture
def offline_client():
  from secretary_orm import Config, Environment

  from secretary_warehouse.http import create_app

  env = Environment(
    Config(dsn='postgresql://nobody:nothing@127.0.0.1:1/none?connect_timeout=1', pool_timeout=1),
    auto_sync=False,
  )
  with TestClient(create_app(env, SETTINGS, TokenVerifier(SETTINGS, StaticKeys()))) as client:
    yield client
  Environment.reset()


def test_auth_config_needs_no_token(offline_client):
  response = offline_client.get('/v1/auth/config')
  assert response.status_code == 200
  assert response.json() == {'issuer': SETTINGS.issuer, 'client_id': 'secretary-agent'}


@pytest.mark.parametrize('headers', [
  {},
  {'Authorization': 'Basic abc'},
  {'Authorization': 'Bearer '},
  {'Authorization': 'Bearer not-a-jwt'},
], ids=['missing', 'wrong-scheme', 'empty', 'garbage'])
def test_missing_or_malformed_token_is_401(offline_client, headers):
  response = offline_client.post('/v1/devices', json=DEVICE, headers=headers)
  assert response.status_code == 401
  assert response.json() == {'error': 'invalid_token'}
  assert response.headers['www-authenticate'].startswith('Bearer')


@pytest.mark.parametrize('token', [
  mint(key=OTHER_KEY),                                    # forged signature
  mint(kid='k2'),                                         # unknown key
  mint(exp=int(time.time()) - 120, iat=int(time.time()) - 500),  # expired (beyond leeway)
  mint(aud='account'),                                    # wrong audience
  mint(azp='secretary-dashboard'),                        # another client's token
  mint(iss='http://evil.test/realms/secretary'),         # another issuer
  mint(typ='Refresh'),                                    # not an access token
  mint(drop=('sid',)),                                    # no device session
  mint(drop=('email',)),
  mint(drop=('exp',)),
  jwt.encode({'sub': 'x'}, 'secret', algorithm='HS256', headers={'kid': 'k1'}),  # alg confusion
], ids=['forged', 'unknown-kid', 'expired', 'audience', 'client', 'issuer', 'typ', 'no-sid',
        'no-email', 'no-exp', 'hs256'])
def test_bad_tokens_are_401(offline_client, token):
  for path, body in (('/v1/devices', DEVICE), ('/v1/agent/heartbeat',
                     {'agent_version': '0.1.0', 'queue_depth': 0, 'clock_offset_ms': 0})):
    response = offline_client.post(path, json=body, headers=auth(token))
    assert response.status_code == 401, path
    assert response.json() == {'error': 'invalid_token'}


def test_leeway_accepts_slightly_expired_token():
  verifier = TokenVerifier(SETTINGS, StaticKeys())
  now = int(time.time())
  claims = verifier.verify(mint(exp=now - 10, iat=now - 310))
  assert (claims.subject, claims.session_id) == ('kc-user-1', 'session-1')


def test_invalid_device_body_is_400(offline_client):
  response = offline_client.post('/v1/devices', json={'name': ''}, headers=auth(mint()))
  assert response.status_code == 400
  assert response.json() == {'error': 'invalid_body'}


# ------------------------------------------------------- event validation


def test_validate_event_rules():
  now = datetime.now(UTC)
  zero = timedelta(0)
  validate_event(event(), now, zero)  # baseline is valid

  def reason(**overrides):
    with pytest.raises(Rejected) as exc:
      validate_event(event(**overrides), now, zero)
    return exc.value.reason

  future = (now + timedelta(minutes=10)).isoformat()
  assert reason(event_id=str(uuid.uuid4())) == 'invalid_event_id'
  assert reason(event_id='nope') == 'invalid_event_id'
  assert reason(schema_version=2) == 'unsupported_schema_version'
  assert reason(event_type='keyboard.keys') == 'unknown_event_type'
  assert reason(source='agent') == 'invalid_source'
  assert reason(timestamp_end=(now - timedelta(hours=1)).isoformat()) == 'invalid_timestamps'
  assert reason(timestamp_start='2026-11-07T09:00:00') == 'invalid_timestamps'  # no zone
  assert reason(timestamp_start=future, timestamp_end=None) == 'timestamp_in_future'
  assert reason(timestamp_start=(now - timedelta(days=400)).isoformat(), timestamp_end=None) == \
    'timestamp_out_of_range'
  assert reason(metadata=[]) == 'invalid_metadata'
  assert reason(metadata={'title': 'x' * 9000}) == 'metadata_too_large'
  assert reason(repository='github.com/acme/api') == 'invalid_repository'


def test_clock_offset_is_subtracted():
  now = datetime.now(UTC)
  row = validate_event(event(), now, timedelta(milliseconds=90_000))  # device 90 s fast
  start = datetime.fromisoformat(row['timestamp_start'])
  adjusted = datetime.fromisoformat(row['server_adjusted_start'])
  assert start - adjusted == timedelta(seconds=90)


# ------------------------------------------------------------- with database


@pytest.fixture
def client(test_db, app_env):
  from secretary_warehouse.http import create_app

  with TestClient(create_app(app_env, SETTINGS, TokenVerifier(SETTINGS, StaticKeys()))) as client:
    yield client


def register(client, token=None) -> int:
  response = client.post('/v1/devices', json=DEVICE, headers=auth(token or mint()))
  assert response.status_code == 200, response.text
  return response.json()['device_id']


def test_register_provisions_user_and_is_idempotent_per_session(client, test_db):
  first = register(client)
  again = client.post('/v1/devices', json={**DEVICE, 'agent_version': '0.2.0'}, headers=auth(mint()))
  assert again.json()['device_id'] == first

  other_session = register(client, mint(sid='session-2'))
  assert other_session != first, 're-login (new Keycloak session) is a new device'

  users = test_db.admin.execute('SELECT id, email FROM users').fetchall()
  assert [u['email'] for u in users] == ['dev@example.test']
  devices = test_db.admin.execute(
    'SELECT id, user_id, agent_version, auth_session_id FROM devices ORDER BY id').fetchall()
  assert [(d['auth_session_id'], d['agent_version']) for d in devices] == \
    [('session-1', '0.2.0'), ('session-2', '0.1.0')]
  assert {d['user_id'] for d in devices} == {users[0]['id']}


def test_batch_before_registration_is_403(client):
  body, headers = gz(batch([event()]))
  response = client.post('/v1/events:batch', content=body, headers={**headers, **auth(mint())})
  assert response.status_code == 403
  assert response.json() == {'error': 'device_not_registered'}


def test_batch_is_stored_once_even_when_resent(client, test_db):
  device_id = register(client)
  events = [event() for _ in range(5)]
  body, headers = gz(batch(events, offset=1500))

  first = client.post('/v1/events:batch', content=body, headers={**headers, **auth(mint())})
  assert first.status_code == 200, first.text
  result = first.json()
  assert sorted(result['accepted']) == sorted(e['event_id'] for e in events)
  assert result['duplicates'] == [] and result['rejected'] == []
  assert datetime.fromisoformat(result['server_time'])

  # The agent lost the response and resends the same events in a new batch.
  body, headers = gz(batch(events, offset=1500))
  second = client.post('/v1/events:batch', content=body, headers={**headers, **auth(mint())})
  assert second.json()['accepted'] == []
  assert sorted(second.json()['duplicates']) == sorted(e['event_id'] for e in events)

  rows = test_db.admin.execute(
    'SELECT device_id, timestamp_start, server_adjusted_start, metadata FROM activity_events').fetchall()
  assert len(rows) == 5
  assert all(r['device_id'] == device_id and r['metadata'] == {} for r in rows)
  assert all(r['timestamp_start'] - r['server_adjusted_start'] == timedelta(milliseconds=1500) for r in rows)


def test_invalid_events_are_rejected_individually(client, test_db):
  register(client)
  good = event()
  repeated = event()
  body = batch([good, event(event_type='keyboard.keys'), {'nope': 1}, repeated, repeated])
  response = client.post('/v1/events:batch', json=body, headers=auth(mint()))  # plain JSON also works
  assert response.status_code == 200
  result = response.json()
  assert sorted(result['accepted']) == sorted([good['event_id'], repeated['event_id']])
  assert [r['reason'] for r in result['rejected']] == ['unknown_event_type', 'invalid_event_id']
  assert test_db.admin.execute('SELECT count(*) AS n FROM activity_events').fetchone()['n'] == 2


def test_whole_batch_errors(client):
  register(client)
  too_many = batch([event() for _ in range(1001)])
  body, headers = gz(too_many)
  assert client.post('/v1/events:batch', content=body,
                     headers={**headers, **auth(mint())}).status_code == 413

  bomb = gzip.compress(b'{"events": [' + b' ' * (20 * 1024 * 1024) + b']}')
  response = client.post('/v1/events:batch', content=bomb,
                         headers={'Content-Encoding': 'gzip', **auth(mint())})
  assert response.status_code == 413
  assert response.json() == {'error': 'batch_too_large'}

  for raw, enc in ((b'not json', None), (b'\x1f\x8bbroken', 'gzip'), (b'[]', None)):
    headers = {'Content-Encoding': enc} if enc else {}
    response = client.post('/v1/events:batch', content=raw, headers={**headers, **auth(mint())})
    assert response.status_code == 400, raw
    assert response.json() == {'error': 'invalid_body'}

  response = client.post('/v1/events:batch', json={**batch([event()]), 'clock_offset_ms': 'x'},
                         headers=auth(mint()))
  assert response.status_code == 400


def test_revoked_device_is_401(client, test_db):
  device_id = register(client)
  test_db.admin.execute('UPDATE devices SET revoked_at = now() WHERE id = %s', [device_id])
  client.app.state.authenticator.forget()  # in production the cache expires within 60 s

  for path, body in (
    ('/v1/events:batch', batch([event()])),
    ('/v1/agent/heartbeat', {'agent_version': '0.1.0', 'queue_depth': 0, 'clock_offset_ms': 0}),
    ('/v1/devices', DEVICE),  # cannot re-register a revoked session either
  ):
    response = client.post(path, json=body, headers=auth(mint()))
    assert response.status_code == 401, path
    assert response.json() == {'error': 'device_revoked'}

  # A new device login (new Keycloak session) works again.
  assert register(client, mint(sid='session-new')) != device_id


def test_heartbeat_updates_device(client, test_db):
  device_id = register(client)
  response = client.post('/v1/agent/heartbeat', headers=auth(mint()),
                         json={'agent_version': '0.1.1', 'queue_depth': 42, 'clock_offset_ms': -250})
  assert response.status_code == 200
  server_time = datetime.fromisoformat(response.json()['server_time'])
  assert abs(server_time - datetime.now(UTC)) < timedelta(seconds=5)
  device = test_db.admin.execute('SELECT * FROM devices WHERE id = %s', [device_id]).fetchone()
  assert (device['queue_depth'], device['clock_offset_ms'], device['agent_version']) == (42, -250, '0.1.1')
  assert device['last_seen_at'] is not None


def test_users_only_see_their_own_events(client, app_env):
  from secretary_warehouse.db import user_transaction

  register(client)
  register(client, mint(sub='kc-user-2', sid='session-u2', email='other@example.test'))
  for token, n in ((mint(), 3), (mint(sub='kc-user-2', sid='session-u2', email='other@example.test'), 2)):
    response = client.post('/v1/events:batch', json=batch([event() for _ in range(n)]), headers=auth(token))
    assert len(response.json()['accepted']) == n

  authenticator = client.app.state.authenticator
  for token, n in ((mint(), 3), (mint(sub='kc-user-2', sid='session-u2', email='other@example.test'), 2)):
    user_id = authenticator.device(f'Bearer {token}').user_id
    with user_transaction(app_env.db, user_id) as conn:
      assert conn.execute('SELECT count(*) AS n FROM activity_events').fetchone()['n'] == n
