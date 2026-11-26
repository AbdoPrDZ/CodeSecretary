"""Privacy export and delete (OV-12, WH-12, WH-13): the export matches the
database, and a delete removes the events and everything derived from them
for exactly the chosen scope, then queues the affected days."""

from __future__ import annotations

import json
from datetime import UTC, date, datetime, timedelta

import psycopg
import pytest
from test_corrections import _event, _project, _session, _uid
from test_overview import _device, client, headers, keycloak, sign_in  # noqa: F401 - fixtures

EMAIL = 'dev@example.test'
D1, D2 = date(2026, 11, 16), date(2026, 11, 17)
T1, T2 = datetime(2026, 11, 16, 9, tzinfo=UTC), datetime(2026, 11, 17, 9, tzinfo=UTC)


def _count(test_db, table: str, uid: int) -> int:
  return test_db.admin.execute(f'SELECT count(*) AS n FROM {table} WHERE user_id = %s', [uid]).fetchone()['n']  # noqa: S608


def _seed(test_db, email=EMAIL, sid='agent-sid-p'):
  uid = _uid(test_db, email)
  laptop = _device(test_db, email, 'laptop', sid)
  desktop = _device(test_db, email, 'desktop', sid + '-2')
  api = _project(test_db, uid, 'API')
  ids = {
    'd1_laptop': _event(test_db, uid, laptop, 'window.focus', T1, 20, {'app': 'Code.exe', 'title': 'main.rs'}),
    'd1_desktop': _event(test_db, uid, desktop, 'window.focus', T1 + timedelta(hours=1), 20, {'app': 'zsh'}),
    'd2_laptop': _event(test_db, uid, laptop, 'window.focus', T2, 30, {'app': 'Code.exe', 'title': 'lib.rs'}),
  }
  for key, day in (('d1_laptop', D1), ('d1_desktop', D1), ('d2_laptop', D2)):
    test_db.admin.execute(
      """INSERT INTO event_assignments (user_id, event_id, local_day, project_id, confidence, method,
                                        algorithm_version) VALUES (%s, %s, %s, %s, 0.7, 'temporal', 2)""",
      [uid, ids[key], day, api])
  test_db.admin.execute(
    'INSERT INTO assignment_overrides (user_id, event_id, local_day, project_id) VALUES (%s, %s, %s, %s)',
    [uid, ids['d1_laptop'], D1, api])
  test_db.admin.execute(
    """INSERT INTO time_adjustments (user_id, local_day, kind, starts_at, ends_at, project_id)
       VALUES (%s, %s, 'add', %s, %s, %s)""", [uid, D1, T1 + timedelta(hours=5), T1 + timedelta(hours=6), api])
  _session(test_db, uid, api, T1, 80, [ids['d1_laptop'], ids['d1_desktop']])
  _session(test_db, uid, api, T2, 30, [ids['d2_laptop']])
  return uid, laptop, desktop, ids


def _jobs(test_db, uid) -> list[date]:
  return [r['local_day'] for r in test_db.admin.execute(
    "SELECT local_day FROM processing_jobs WHERE user_id = %s AND reason = 'privacy_delete' ORDER BY local_day",
    [uid]).fetchall()]


def test_export_matches_the_database(client, keycloak, test_db):
  sign_in(client, keycloak)
  uid, _laptop, _desktop, ids = _seed(test_db)
  r = client.get('/api/privacy/export.ndjson')
  assert r.status_code == 200 and r.headers['content-type'].startswith('application/x-ndjson')
  assert r.headers['content-disposition'].startswith('attachment; filename="codesecretary-events-')
  lines = [json.loads(line) for line in r.text.splitlines()]
  assert [line['event_id'] for line in lines] == [ids['d1_laptop'], ids['d1_desktop'], ids['d2_laptop']]
  assert lines[0]['metadata'] == {'app': 'Code.exe', 'title': 'main.rs'} and lines[0]['device_name'] == 'laptop'
  assert lines[0]['timestamp_start'] == T1.isoformat()
  assert len(lines) == _count(test_db, 'activity_events', uid)


