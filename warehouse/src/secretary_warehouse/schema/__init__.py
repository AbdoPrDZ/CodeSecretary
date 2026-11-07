"""Schema ownership (D15): the ORM models plus two SQL scripts.

``migrate()`` runs, as the schema owner and in ONE transaction:

1. ``sql/pre.sql`` -- roles and the partitioned ``activity_events`` shell;
2. the ORM sync -- tables, columns, indexes, foreign keys, checks
   (``secretary_orm.Registry.init_models``);
3. ``sql/post.sql`` -- RLS helpers and policies, grants, expression/partial
   indexes, event partitions.

Every step is idempotent, so ``migrate()`` runs on every deployment. A failure
anywhere rolls everything back. Changes the ORM considers unsafe (type changes
that can lose data, column drops) block the migration instead of running.
"""

from __future__ import annotations

import logging
from importlib.resources import files

from secretary_orm import Config, Database
from secretary_orm.registry import Registry

from secretary_warehouse.schema import models  # noqa: F401  (registers every model)

logger = logging.getLogger('secretary_warehouse.schema')

#: Serializes concurrent migrations (several replicas starting at once).
MIGRATION_LOCK = 0x5EC7E7A1


def _sql(name: str) -> str:
  return files('secretary_warehouse.schema').joinpath('sql', name).read_text(encoding='utf-8')


def migrate(owner_dsn: str, *, allow_drop: bool = False) -> None:
  """Bring the database schema up to date (see module docs)."""

  db = Database(Config(dsn=owner_dsn, pool_min=1, pool_max=1, pool_timeout=30))
  try:
    registry = Registry(db, auto_sync=False)
    with db.transaction() as conn:
      conn.execute('SELECT pg_advisory_xact_lock(%s)', [MIGRATION_LOCK])
      conn.execute(_sql('pre.sql'))
      plan = registry.init_models(allow_drop=allow_drop)
      conn.execute(_sql('post.sql'))
    constraints = registry.last_constraint_plan
    logger.info(
      'schema migrated',
      extra={
        'schema_changes': len(plan.changes),
        'constraints_created': len(constraints.created) if constraints else 0,
        'constraints_replaced': len(constraints.replaced) if constraints else 0,
        'constraints_dropped': len(constraints.dropped) if constraints else 0,
      },
    )
  finally:
    db.close()


def preview(owner_dsn: str, *, allow_drop: bool = False):
  """The ORM part of a migration as a ``SchemaPlan``, without changing anything."""

  db = Database(Config(dsn=owner_dsn, pool_min=1, pool_max=1, pool_timeout=30))
  try:
    return Registry(db, auto_sync=False).preview(allow_drop=allow_drop)
  finally:
    db.close()
