"""Row-level security and schema tests (WH-1R; same guarantees as WH-2/WH-3)."""

from __future__ import annotations

import uuid
from dataclasses import dataclass

import psycopg
import pytest

from secretary_warehouse.db import system_transaction, user_transaction

#: Tables intentionally without RLS, with the reason.
NO_RLS = {
  'processing_jobs',  # system queue; see schema/models/jobs.py
  'sys_model',        # ORM schema bookkeeping; owner only, not granted to the app role
  'sys_field',
}


# ---------------------------------------------------------------------------
# helpers


def count(conn, table: str) -> int:
  # `table` comes from pg_catalog / test constants, never user input.
  return conn.execute(f'SELECT count(*) AS n FROM "{table}"').fetchone()['n']


def insert_event(conn, user: int, device: int, event_id=None, at='now()') -> int:
  cur = conn.execute(
    f"""INSERT INTO activity_events
          (event_id, user_id, device_id, batch_id, schema_version, event_type, source,
           timestamp_start, server_adjusted_start)
        VALUES (%s, %s, %s, gen_random_uuid(), 1, 'presence.active', 'agent@0.1.0',
                {at}, {at})
        ON CONFLICT DO NOTHING""",
    [event_id or uuid.uuid4(), user, device],
  )
  return cur.rowcount


@dataclass
class World:
  a: int
  b: int
  a_device: int
  b_device: int
  a_personal_ws: int
  a_personal_project: int
  team_ws: int
  team_project: int


def create_user(admin, email: str, subject: str) -> tuple[int, int, int]:
  user = admin.execute(
    "SELECT app_resolve_identity('https://id.test/realms/secretary', %s, %s, NULL) AS id",
    [subject, email],
  ).fetchone()['id']
  ws = admin.execute('SELECT id FROM workspaces WHERE personal_owner_id = %s', [user]).fetchone()['id']
  device = admin.execute(
    "INSERT INTO devices (user_id, name, os, auth_session_id) VALUES (%s, 'laptop', 'linux', %s) RETURNING id",
    [user, str(uuid.uuid4())],
  ).fetchone()['id']
  return user, ws, device


def seed(admin) -> World:
  """Users A and B, each with a personal workspace, device, and 2 events.
  A owns team workspace T (B is a member) with one shared project.
  A has a personal project, a personal rule and a shared rule on T's project.
  B has a draft and a submitted submission for T's project."""

  a, a_ws, a_dev = create_user(admin, 'a@example.test', 'sub-a')
  b, _b_ws, b_dev = create_user(admin, 'b@example.test', 'sub-b')
  for user, device in [(a, a_dev), (a, a_dev), (b, b_dev), (b, b_dev)]:
    insert_event(admin, user, device)

  team_ws = admin.execute("INSERT INTO workspaces (kind, name) VALUES ('team', 'Acme') RETURNING id").fetchone()['id']
  admin.execute(
    "INSERT INTO memberships (workspace_id, user_id, role) VALUES (%s, %s, 'owner'), (%s, %s, 'member')",
    [team_ws, a, team_ws, b],
  )

  def project(ws, name):
    return admin.execute(
      'INSERT INTO projects (workspace_id, name) VALUES (%s, %s) RETURNING id', [ws, name]
    ).fetchone()['id']

  a_personal_project = project(a_ws, 'Side project')
  team_project = project(team_ws, 'Client Acme')

  admin.execute(
    """INSERT INTO project_mappings (workspace_id, project_id, user_id, signal, pattern)
       VALUES (%s, %s, %s, 'path', '/home/a/acme/*'), (%s, %s, NULL, 'remote', 'github.com/acme/*')""",
    [team_ws, team_project, a, team_ws, team_project],
  )
  admin.execute(
    """INSERT INTO submissions (workspace_id, project_id, user_id, period_start, period_end, total_seconds, status)
       VALUES (%s, %s, %s, '2026-11-02', '2026-11-08', 3600, 'draft'),
              (%s, %s, %s, '2026-10-26', '2026-11-01', 7200, 'submitted')""",
    [team_ws, team_project, b, team_ws, team_project, b],
  )
  return World(a, b, a_dev, b_dev, a_ws, a_personal_project, team_ws, team_project)


def tenant_tables(admin) -> list[str]:
  rows = admin.execute(
    """SELECT c.relname AS name FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
       WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p') AND NOT c.relispartition
       ORDER BY 1"""
  ).fetchall()
  return [row['name'] for row in rows if row['name'] not in NO_RLS]


# ---------------------------------------------------------------------------
# schema-wide guarantees


def test_every_tenant_table_has_rls_and_a_policy(test_db):
  tables = tenant_tables(test_db.admin)
  assert len(tables) >= 20, tables
  for table in tables:
    row = test_db.admin.execute(
      """SELECT c.relrowsecurity AS rls,
                (SELECT count(*) FROM pg_policies p WHERE p.tablename = c.relname) AS policies
         FROM pg_class c WHERE c.relname = %s AND c.relnamespace = 'public'::regnamespace""",
      [table],
    ).fetchone()
    assert row['rls'], f'{table}: RLS not enabled'
    assert row['policies'] > 0, f'{table}: no policy'


