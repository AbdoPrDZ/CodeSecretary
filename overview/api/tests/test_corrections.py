"""Session detail, corrections and the inbox (OV-8, OV-9, OV-10)."""

from __future__ import annotations

import json
import uuid
from datetime import UTC, date, datetime, timedelta

from test_overview import _device, client, headers, keycloak, sign_in  # noqa: F401 - fixtures

EMAIL = 'dev@example.test'
DAY = date(2026, 11, 16)
T0 = datetime(2026, 11, 16, 9, tzinfo=UTC)


def _uid(test_db, email=EMAIL) -> int:
  return test_db.admin.execute('SELECT id FROM users WHERE email = %s', [email]).fetchone()['id']


def _project(test_db, uid: int, name: str) -> int:
  ws = test_db.admin.execute('SELECT id FROM workspaces WHERE personal_owner_id = %s', [uid]).fetchone()['id']
  return test_db.admin.execute('INSERT INTO projects (workspace_id, name) VALUES (%s, %s) RETURNING id',
                               [ws, name]).fetchone()['id']


def _event(test_db, uid, device, kind, start, minutes, metadata, root=None) -> str:
  event_id = str(uuid.uuid4())
  test_db.admin.execute(
    """INSERT INTO activity_events (event_id, user_id, device_id, batch_id, schema_version, event_type,
         source, timestamp_start, timestamp_end, server_adjusted_start, server_adjusted_end,
         repository_root, metadata)
       VALUES (%s, %s, %s, %s, 1, %s, 'agent@0.1.0', %s, %s, %s, %s, %s, %s::jsonb)""",
    [event_id, uid, device, str(uuid.uuid4()), kind, start, start + timedelta(minutes=minutes),
     start, start + timedelta(minutes=minutes), root, json.dumps(metadata)],
  )
  return event_id


def _session(test_db, uid, project, start, minutes, events) -> int:
  sid = test_db.admin.execute(
    """INSERT INTO work_sessions (user_id, local_day, project_id, kind, starts_at, ends_at,
                                  algorithm_version, config_hash)
       VALUES (%s, %s, %s, 'active', %s, %s, 2, 'h') RETURNING id""",
    [uid, start.date(), project, start, start + timedelta(minutes=minutes)],
  ).fetchone()['id']
  for e in events:
    test_db.admin.execute('INSERT INTO session_evidence (session_id, user_id, event_id) VALUES (%s, %s, %s)',
                          [sid, uid, e])
  return sid


def _seed(client, keycloak, test_db):
  me = sign_in(client, keycloak)
  uid = _uid(test_db)
  device = _device(test_db, EMAIL, 'laptop', 'agent-sid-c')
  api, web = _project(test_db, uid, 'API'), _project(test_db, uid, 'Web')
  rule = test_db.admin.execute(
    """INSERT INTO project_mappings (workspace_id, project_id, user_id, signal, pattern)
       SELECT workspace_id, id, %s, 'path', '/home/dev/api' FROM projects WHERE id = %s RETURNING id""",
    [uid, api]).fetchone()['id']
  editor = _event(test_db, uid, device, 'editor.file_focus', T0, 20, {'file_path': 'src/main.rs'}, root='/home/dev/api')
  test_db.admin.execute(
    """INSERT INTO event_assignments (user_id, event_id, local_day, project_id, confidence, method, rule_id,
                                      algorithm_version)
       VALUES (%s, %s, %s, %s, 0.95, 'rule', %s, 2)""", [uid, editor, DAY, api, rule])
  focus = _event(test_db, uid, device, 'window.focus', T0, 20, {'app': 'Code.exe', 'title': 'main.rs - api'})
  assigned = _session(test_db, uid, api, T0, 20, [editor, focus])
  chat = [_event(test_db, uid, device, 'window.focus', T0 + timedelta(minutes=30 + 10 * i), 10,
                 {'app': 'slack.exe', 'title': f'chat {i}'}) for i in range(2)]
  loose = [_session(test_db, uid, None, T0 + timedelta(minutes=30 + 10 * i), 10, [e]) for i, e in enumerate(chat)]
  web_ev = _event(test_db, uid, device, 'editor.file_focus', T0 + timedelta(hours=2), 15,
                  {'file_path': 'index.ts'}, root='/home/dev/web')
  unassigned_web = _session(test_db, uid, None, T0 + timedelta(hours=2), 15, [web_ev])
  return dict(me=me, uid=uid, api=api, web=web, rule=rule, editor=editor, focus=focus, assigned=assigned,
              loose=loose, chat=chat, unassigned_web=unassigned_web, web_ev=web_ev)


