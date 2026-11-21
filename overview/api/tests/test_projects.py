"""Projects, mapping rules, the match preview and suggestions (OV-5, D17)."""

from __future__ import annotations

import json
import uuid
from datetime import UTC, datetime, timedelta

from test_overview import _device, client, headers, keycloak, sign_in  # noqa: F401 - fixtures

EMAIL = 'dev@example.test'


def _event(test_db, device, event_type, metadata, root=None, remote=None, ago_hours=1):
  user = test_db.admin.execute('SELECT id FROM users WHERE email = %s', [EMAIL]).fetchone()['id']
  start = datetime.now(UTC) - timedelta(hours=ago_hours)
  test_db.admin.execute(
    """INSERT INTO activity_events (event_id, user_id, device_id, batch_id, schema_version, event_type,
         source, timestamp_start, timestamp_end, server_adjusted_start, server_adjusted_end,
         repository_root, repository_remote, metadata)
       VALUES (%s, %s, %s, %s, 1, %s, 'agent@0.1.0', %s, %s, %s, %s, %s, %s, %s::jsonb)""",
    [str(uuid.uuid4()), user, device, str(uuid.uuid4()), event_type, start, start + timedelta(minutes=1),
     start, start + timedelta(minutes=1), root, remote, json.dumps(metadata)],
  )


def _seed(test_db):
  device = _device(test_db, EMAIL, 'laptop', 'agent-sid-p')
  for _ in range(3):
    _event(test_db, device, 'window.focus', {'app': 'Code.exe', 'title': 'main.rs - api'})
  _event(test_db, device, 'window.focus', {'app': 'chrome.exe', 'title': 'Invoice 7 - Acme Portal'})
  _event(test_db, device, 'git.commit', {'hash': 'a' * 40, 'branch': 'main'},
         root='C:\\work\\acme\\api', remote='github.com/acme/api')
  _event(test_db, device, 'git.checkout', {'from_branch': 'main', 'to_branch': 'x'},
         root='/home/me/other', remote='gitlab.com/me/other')
  _event(test_db, device, 'window.focus', {'app': 'Code.exe', 'title': 'old'}, ago_hours=24 * 40)