def test_tables_without_rls_are_not_readable_by_the_app_role(test_db, app_env):
  for table in NO_RLS - {'processing_jobs'}:
    with pytest.raises(psycopg.errors.InsufficientPrivilege):
      with system_transaction(app_env.db) as conn:
        count(conn, table)


def test_without_context_every_tenant_table_is_empty(test_db, app_env):
  seed(test_db.admin)
  with system_transaction(app_env.db) as conn:
    for table in tenant_tables(test_db.admin):
      assert count(conn, table) == 0, f'{table} visible without context'


def test_context_does_not_leak_to_the_next_transaction(test_db, app_env):
  world = seed(test_db.admin)
  with user_transaction(app_env.db, world.a) as conn:
    assert count(conn, 'activity_events') == 2
  # Same single pooled connection, new transaction, no context.
  with system_transaction(app_env.db) as conn:
    assert count(conn, 'activity_events') == 0
  # ORM calls outside any transaction have no context either.
  assert app_env['activity.event'].search([]) == []


def test_migration_is_idempotent(test_db):
  from secretary_warehouse.schema import migrate, preview

  migrate(test_db.owner_dsn)
  plan = preview(test_db.owner_dsn)
  assert plan.changes == [] and plan.blocked == []


# ---------------------------------------------------------------------------
# sign-in


def test_first_sign_in_provisions_user_and_personal_workspace(test_db, app_env):
  def resolve(subject):
    with system_transaction(app_env.db) as conn:
      return conn.execute(
        "SELECT app_resolve_identity('https://id.test/realms/secretary', %s, 'c@example.test', 'C') AS id",
        [subject],
      ).fetchone()['id']

  user = resolve('sub-c')
  assert resolve('sub-c') == user  # second sign-in: same user, nothing new

  with user_transaction(app_env.db, user):
    me = app_env['user'].search([])
    assert [u.email for u in me] == ['c@example.test']
    workspaces = app_env['workspace'].search([])
    assert [(w.kind, w.personal_owner_id.id) for w in workspaces] == [('personal', user)]
    assert [m.role for m in app_env['membership'].search([])] == ['owner']


def test_the_app_role_cannot_create_users_directly(test_db, app_env):
  with pytest.raises(psycopg.errors.InsufficientPrivilege):
    with system_transaction(app_env.db) as conn:
      conn.execute("INSERT INTO users (email) VALUES ('rogue@example.test')")


# ---------------------------------------------------------------------------
# user-scoped data


def test_users_see_only_their_own_rows(test_db, app_env):
  world = seed(test_db.admin)
  for user in (world.a, world.b):
    with user_transaction(app_env.db, user) as conn:
      assert count(conn, 'users') == 1
      assert count(conn, 'devices') == 1
      assert count(conn, 'activity_events') == 2
      others = conn.execute(
        'SELECT count(*) AS n FROM activity_events WHERE user_id <> %s', [user]
      ).fetchone()['n']
      assert others == 0
      # The ORM sees the same thing.
      assert {e.user_id.id for e in app_env['activity.event'].search([])} == {user}


def test_cannot_write_rows_for_another_user(test_db, app_env):
  world = seed(test_db.admin)
  with pytest.raises(psycopg.errors.InsufficientPrivilege):
    with user_transaction(app_env.db, world.a) as conn:
      insert_event(conn, world.b, world.b_device)
  # Through the ORM as well.
  with pytest.raises(psycopg.errors.InsufficientPrivilege):
    with user_transaction(app_env.db, world.a):
      app_env['exclusion'].create({'user_id': world.b, 'kind': 'app', 'pattern': 'Slack'})


def test_activity_events_are_insert_only(test_db, app_env):
  world = seed(test_db.admin)
  for sql in ("UPDATE activity_events SET event_type = 'x'", 'DELETE FROM activity_events'):
    with pytest.raises(psycopg.errors.InsufficientPrivilege):
      with user_transaction(app_env.db, world.a) as conn:
        conn.execute(sql)


def test_resent_events_are_deduplicated(test_db, app_env):
  world = seed(test_db.admin)
  event_id = uuid.uuid4()
  with user_transaction(app_env.db, world.a) as conn:
    at = "'2026-11-06T10:00:00Z'::timestamptz"
    assert insert_event(conn, world.a, world.a_device, event_id, at) == 1
    assert insert_event(conn, world.a, world.a_device, event_id, at) == 0


# ---------------------------------------------------------------------------
# workspaces, projects, rules


def test_workspace_data_is_visible_to_members_only(test_db, app_env):
  world = seed(test_db.admin)
  with user_transaction(app_env.db, world.a) as conn:
    assert count(conn, 'workspaces') == 2
    assert count(conn, 'projects') == 2
  with user_transaction(app_env.db, world.b) as conn:
    projects = [r['id'] for r in conn.execute('SELECT id FROM projects').fetchall()]
    assert projects == [world.team_project]
    sees = conn.execute(
      'SELECT count(*) AS n FROM workspaces WHERE id = %s', [world.a_personal_ws]
    ).fetchone()['n']
    assert sees == 0
  assert world.a_personal_project != world.team_project