def _jobs(test_db) -> dict:
  return {r['local_day']: r for r in test_db.admin.execute(
    "SELECT local_day, reason, run_after <= now() AS due FROM processing_jobs WHERE status = 'queued'").fetchall()}


def test_session_detail_explains_itself(client, keycloak, test_db):
  s = _seed(client, keycloak, test_db)
  detail = client.get(f"/api/sessions/{s['assigned']}").json()
  assert detail['session']['project_name'] == 'API' and detail['session']['seconds'] == 1200
  assert detail['evidence_total'] == 2 and not detail['truncated']
  assert detail['methods'] == {'rule': 1, 'unassigned': 1}
  assert [r['pattern'] for r in detail['rules']] == ['/home/dev/api']
  editor = next(e for e in detail['events'] if e['event_type'] == 'editor.file_focus')
  assert (editor['repository_root'], editor['method'], editor['device_name']) == ('/home/dev/api', 'rule', 'laptop')
  assert client.get('/api/sessions/999999').status_code == 404

  sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  assert client.get(f"/api/sessions/{s['assigned']}").status_code == 404


def test_assign_sessions_and_events(client, keycloak, test_db):
  s = _seed(client, keycloak, test_db)
  h = headers(s['me'])
  r = client.post('/api/assign', json={'session_ids': s['loose'], 'project_id': s['web']}, headers=h)
  assert r.status_code == 200 and r.json() == {'events': 2}
  rows = test_db.admin.execute('SELECT event_id, local_day, project_id FROM assignment_overrides ORDER BY event_id').fetchall()
  assert {str(r['event_id']) for r in rows} == set(s['chat'])
  assert all(r['local_day'] == DAY and r['project_id'] == s['web'] for r in rows)
  jobs = _jobs(test_db)
  assert set(jobs) == {DAY - timedelta(days=1), DAY, DAY + timedelta(days=1)}
  assert all(j['due'] and j['reason'] == 'correction' for j in jobs.values())

  # "Not project work", then back to automatic.
  client.post('/api/assign', json={'event_ids': [s['chat'][0]], 'project_id': None}, headers=h)
  assert test_db.admin.execute('SELECT project_id FROM assignment_overrides WHERE event_id = %s',
                               [s['chat'][0]]).fetchone()['project_id'] is None
  client.post('/api/assign', json={'event_ids': s['chat'], 'clear': True}, headers=h)
  assert test_db.admin.execute('SELECT count(*) AS n FROM assignment_overrides').fetchone()['n'] == 0

  assert client.post('/api/assign', json={'session_ids': s['loose'], 'project_id': 999999},
                     headers=h).status_code == 404
  assert client.post('/api/assign', json={'session_ids': s['loose'], 'project_id': s['web']}).status_code == 403


def test_other_users_cannot_assign_my_events(client, keycloak, test_db):
  s = _seed(client, keycloak, test_db)
  other = sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  other_project = _project(test_db, _uid(test_db, 'other@example.test'), 'Theirs')
  r = client.post('/api/assign', json={'event_ids': s['chat'], 'session_ids': s['loose'],
                                       'project_id': other_project}, headers=headers(other))
  assert r.json() == {'events': 0}
  assert test_db.admin.execute('SELECT count(*) AS n FROM assignment_overrides').fetchone()['n'] == 0
  # Their project is not visible to me either.
  me = sign_in(client, keycloak, sub='kc-user-1', email=EMAIL, name='Dev')
  assert client.post('/api/assign', json={'session_ids': s['loose'], 'project_id': other_project},
                     headers=headers(me)).status_code == 404


