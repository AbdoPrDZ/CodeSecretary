"""Reports and CSV export (OV-11): CSV totals equal the dashboard totals."""

from __future__ import annotations

import csv
import io
from datetime import UTC, date, datetime, timedelta

from test_overview import client, keycloak, sign_in  # noqa: F401 - fixtures
from test_time import _project, _session, _user


def _csv(response) -> list[list[str]]:
  assert response.status_code == 200, response.text
  assert response.headers['content-type'].startswith('text/csv')
  assert response.headers['content-disposition'].startswith('attachment; filename="codesecretary-')
  return list(csv.reader(io.StringIO(response.content.decode('utf-8-sig'))))


def _seed(test_db, uid):
  api, web = _project(test_db, uid, 'API'), _project(test_db, uid, '=cmd')  # formula-looking name
  t = datetime(2026, 11, 16, 9, tzinfo=UTC)
  _session(test_db, uid, date(2026, 11, 16), api, t, 90)
  _session(test_db, uid, date(2026, 11, 16), web, t + timedelta(hours=2), 30)
  _session(test_db, uid, date(2026, 11, 16), None, t + timedelta(hours=3), 15)
  _session(test_db, uid, date(2026, 11, 17), api, t + timedelta(days=1), 45)
  _session(test_db, uid, date(2026, 11, 17), api, t + timedelta(days=1, hours=1), 5, kind='waiting')
  return api, web


def test_csv_totals_equal_dashboard_totals(client, keycloak, test_db):
  sign_in(client, keycloak)
  uid = _user(test_db)
  api, web = _seed(test_db, uid)
  rng = {'start': '2026-11-01', 'end': '2026-11-30'}
  dashboard = client.get('/api/time', params=rng).json()
  report = client.get('/api/reports', params=rng).json()
  assert report['total_seconds'] == dashboard['total_seconds'] == 180 * 60

  rows = _csv(client.get('/api/reports/export.csv', params={**rng, 'group': 'project'}))
  assert rows[0] == ['project', 'sessions', 'days', 'hours', 'seconds']
  assert rows[1:] == [['API', '2', '2', '2.25', '8100'], ["'=cmd", '1', '1', '0.50', '1800'],
                      ['Unassigned', '1', '1', '0.25', '900'], ['Total', '', '', '3.00', '10800']]

  for group in ('day', 'session'):
    rows = _csv(client.get('/api/reports/export.csv', params={**rng, 'group': group}))
    assert rows[-1][0] == 'Total' and int(rows[-1][-2 if group == 'session' else -1]) == dashboard['total_seconds']
    body = rows[1:-1]
    seconds = [int(r[-2] if group == 'session' else r[-1]) for r in body]
    assert sum(seconds) == dashboard['total_seconds']
  # Session rows carry local start and end times in the user's zone.
  assert body[0][:4] == ['2026-11-16', 'API', '2026-11-16T09:00:00+00:00', '2026-11-16T10:30:00+00:00']

  # Project filter: the dashboard's per-project number, unassigned left out.
  rows = _csv(client.get('/api/reports/export.csv',
                         params={**rng, 'group': 'day', 'project': [api], 'unassigned': 'false'}))
  assert rows[1:] == [['2026-11-16', 'API', '1.50', '5400'], ['2026-11-17', 'API', '0.75', '2700'],
                      ['Total', '', '2.25', '8100']]
  api_total = next(p['seconds'] for p in dashboard['projects'] if p['project_id'] == api)
  assert int(rows[-1][-1]) == api_total
  only_api = client.get('/api/reports', params={**rng, 'project': [api], 'unassigned': 'false'}).json()
  assert (only_api['total_seconds'], only_api['unassigned_seconds']) == (api_total, 0)
  assert [p['name'] for p in only_api['projects']] == ['API']
  assert only_api['days'][15]['total_seconds'] == 5400 and only_api['filter'] == {'projects': [api], 'unassigned': False}
  # Unassigned only.
  rows = _csv(client.get('/api/reports/export.csv', params={**rng, 'group': 'session', 'project': [-1]}))
  assert [r[1] for r in rows[1:-1]] == ['Unassigned'] and rows[-1][-2] == '900'


def test_report_ranges_and_errors(client, keycloak, test_db):
  sign_in(client, keycloak)
  # A year is a valid report; /api/time stays limited to a quarter.
  assert client.get('/api/reports', params={'start': '2026-01-01', 'end': '2026-12-31'}).status_code == 200
  assert client.get('/api/time', params={'start': '2026-01-01', 'end': '2026-12-31'}).status_code == 400
  assert client.get('/api/reports', params={'start': '2025-01-01', 'end': '2026-12-31'}).status_code == 400
  bad = client.get('/api/reports/export.csv', params={'start': '2026-11-01', 'end': '2026-11-30', 'group': 'x'})
  assert bad.status_code == 400 and bad.json() == {'error': 'invalid_group'}


def test_reports_are_private(client, keycloak, test_db):
  sign_in(client, keycloak)
  _seed(test_db, _user(test_db))
  sign_in(client, keycloak, sub='kc-user-2', email='other@example.test', name='Other')
  rng = {'start': '2026-11-01', 'end': '2026-11-30'}
  assert client.get('/api/reports', params=rng).json()['total_seconds'] == 0
  for group in ('project', 'day', 'session'):
    assert _csv(client.get('/api/reports/export.csv', params={**rng, 'group': group}))[1:][0][0] == 'Total'
  client.cookies.clear()
  assert client.get('/api/reports/export.csv', params=rng).status_code == 401