def test_only_admins_manage_shared_projects(test_db, app_env):
  world = seed(test_db.admin)
  with pytest.raises(psycopg.errors.InsufficientPrivilege):
    with user_transaction(app_env.db, world.b):
      app_env['project'].create({'workspace_id': world.team_ws, 'name': 'Rogue'})
  with user_transaction(app_env.db, world.a):
    project = app_env['project'].create({'workspace_id': world.team_ws, 'name': 'Second client'})
    assert project.id


def test_personal_rules_stay_private_shared_rules_are_visible(test_db, app_env):
  world = seed(test_db.admin)
  with user_transaction(app_env.db, world.a) as conn:
    assert count(conn, 'project_mappings') == 2
  with user_transaction(app_env.db, world.b) as conn:
    visible = [r['user_id'] for r in conn.execute('SELECT user_id FROM project_mappings').fetchall()]
    assert visible == [None], 'B sees only the shared rule'


def test_rows_cannot_point_at_another_workspaces_project(test_db):
  world = seed(test_db.admin)
  # Composite FK (project_id, workspace_id): a team rule cannot target A's personal project.
  with pytest.raises(psycopg.errors.ForeignKeyViolation):
    test_db.admin.execute(
      "INSERT INTO project_mappings (workspace_id, project_id, signal, pattern) VALUES (%s, %s, 'path', '/x')",
      [world.team_ws, world.a_personal_project],
    )


def test_database_enforces_selections_and_checks(test_db):
  world = seed(test_db.admin)
  with pytest.raises(psycopg.errors.CheckViolation):
    test_db.admin.execute(
      "INSERT INTO memberships (workspace_id, user_id, role) VALUES (%s, %s, 'superuser')",
      [world.team_ws, world.b],
    )
  with pytest.raises(psycopg.errors.CheckViolation):
    test_db.admin.execute("INSERT INTO exclusions (user_id, kind) VALUES (%s, 'time_window')", [world.a])


def test_deleting_a_user_cascades_to_their_data(test_db):
  world = seed(test_db.admin)
  test_db.admin.execute('DELETE FROM users WHERE id = %s', [world.b])
  for table in ('activity_events', 'devices', 'submissions', 'memberships', 'user_identities'):
    left = test_db.admin.execute(f'SELECT count(*) AS n FROM "{table}" WHERE user_id = %s', [world.b]).fetchone()['n']
    assert left == 0, table


# ---------------------------------------------------------------------------
# private until submitted


def test_drafts_are_private_submitted_totals_reach_admins(test_db, app_env):
  world = seed(test_db.admin)
  with user_transaction(app_env.db, world.b) as conn:
    assert count(conn, 'submissions') == 2
  with user_transaction(app_env.db, world.a) as conn:
    statuses = [r['status'] for r in conn.execute('SELECT status FROM submissions').fetchall()]
    assert statuses == ['submitted']
    # ...and still nothing of B's raw activity or devices.
    b_events = conn.execute(
      'SELECT count(*) AS n FROM activity_events WHERE user_id = %s', [world.b]
    ).fetchone()['n']
    assert b_events == 0


def test_members_cannot_approve_their_own_submissions(test_db, app_env):
  world = seed(test_db.admin)
  with pytest.raises(psycopg.errors.InsufficientPrivilege):
    with user_transaction(app_env.db, world.b) as conn:
      conn.execute("UPDATE submissions SET status = 'approved' WHERE status = 'submitted'")
  with user_transaction(app_env.db, world.a) as conn:
    approved = conn.execute(
      "UPDATE submissions SET status = 'approved' WHERE status = 'submitted'"
    ).rowcount
    assert approved == 1, 'team owner approves'


# ---------------------------------------------------------------------------
# partitions and jobs


def test_event_partitions_cover_retention_and_three_months_ahead(test_db, app_env):
  missing = test_db.admin.execute(
    """SELECT to_char(m, 'YYYY_MM') AS m FROM generate_series(
           date_trunc('month', now() AT TIME ZONE 'UTC') - interval '12 months',
           date_trunc('month', now() AT TIME ZONE 'UTC') + interval '3 months',
           interval '1 month') AS m
       WHERE to_regclass('activity_events_' || to_char(m, 'YYYY_MM')) IS NULL"""
  ).fetchall()
  assert missing == []
  # Maintenance is idempotent and callable by the app role.
  with system_transaction(app_env.db) as conn:
    created = conn.execute('SELECT app_ensure_event_partitions(12, 3) AS n').fetchone()['n']
    assert created == 0


def test_one_queued_job_per_user_day(test_db, app_env):
  world = seed(test_db.admin)
  with system_transaction(app_env.db) as conn:
    for _ in range(3):
      conn.execute(
        """INSERT INTO processing_jobs (user_id, local_day, reason, run_after)
           VALUES (%s, '2026-11-06', 'ingest', now() + interval '2 minutes')
           ON CONFLICT (user_id, local_day) WHERE status = 'queued'
           DO UPDATE SET run_after = EXCLUDED.run_after, updated_at = now()""",
        [world.a],
      )
    assert count(conn, 'processing_jobs') == 1
