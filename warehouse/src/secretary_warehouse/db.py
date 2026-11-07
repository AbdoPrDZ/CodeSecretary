"""Database access with per-transaction tenant context (row-level security).

All tenant data is protected by Postgres RLS policies that compare rows
against ``app.user_id``. This module is the only place that sets it, and only
with ``SET LOCAL`` semantics (``set_config(..., true)``): the value disappears
when the transaction ends, so a pooled connection can never carry one user's
context into another request.

- :func:`user_transaction` -- the normal path: every ORM call and SQL statement
  inside the block is limited to that user's rows.
- :func:`system_transaction` -- no tenant. Tenant tables return nothing; use it
  only for system tables (``processing_jobs``) and SECURITY DEFINER entry points
  (e.g. identity lookup) that run before the user is known.

ORM calls made *outside* either block run without context and see nothing.
Never call ``set_config('app.user_id', ..., false)`` (session level) anywhere.
"""

from __future__ import annotations

from contextlib import contextmanager

from secretary_orm import Database


@contextmanager
def user_transaction(db: Database, user_id: int):
  """A transaction scoped to ``user_id``; ORM calls inside share it."""

  with db.transaction() as conn:
    conn.execute("SELECT set_config('app.user_id', %s, true)", [str(int(user_id))])
    yield conn


@contextmanager
def system_transaction(db: Database):
  """A transaction with no tenant context (see module docs)."""

  with db.transaction() as conn:
    yield conn


def ping(db: Database) -> None:
  """Readiness check: raises when the database is unreachable."""

  db.fetchone('SELECT 1 AS ok')
