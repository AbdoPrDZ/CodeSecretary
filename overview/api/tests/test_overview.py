"""Dashboard API: sign-in, sessions, CSRF, devices, activity, exclusions, isolation."""

from __future__ import annotations

import time
import uuid
from datetime import UTC, datetime, timedelta
from urllib.parse import parse_qs, urlsplit

import httpx
import jwt
import pytest
from cryptography.hazmat.primitives.asymmetric import rsa
from fastapi.testclient import TestClient

from secretary_overview.config import Config
from secretary_overview.oidc import Oidc, safe_next

ISSUER = 'http://kc.test/realms/secretary'
KEY = rsa.generate_private_key(public_exponent=65537, key_size=2048)


def make_config(**overrides) -> Config:
  values = dict(database_url='postgres://unused', oidc_client_secret='s3cret',
                session_secret='x' * 40, public_url='http://dash.test',
                keycloak_issuer=ISSUER, keycloak_internal_url=ISSUER)
  values.update(overrides)
  return Config(**values)


class StaticKeys:
  def get_signing_key_from_jwt(self, _token):
    return jwt.PyJWK.from_dict({**jwt.algorithms.RSAAlgorithm.to_jwk(KEY.public_key(), as_dict=True),
                                'kid': 'k1', 'alg': 'RS256'})


class FakeKeycloak:
  """Discovery + token endpoint; `user` decides who signs in next."""

  def __init__(self):
    self.user = {'sub': 'kc-user-1', 'email': 'dev@example.test', 'name': 'Dev'}
    self.nonce = None
    self.last_token_form = None

  def handler(self, request: httpx.Request) -> httpx.Response:
    if request.url.path.endswith('/.well-known/openid-configuration'):
      return httpx.Response(200, json={
        'issuer': ISSUER,
        'authorization_endpoint': 'http://kc.public/realms/secretary/protocol/openid-connect/auth',
        'token_endpoint': ISSUER + '/protocol/openid-connect/token',
        'jwks_uri': ISSUER + '/protocol/openid-connect/certs',
        'end_session_endpoint': 'http://kc.public/realms/secretary/protocol/openid-connect/logout',
      })
    if request.url.path.endswith('/token'):
      form = parse_qs(request.content.decode())
      self.last_token_form = form
      if form.get('code') != ['good-code']:
        return httpx.Response(400, json={'error': 'invalid_grant'})
      now = int(time.time())
      claims = {'iss': ISSUER, 'aud': 'secretary-dashboard', 'iat': now, 'exp': now + 300,
                'nonce': self.nonce, 'sid': 'kc-session-' + self.user['sub'], **self.user}
      token = jwt.encode(claims, KEY, algorithm='RS256', headers={'kid': 'k1'})
      return httpx.Response(200, json={'id_token': token, 'access_token': 'unused'})
    return httpx.Response(404)


@pytest.fixture
def keycloak():
  return FakeKeycloak()


@pytest.fixture
def client(app_env, keycloak):
  from secretary_overview.app import create_app

  config = make_config()
  oidc = Oidc(config, http=httpx.Client(transport=httpx.MockTransport(keycloak.handler)),
              keys=StaticKeys())
  with TestClient(create_app(app_env, config, oidc), base_url='http://dash.test') as c:
    yield c


def sign_in(client, keycloak, **user) -> dict:
  """Run the browser flow; returns /api/me (with the CSRF token)."""

  if user:
    keycloak.user = user
  client.cookies.clear()
  response = client.get('/auth/login', params={'next': '/devices'}, follow_redirects=False)
  assert response.status_code == 302
  query = parse_qs(urlsplit(response.headers['location']).query)
  keycloak.nonce = query['nonce'][0]
  response = client.get('/auth/callback', params={'code': 'good-code', 'state': query['state'][0]},
                        follow_redirects=False)
  assert response.status_code == 302, response.text
  assert response.headers['location'] == '/devices'
  me = client.get('/api/me')
  assert me.status_code == 200
  return me.json()