def test_delete_one_day(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  uid, _laptop, _desktop, ids = _seed(test_db)
  assert client.post('/api/privacy/delete', json={'scope': 'day', 'day': '2026-11-16', 'confirm': True}).status_code == 403
  assert client.post('/api/privacy/delete', json={'scope': 'day', 'day': '2026-11-16'}, headers=h).json() == \
    {'error': 'confirm_required'}
  r = client.post('/api/privacy/delete', json={'scope': 'day', 'day': '2026-11-16', 'confirm': True}, headers=h)
  assert r.json() == {'deleted_events': 2, 'queued_days': 3}
  left = [r['event_id'] for r in test_db.admin.execute('SELECT event_id::text FROM activity_events').fetchall()]
  assert left == [ids['d2_laptop']]
  # Derived rows of the deleted events are gone at once; the day's sessions are rewritten by the engine.
  assert [str(r['event_id']) for r in test_db.admin.execute('SELECT event_id FROM event_assignments').fetchall()] == \
    [ids['d2_laptop']]
  assert [str(r['event_id']) for r in test_db.admin.execute('SELECT event_id FROM session_evidence').fetchall()] == \
    [ids['d2_laptop']]
  assert _count(test_db, 'assignment_overrides', uid) == 0
  assert _count(test_db, 'time_adjustments', uid) == 1  # the user's own correction stays
  assert _jobs(test_db, uid) == [date(2026, 11, 15), D1, D2]


def test_delete_one_device(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  uid, laptop, desktop, ids = _seed(test_db)
  r = client.post('/api/privacy/delete', json={'scope': 'device', 'device_id': laptop, 'confirm': True}, headers=h)
  assert r.json() == {'deleted_events': 2, 'queued_days': 2}
  left = [r['event_id'] for r in test_db.admin.execute('SELECT event_id::text FROM activity_events').fetchall()]
  assert left == [ids['d1_desktop']]
  assert _jobs(test_db, uid) == [D1, D2]
  assert test_db.admin.execute('SELECT count(*) AS n FROM devices WHERE id = %s', [laptop]).fetchone()['n'] == 1


def test_delete_everything(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  uid, *_ = _seed(test_db)
  r = client.post('/api/privacy/delete', json={'scope': 'all', 'confirm': True}, headers=h)
  assert r.json() == {'deleted_events': 3, 'queued_days': 0}
  for table in ('activity_events', 'event_assignments', 'work_sessions', 'session_evidence',
                'assignment_overrides', 'time_adjustments', 'processing_jobs'):
    assert _count(test_db, table, uid) == 0, table
  # Settings and the account stay.
  assert _count(test_db, 'devices', uid) == 2
  assert client.get('/api/projects').json()['projects'][0]['name'] == 'API'
  assert client.get('/api/privacy/export.ndjson').text == ''


def test_delete_is_limited_to_the_caller(client, keycloak, test_db):
  sign_in(client, keycloak)
  owner, laptop, *_ = _seed(test_db)
  h = headers(sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other'))
  other = _uid(test_db, 'other@example.test')
  assert client.get('/api/privacy/export.ndjson').text == ''
  # Someone else's device: 404, nothing deleted.
  r = client.post('/api/privacy/delete', json={'scope': 'device', 'device_id': laptop, 'confirm': True}, headers=h)
  assert r.status_code == 404
  for scope in ({'scope': 'day', 'day': '2026-11-16'}, {'scope': 'all'}):
    assert client.post('/api/privacy/delete', json={**scope, 'confirm': True}, headers=h).json()['deleted_events'] == 0
  assert _count(test_db, 'activity_events', owner) == 3
  assert _count(test_db, 'work_sessions', owner) == 2
  assert _jobs(test_db, owner) == [] and other != owner
  for bad in ({'scope': 'week'}, {'scope': 'day'}, {'scope': 'device'}):
    assert client.post('/api/privacy/delete', json={**bad, 'confirm': True}, headers=h).status_code == 400


def test_the_app_role_cannot_delete_events_directly(client, keycloak, test_db):
  sign_in(client, keycloak)
  uid, *_ = _seed(test_db)
  with psycopg.connect(test_db.app_dsn, autocommit=True) as conn:
    conn.execute("SELECT set_config('app.user_id', %s, false)", [str(uid)])
    with pytest.raises(psycopg.errors.InsufficientPrivilege):
      conn.execute('DELETE FROM activity_events')
    # Without a tenant the procedure refuses.
    conn.execute("SELECT set_config('app.user_id', '', false)")
    with pytest.raises(psycopg.Error):
      conn.execute("SELECT * FROM app_privacy_delete('all', NULL, NULL)")
  assert _count(test_db, 'activity_events', uid) == 3