def test_time_range_corrections_and_undo(client, keycloak, test_db):
  s = _seed(client, keycloak, test_db)
  h = headers(s['me'])
  iso = lambda t: t.isoformat().replace('+00:00', 'Z')  # noqa: E731
  # Split: move 10 minutes of the API session to Web.
  r = client.post('/api/adjustments', json={'kind': 'reassign', 'start': iso(T0 + timedelta(minutes=5)),
                                            'end': iso(T0 + timedelta(minutes=15)), 'project_id': s['web'],
                                            'from_project_id': s['api']}, headers=h)
  assert r.status_code == 201, r.text
  split = r.json()['ids'][0]
  # Manual time across midnight: one row per local day.
  late = datetime(2026, 11, 16, 23, 30, tzinfo=UTC)
  r = client.post('/api/adjustments', json={'kind': 'add', 'start': iso(late), 'end': iso(late + timedelta(hours=1)),
                                            'project_id': s['api'], 'note': ' call '}, headers=h)
  assert len(r.json()['ids']) == 2
  # Trim: remove ignores a project.
  assert client.post('/api/adjustments', json={'kind': 'remove', 'start': iso(T0), 'end': iso(T0 + timedelta(minutes=1)),
                                               'project_id': s['web']}, headers=h).status_code == 201

  day = client.get('/api/adjustments', params={'day': DAY.isoformat()}).json()['adjustments']
  assert [(a['kind'], a['project_name'], a['from_project_name']) for a in day] == \
    [('reassign', 'Web', 'API'), ('add', 'API', None), ('remove', None, None)]
  assert day[1]['note'] == 'call' and day[1]['ends_at'] == '2026-11-17T00:00:00Z'
  assert len(client.get('/api/adjustments', params={'day': '2026-11-17'}).json()['adjustments']) == 1
  assert set(_jobs(test_db)) == {DAY, DAY + timedelta(days=1)}

  assert client.delete(f'/api/adjustments/{split}', headers=h).status_code == 200
  assert client.delete(f'/api/adjustments/{split}', headers=h).status_code == 404
  assert len(client.get('/api/adjustments', params={'day': DAY.isoformat()}).json()['adjustments']) == 2

  for bad in ({'kind': 'reassign', 'start': iso(T0), 'end': iso(T0 + timedelta(minutes=5))},  # no project
              {'kind': 'add', 'start': iso(T0), 'end': iso(T0), 'project_id': s['api']},
              {'kind': 'add', 'start': iso(T0), 'end': iso(T0 + timedelta(hours=25)), 'project_id': s['api']},
              {'kind': 'split', 'start': iso(T0), 'end': iso(T0 + timedelta(minutes=5)), 'project_id': s['api']}):
    assert client.post('/api/adjustments', json=bad, headers=h).status_code == 400, bad

  sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  assert client.get('/api/adjustments', params={'day': DAY.isoformat()}).json()['adjustments'] == []


def test_inbox_groups_unassigned_time(client, keycloak, test_db):
  s = _seed(client, keycloak, test_db)
  body = client.get('/api/inbox', params={'start': '2026-11-16', 'end': '2026-11-22'}).json()
  assert body['sessions'] == 3 and body['total_seconds'] == 35 * 60
  assert [(g['signal'], g['value'], g['seconds'], len(g['sessions'])) for g in body['groups']] == \
    [('app', 'slack.exe', 1200, 2), ('path', '/home/dev/web', 900, 1)]
  assert sorted(body['groups'][0]['titles']) == ['chat 0', 'chat 1']
  sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  assert client.get('/api/inbox', params={'start': '2026-11-16', 'end': '2026-11-22'}).json()['groups'] == []