def headers(me) -> dict:
  return {'X-CSRF-Token': me['csrf_token']}


# ------------------------------------------------------------- offline


def test_safe_next_only_allows_local_paths():
  assert safe_next('/activity?day=2026-11-20') == '/activity?day=2026-11-20'
  for bad in [None, '', 'https://evil.test', '//evil.test', '/\\evil.test', 'activity']:
    assert safe_next(bad) == '/'


def test_config_requires_secrets_and_hides_them():
  with pytest.raises(ValueError):
    Config.from_lookup({'DATABASE_URL': 'x', 'OIDC_CLIENT_SECRET': 'y', 'SESSION_SECRET': 'short'})
  config = Config.from_lookup({'DATABASE_URL': 'postgres://u:pw@h/db', 'OIDC_CLIENT_SECRET': 'topsecret',
                               'SESSION_SECRET': 'z' * 40, 'PUBLIC_URL': 'https://dash.example/'})
  assert config.secure_cookies and config.redirect_uri == 'https://dash.example/auth/callback'
  assert 'pw@' not in repr(config) and 'topsecret' not in repr(config) and 'zzzz' not in repr(config)


# ------------------------------------------------------------- with database


def test_login_redirect_uses_pkce_and_a_signed_state_cookie(client):
  response = client.get('/auth/login', follow_redirects=False)
  location = urlsplit(response.headers['location'])
  query = parse_qs(location.query)
  assert location.netloc == 'kc.public'
  assert query['code_challenge_method'] == ['S256'] and query['code_challenge'][0]
  assert query['redirect_uri'] == ['http://dash.test/auth/callback']
  assert query['client_id'] == ['secretary-dashboard']
  cookie = response.headers['set-cookie']
  assert 'sec_login=' in cookie and 'HttpOnly' in cookie and 'Path=/auth' in cookie


def test_sign_in_provisions_the_user_and_starts_a_session(client, keycloak, test_db):
  me = sign_in(client, keycloak)
  assert me['email'] == 'dev@example.test' and len(me['csrf_token']) > 20
  form = keycloak.last_token_form
  assert form['code_verifier'][0] and form['client_secret'] == ['s3cret']
  row = test_db.admin.execute('SELECT token_hash, auth_session_id FROM web_sessions').fetchone()
  cookie = client.cookies.get('sec_session')
  assert cookie and row['token_hash'] != cookie, 'only the hash is stored'
  assert row['auth_session_id'] == 'kc-session-kc-user-1'
  workspaces = test_db.admin.execute("SELECT count(*) AS n FROM workspaces WHERE kind = 'personal'").fetchone()
  assert workspaces['n'] == 1


def test_bad_state_or_code_is_rejected(client, keycloak):
  response = client.get('/auth/login', follow_redirects=False)
  state = parse_qs(urlsplit(response.headers['location']).query)['state'][0]
  assert client.get('/auth/callback', params={'code': 'good-code', 'state': 'other'}).status_code == 400
  assert client.get('/auth/callback', params={'code': 'bad-code', 'state': state}).status_code == 400
  client.cookies.clear()
  assert client.get('/auth/callback', params={'code': 'good-code', 'state': state}).status_code == 400
  assert client.get('/api/me').json() == {'error': 'signed_out'}


def test_wrong_nonce_is_rejected(client, keycloak):
  response = client.get('/auth/login', follow_redirects=False)
  state = parse_qs(urlsplit(response.headers['location']).query)['state'][0]
  keycloak.nonce = 'replayed'
  response = client.get('/auth/callback', params={'code': 'good-code', 'state': state})
  assert response.status_code == 400 and response.json()['reason'] == 'nonce_mismatch'


