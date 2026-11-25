"""Time per project from the engine's sessions (OV-7 first slice), the time
zone setting (OV-13) and the session lookup cost."""

from __future__ import annotations

from datetime import UTC, date, datetime, timedelta

from test_overview import client, headers, keycloak, sign_in  # noqa: F401 - fixtures

EMAIL = 'dev@example.test'


def _user(test_db, email=EMAIL) -> int:
  return test_db.admin.execute('SELECT id FROM users WHERE email = %s', [email]).fetchone()['id']


def _project(test_db, user_id: int, name: str) -> int:
  ws = test_db.admin.execute('SELECT id FROM workspaces WHERE personal_owner_id = %s', [user_id]).fetchone()['id']
  return test_db.admin.execute('INSERT INTO projects (workspace_id, name, color) VALUES (%s, %s, %s) RETURNING id',
                               [ws, name, '#22aa88']).fetchone()['id']


def _session(test_db, user_id, day: date, project, start: datetime, minutes: int, kind='active') -> int:
  sid = test_db.admin.execute(
    """INSERT INTO work_sessions (user_id, local_day, project_id, kind, starts_at, ends_at,
                                  algorithm_version, config_hash)
       VALUES (%s, %s, %s, %s, %s, %s, 1, 'h') RETURNING id""",
    [user_id, day, project, kind, start, start + timedelta(minutes=minutes)],
  ).fetchone()['id']
  test_db.admin.execute(
    "INSERT INTO session_evidence (session_id, user_id, event_id) VALUES (%s, %s, gen_random_uuid())",
    [sid, user_id])
  return sid


def test_time_per_project_and_day(client, keycloak, test_db):
  sign_in(client, keycloak)
  uid = _user(test_db)
  api, web = _project(test_db, uid, 'API'), _project(test_db, uid, 'Web')
  d1, d2 = date(2026, 11, 16), date(2026, 11, 17)
  t = datetime(2026, 11, 16, 9, tzinfo=UTC)
  _session(test_db, uid, d1, api, t, 90)
  _session(test_db, uid, d1, web, t + timedelta(hours=2), 30)
  _session(test_db, uid, d1, None, t + timedelta(hours=3), 15)
  _session(test_db, uid, d2, api, t + timedelta(days=1), 60)
  _session(test_db, uid, d2, api, t + timedelta(days=1, hours=2), 1, kind='waiting')  # not active time
  test_db.admin.execute(
    "INSERT INTO processing_jobs (user_id, local_day, reason, run_after) VALUES (%s, %s, 'ingest', now())",
    [uid, d2])

  body = client.get('/api/time', params={'start': '2026-11-16', 'end': '2026-11-22'}).json()
  assert (body['total_seconds'], body['assigned_seconds'], body['unassigned_seconds']) == \
    (195 * 60, 180 * 60, 15 * 60)
  assert [(p['name'], p['seconds'], p['sessions'], p['days']) for p in body['projects']] == \
    [('API', 150 * 60, 2, 2), ('Web', 30 * 60, 1, 1)]
  assert len(body['days']) == 7 and body['days'][2]['total_seconds'] == 0
  assert body['days'][0]['projects'] == [{'project_id': api, 'seconds': 5400}, {'project_id': web, 'seconds': 1800}]
  assert body['pending_days'] == 1 and body['time_zone'] == 'UTC'

  day = client.get('/api/sessions', params={'day': '2026-11-16'}).json()
  assert [(s['project_name'], s['seconds'], s['evidence']) for s in day['sessions']] == \
    [('API', 5400, 1), ('Web', 1800, 1), (None, 900, 1)]
  assert day['range'] == {'start': '2026-11-16T00:00:00Z', 'end': '2026-11-17T00:00:00Z'}

  for bad in ({'start': '2026-11-22', 'end': '2026-11-16'}, {'start': 'x', 'end': 'y'},
              {'start': '2026-01-01', 'end': '2026-12-31'}):
    assert client.get('/api/time', params=bad).status_code == 400


def test_time_is_private(client, keycloak, test_db):
  sign_in(client, keycloak)
  uid = _user(test_db)
  _session(test_db, uid, date(2026, 11, 16), _project(test_db, uid, 'API'),
           datetime(2026, 11, 16, 9, tzinfo=UTC), 60)
  sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  body = client.get('/api/time', params={'start': '2026-11-16', 'end': '2026-11-16'}).json()
  assert body['total_seconds'] == 0 and body['projects'] == [] and body['pending_days'] == 0
  assert client.get('/api/sessions', params={'day': '2026-11-16'}).json()['sessions'] == []


def test_time_zone_setting_requeues_recent_days(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  assert client.put('/api/me/time_zone', json={'time_zone': 'Mars/Olympus'}, headers=h).status_code == 400
  assert client.put('/api/me/time_zone', json={'time_zone': 'Africa/Algiers'}).status_code == 403  # CSRF
  assert client.put('/api/me/time_zone', json={'time_zone': 'Africa/Algiers'}, headers=h).status_code == 200
  assert client.get('/api/me').json()['time_zone'] == 'Africa/Algiers'
  jobs = test_db.admin.execute(
    "SELECT count(*) AS n FROM processing_jobs WHERE reason = 'time_zone_changed'").fetchone()['n']
  assert jobs == 30
  # Same zone again: nothing to recalculate.
  test_db.admin.execute('DELETE FROM processing_jobs')
  client.put('/api/me/time_zone', json={'time_zone': 'Africa/Algiers'}, headers=h)
  assert test_db.admin.execute('SELECT count(*) AS n FROM processing_jobs').fetchone()['n'] == 0
  day = client.get('/api/sessions', params={'day': '2026-11-16'}).json()
  assert day['range']['start'] == '2026-11-15T23:00:00Z'  # UTC+1


def test_session_lookup_writes_rarely_and_responses_are_compressed(client, keycloak, test_db):
  sign_in(client, keycloak)
  seen = lambda: test_db.admin.execute('SELECT last_seen_at FROM web_sessions').fetchone()['last_seen_at']  # noqa: E731
  first = seen()
  for _ in range(3):
    assert client.get('/api/me').status_code == 200
  assert seen() == first, 'no write per request'
  test_db.admin.execute("UPDATE web_sessions SET last_seen_at = now() - interval '10 minutes'")
  client.get('/api/me')
  assert seen() > first - timedelta(minutes=1)

  uid = _user(test_db)
  for i in range(40):
    _session(test_db, uid, date(2026, 11, 16), None, datetime(2026, 11, 16, 0, tzinfo=UTC) + timedelta(minutes=2 * i), 1)
  r = client.get('/api/sessions', params={'day': '2026-11-16'}, headers={'Accept-Encoding': 'gzip'})
  assert r.headers.get('content-encoding') == 'gzip'
  assert len(r.json()['sessions']) == 40