def test_projects_crud(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  created = client.post('/api/projects', json={'name': ' Acme API ', 'color': '#22aa88',
                                               'client_name': 'Acme', 'hourly_rate': 85.5,
                                               'currency': 'eur'}, headers=h)
  assert created.status_code == 201, created.text
  project = created.json()
  assert (project['name'], project['hourly_rate'], project['currency'], project['rules']) == \
    ('Acme API', '85.50', 'EUR', [])
  for bad in ({'name': ' '}, {'name': 'x', 'color': 'red'}, {'name': 'x', 'currency': 'EURO'},
              {'name': 'x', 'hourly_rate': -1}, {'color': '#000000'}, {'name': 'x', 'owner': 1}):
    assert client.post('/api/projects', json=bad, headers=h).status_code == 400, bad

  assert client.patch(f"/api/projects/{project['id']}", json={'hourly_rate': None, 'archived': True},
                      headers=h).status_code == 200
  listed = client.get('/api/projects').json()['projects']
  assert listed[0]['hourly_rate'] is None and listed[0]['archived_at'] is not None
  assert client.delete(f"/api/projects/{project['id']}", headers=h).status_code == 200
  assert client.get('/api/projects').json()['projects'] == []
  assert client.delete(f"/api/projects/{project['id']}", headers=h).status_code == 404


def test_rules_are_normalized_and_queue_recalculation(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  project = client.post('/api/projects', json={'name': 'Acme'}, headers=h).json()
  url = f"/api/projects/{project['id']}/rules"
  rule = client.post(url, json={'signal': 'remote', 'pattern': 'git@github.com:Acme/API.git'}, headers=h)
  assert rule.status_code == 201, rule.text
  assert rule.json()['pattern'] == 'github.com/acme/api'
  assert client.post(url, json={'signal': 'remote', 'pattern': 'https://github.com/acme/api'},
                     headers=h).json() == {'error': 'duplicate_rule'}
  for bad in ({'signal': 'cpu', 'pattern': 'x'}, {'signal': 'path', 'pattern': '  '},
              {'signal': 'app', 'pattern': 'x', 'priority': 1000}):
    assert client.post(url, json=bad, headers=h).status_code == 400, bad

  jobs = test_db.admin.execute(
    "SELECT count(*) AS n, min(reason) AS reason FROM processing_jobs WHERE status = 'queued'").fetchone()
  assert (jobs['n'], jobs['reason']) == (30, 'rules_changed')
  # A second change does not duplicate the queued days.
  client.post(url, json={'signal': 'app', 'pattern': 'code'}, headers=h)
  assert test_db.admin.execute('SELECT count(*) AS n FROM processing_jobs').fetchone()['n'] == 30

  rules = client.get('/api/projects').json()['projects'][0]['rules']
  assert [(r['signal'], r['pattern'], r['shared']) for r in rules] == \
    [('remote', 'github.com/acme/api', False), ('app', 'code', False)]
  assert client.delete(f"/api/rules/{rules[0]['id']}", headers=h).status_code == 200
  assert client.delete(f"/api/rules/{rules[0]['id']}", headers=h).status_code == 404


def test_preview_counts_recent_matches_and_names_conflicts(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  _seed(test_db)
  preview = client.post('/api/rules/preview', json={'signal': 'app', 'pattern': 'code'}).status_code
  assert preview == 403, 'the preview reads data: it needs the CSRF token like a mutation'
  preview = client.post('/api/rules/preview', json={'signal': 'app', 'pattern': 'CODE'}, headers=h).json()
  # The 40-day-old event is outside the 30-day window.
  assert (preview['matched_events'], preview['distinct_values'], preview['conflicts']) == (3, 1, [])
  assert preview['examples'][0]['value'] == 'Code.exe' and preview['examples'][0]['events'] == 3
  assert client.post('/api/rules/preview', json={'signal': 'app', 'pattern': 'code', 'days': 60},
                     headers=h).json()['matched_events'] == 4

  title = client.post('/api/rules/preview', json={'signal': 'title', 'pattern': 'acme'}, headers=h).json()
  assert title['matched_events'] == 1
  path = client.post('/api/rules/preview', json={'signal': 'path', 'pattern': 'C:/work/acme'},
                     headers=h).json()
  assert path['matched_events'] == 1 and path['examples'][0]['value'] == 'C:\\work\\acme\\api'

  editor = client.post('/api/projects', json={'name': 'Editor time'}, headers=h).json()
  client.post(f"/api/projects/{editor['id']}/rules", json={'signal': 'title', 'pattern': 'main.rs'},
              headers=h)
  preview = client.post('/api/rules/preview', json={'signal': 'app', 'pattern': 'code'}, headers=h).json()
  assert preview['conflicts'] == [{'project_id': editor['id'], 'name': 'Editor time', 'events': 3}]
  # Rules of the project being edited are not conflicts.
  same = client.post('/api/rules/preview', json={'signal': 'app', 'pattern': 'code',
                                                 'project_id': editor['id']}, headers=h).json()
  assert same['conflicts'] == []


def test_suggestions_offer_uncovered_repositories(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  _seed(test_db)
  repos = client.get('/api/rules/suggestions').json()['repositories']
  assert sorted(r['remote'] for r in repos) == ['github.com/acme/api', 'gitlab.com/me/other']
  project = client.post('/api/projects', json={'name': 'Acme'}, headers=h).json()
  client.post(f"/api/projects/{project['id']}/rules", json={'signal': 'path', 'pattern': 'C:\\work\\acme'},
              headers=h)
  repos = client.get('/api/rules/suggestions').json()['repositories']
  assert [(r['remote'], r['name']) for r in repos] == [('gitlab.com/me/other', 'other')]


def test_other_users_cannot_see_or_touch_projects(client, keycloak, test_db):
  h = headers(sign_in(client, keycloak))
  _seed(test_db)
  project = client.post('/api/projects', json={'name': 'Mine'}, headers=h).json()
  rule = client.post(f"/api/projects/{project['id']}/rules", json={'signal': 'app', 'pattern': 'code'},
                     headers=h).json()

  hb = headers(sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other'))
  assert client.get('/api/projects').json()['projects'] == []
  assert client.get('/api/rules/suggestions').json()['repositories'] == []
  preview = client.post('/api/rules/preview', json={'signal': 'app', 'pattern': 'code'}, headers=hb).json()
  assert preview['matched_events'] == 0
  assert client.patch(f"/api/projects/{project['id']}", json={'name': 'x'}, headers=hb).status_code == 404
  assert client.delete(f"/api/projects/{project['id']}", headers=hb).status_code == 404
  assert client.post(f"/api/projects/{project['id']}/rules", json={'signal': 'app', 'pattern': 'x'},
                     headers=hb).status_code == 404
  assert client.delete(f"/api/rules/{rule['id']}", headers=hb).status_code == 404