def test_mutations_need_csrf_and_same_origin(client, keycloak):
  me = sign_in(client, keycloak)
  body = {'kind': 'app', 'pattern': 'Slack'}
  assert client.post('/api/exclusions', json=body).json() == {'error': 'bad_csrf'}
  assert client.post('/api/exclusions', json=body, headers={'X-CSRF-Token': 'guess'}).status_code == 403
  evil = {**headers(me), 'Origin': 'http://evil.test'}
  assert client.post('/api/exclusions', json=body, headers=evil).json() == {'error': 'bad_origin'}
  assert client.post('/api/exclusions', json=body, headers=headers(me)).status_code == 201


def test_logout_ends_the_session(client, keycloak, test_db):
  me = sign_in(client, keycloak)
  response = client.post('/auth/logout', headers=headers(me))
  assert response.status_code == 200
  assert response.json()['logout_url'].startswith('http://kc.public/')
  assert 'id_token_hint=' in response.json()['logout_url']
  assert test_db.admin.execute('SELECT count(*) AS n FROM web_sessions').fetchone()['n'] == 0
  assert client.get('/api/me').status_code == 401


def test_expired_sessions_are_refused(client, keycloak, test_db):
  sign_in(client, keycloak)
  test_db.admin.execute("UPDATE web_sessions SET expires_at = now() - interval '1 minute'")
  assert client.get('/api/me').status_code == 401


def _device(test_db, user_email: str, name: str, sid: str) -> int:
  user = test_db.admin.execute('SELECT id FROM users WHERE email = %s', [user_email]).fetchone()
  return test_db.admin.execute(
    """INSERT INTO devices (user_id, name, os, agent_version, auth_session_id, clock_offset_ms, last_seen_at)
       VALUES (%s, %s, 'windows', '0.1.0', %s, 0, now()) RETURNING id""",
    [user['id'], name, sid],
  ).fetchone()['id']


def test_devices_list_rename_revoke(client, keycloak, test_db):
  me = sign_in(client, keycloak)
  device = _device(test_db, 'dev@example.test', 'laptop', 'agent-sid-1')
  listed = client.get('/api/devices').json()['devices']
  assert [d['name'] for d in listed] == ['laptop'] and listed[0]['revoked_at'] is None
  assert client.patch(f'/api/devices/{device}', json={'name': ' work laptop '},
                      headers=headers(me)).status_code == 200
  assert client.patch(f'/api/devices/{device}', json={'name': ''}, headers=headers(me)).status_code == 400
  response = client.post(f'/api/devices/{device}/revoke', headers=headers(me))
  assert response.status_code == 200 and response.json()['revoked_at']
  row = test_db.admin.execute('SELECT name, revoked_at FROM devices WHERE id = %s', [device]).fetchone()
  assert row['name'] == 'work laptop' and row['revoked_at'] is not None


def test_revoked_device_is_rejected_by_the_warehouse(client, keycloak, test_db, app_env):
  """OV-4 acceptance, end to end against the warehouse app on the same database."""

  from secretary_warehouse.auth import AuthSettings, TokenVerifier
  from secretary_warehouse.http import create_app as warehouse_app

  settings = AuthSettings(issuer=ISSUER)

  class Keys:
    def get_signing_key_from_jwt(self, _t):
      return StaticKeys().get_signing_key_from_jwt(_t)

  def agent_token():
    now = int(time.time())
    return jwt.encode({'iss': ISSUER, 'aud': ['secretary-warehouse'], 'azp': 'secretary-agent',
                       'typ': 'Bearer', 'sub': 'kc-user-1', 'sid': 'agent-sid-9',
                       'email': 'dev@example.test', 'iat': now, 'exp': now + 300},
                      KEY, algorithm='RS256', headers={'kid': 'k1'})

  me = sign_in(client, keycloak)
  with TestClient(warehouse_app(app_env, settings, TokenVerifier(settings, Keys()))) as warehouse:
    auth = {'Authorization': f'Bearer {agent_token()}'}
    registered = warehouse.post('/v1/devices', json={'name': 'pc', 'os': 'windows', 'agent_version': '0.1.0'},
                                headers=auth)
    device_id = registered.json()['device_id']
    beat = {'agent_version': '0.1.0', 'queue_depth': 0, 'clock_offset_ms': 0}
    assert warehouse.post('/v1/agent/heartbeat', json=beat, headers=auth).status_code == 200

    assert client.post(f'/api/devices/{device_id}/revoke', headers=headers(me)).status_code == 200
    warehouse.app.state.authenticator.forget()  # its 60 s cache, as after a minute
    response = warehouse.post('/v1/agent/heartbeat', json=beat, headers=auth)
    assert response.status_code == 401 and response.json() == {'error': 'device_revoked'}


def _event(test_db, email, device, start, end, event_type='presence.active'):
  user = test_db.admin.execute('SELECT id FROM users WHERE email = %s', [email]).fetchone()['id']
  test_db.admin.execute(
    """INSERT INTO activity_events (event_id, user_id, device_id, batch_id, schema_version, event_type,
         source, timestamp_start, timestamp_end, server_adjusted_start, server_adjusted_end, metadata)
       VALUES (%s, %s, %s, %s, 1, %s, 'agent@0.1.0', %s, %s, %s, %s, '{}')""",
    [str(uuid.uuid4()), user, device, str(uuid.uuid4()), event_type, start, end, start, end],
  )


def test_activity_returns_the_local_day_raw(client, keycloak, test_db):
  sign_in(client, keycloak)
  device = _device(test_db, 'dev@example.test', 'laptop', 'agent-sid-1')
  t = lambda s: datetime.fromisoformat(s)  # noqa: E731
  # Africa/Algiers is UTC+1: the local day 2026-11-20 is 2026-11-19T23:00Z .. 2026-11-20T23:00Z.
  _event(test_db, 'dev@example.test', device, t('2026-11-19T22:30:00+00:00'), t('2026-11-19T22:50:00+00:00'))
  _event(test_db, 'dev@example.test', device, t('2026-11-19T22:55:00+00:00'), t('2026-11-19T23:10:00+00:00'))
  _event(test_db, 'dev@example.test', device, t('2026-11-20T09:00:00+00:00'), t('2026-11-20T09:30:00+00:00'),
         'presence.idle')
  _event(test_db, 'dev@example.test', device, t('2026-11-20T23:30:00+00:00'), t('2026-11-20T23:40:00+00:00'))

  body = client.get('/api/activity', params={'day': '2026-11-20', 'tz': 'Africa/Algiers'}).json()
  assert body['range'] == {'start': '2026-11-19T23:00:00Z', 'end': '2026-11-20T23:00:00Z'}
  assert [(e['start'], e['event_type']) for e in body['events']] == [
    ('2026-11-19T22:55:00Z', 'presence.active'),  # overlaps the day start
    ('2026-11-20T09:00:00Z', 'presence.idle'),
  ]
  assert body['devices'][0]['name'] == 'laptop' and body['truncated'] is False
  assert 'total' not in str(body), 'the dashboard never computes time'
  assert client.get('/api/activity', params={'day': 'tomorrow'}).status_code == 400
  assert client.get('/api/activity', params={'day': '2026-11-20', 'tz': 'Mars/Base'}).status_code == 400


def test_exclusions_crud_reaches_the_agent_config(client, keycloak, test_db, app_env):
  me = sign_in(client, keycloak)
  h = headers(me)
  assert client.post('/api/exclusions', json={'kind': 'path', 'pattern': ' ~/private '}, headers=h).status_code == 201
  window = client.post('/api/exclusions', json={'kind': 'time_window', 'start': '12:00', 'end': '13:00',
                                                'days': [5, 1, 1]}, headers=h)
  assert window.status_code == 201 and window.json()['days'] == [1, 5]
  for bad in [{'kind': 'path', 'pattern': '  '}, {'kind': 'everything', 'pattern': 'x'},
              {'kind': 'time_window', 'start': '25:00', 'end': '13:00'},
              {'kind': 'time_window', 'start': '12:00', 'end': '12:00'},
              {'kind': 'time_window', 'start': '12:00', 'end': '13:00', 'days': [8]},
              {'kind': 'app', 'pattern': 'x', 'user_id': 99}]:
    assert client.post('/api/exclusions', json=bad, headers=h).status_code == 400, bad

  listed = client.get('/api/exclusions').json()['exclusions']
  assert [(e['kind'], e['pattern'], e['start']) for e in listed] == [
    ('path', '~/private', None), ('time_window', None, '12:00')]

  from secretary_warehouse import agent_config

  user_id = test_db.admin.execute('SELECT id FROM users').fetchone()['id']
  config = agent_config.build(app_env.db, user_id)
  assert config['exclusions']['paths'] == ['~/private']
  assert config['pause_windows'] == [{'start': '12:00', 'end': '13:00', 'days': [1, 5]}]

  assert client.delete(f"/api/exclusions/{listed[0]['id']}", headers=h).status_code == 200
  assert client.delete(f"/api/exclusions/{listed[0]['id']}", headers=h).status_code == 404
  assert agent_config.build(app_env.db, user_id)['exclusions']['paths'] == []


def test_users_never_see_each_other(client, keycloak, test_db):
  me_a = sign_in(client, keycloak)
  device_a = _device(test_db, 'dev@example.test', 'a-laptop', 'agent-sid-a')
  _event(test_db, 'dev@example.test', device_a, datetime.now(UTC) - timedelta(hours=1), datetime.now(UTC))
  client.post('/api/exclusions', json={'kind': 'app', 'pattern': 'Secret'}, headers=headers(me_a))
  exclusion_a = client.get('/api/exclusions').json()['exclusions'][0]['id']

  me_b = sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  today = datetime.now(UTC).date().isoformat()
  assert client.get('/api/devices').json()['devices'] == []
  assert client.get('/api/activity', params={'day': today}).json()['events'] == []
  assert client.get('/api/exclusions').json()['exclusions'] == []
  h = headers(me_b)
  assert client.post(f'/api/devices/{device_a}/revoke', headers=h).status_code == 404
  assert client.patch(f'/api/devices/{device_a}', json={'name': 'mine'}, headers=h).status_code == 404
  assert client.delete(f'/api/exclusions/{exclusion_a}', headers=h).status_code == 404
  row = test_db.admin.execute('SELECT name, revoked_at FROM devices WHERE id = %s', [device_a]).fetchone()
  assert row['name'] == 'a-laptop' and row['revoked_at'] is None


def test_security_headers_and_unknown_api_routes(client):
  response = client.get('/healthz')
  assert "frame-ancestors 'none'" in response.headers['content-security-policy']
  assert response.headers['x-content-type-options'] == 'nosniff'
  assert client.get('/api/nope').json() == {'error': 'not_found'}


def test_sign_in_never_reuses_a_planted_session_cookie(client, keycloak, test_db):
  """Session fixation: a cookie set before sign-in is replaced, never adopted."""

  client.cookies.set('sec_session', 'attacker-chosen-token', domain='dash.test')
  assert client.get('/api/me').status_code == 401
  sign_in(client, keycloak)
  # sign_in clears and re-runs the flow; repeat with the planted cookie kept:
  client.cookies.set('sec_session', 'attacker-chosen-token', domain='dash.test')
  response = client.get('/auth/login', follow_redirects=False)
  query = parse_qs(urlsplit(response.headers['location']).query)
  keycloak.nonce = query['nonce'][0]
  response = client.get('/auth/callback', params={'code': 'good-code', 'state': query['state'][0]},
                        follow_redirects=False)
  issued = response.cookies.get('sec_session')
  assert issued and issued != 'attacker-chosen-token'
  import hashlib
  hashes = {r['token_hash'] for r in test_db.admin.execute('SELECT token_hash FROM web_sessions').fetchall()}
  assert hashlib.sha256(b'attacker-chosen-token').hexdigest() not in hashes
