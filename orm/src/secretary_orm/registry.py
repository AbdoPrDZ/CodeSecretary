"""Model registry for the single database.

Every model class registers itself as a *source* in
:class:`secretary_orm.models.ModelRegistry` when it is defined. The registry
combines those sources (base models, ``_inherit`` extensions and ``_inherits``
models) into one built class per model name with
:class:`secretary_orm.builder.ModelBuilder`, and keeps the database schema in
sync with them through :class:`secretary_orm.schema.SchemaSyncer`.

J-ERP built one registry per database from its installed modules; here there
is one database and every imported model is part of it.
"""

from __future__ import annotations

import logging
import threading

from secretary_orm.builder import BuildResult, ModelBuilder
from secretary_orm.constraints import sync_constraints
from secretary_orm.models import ModelRegistry
from secretary_orm.refs import backfill
from secretary_orm.schema import SchemaBlocked, SchemaSyncer

logger = logging.getLogger('secretary_orm.registry')

#: Builder group holding every model source. The builder validates
#: cross-group dependencies; with a single group there are none to declare.
SOURCE_GROUP = 'app'


class Registry:
  def __init__(self, db, *, auto_sync: bool = True):
    self.db = db
    self.auto_sync = auto_sync
    self.builder = ModelBuilder()
    #: Source groups (kept for ``Model.register_ref`` key resolution).
    self.modules: dict = {SOURCE_GROUP: None}
    self.models: dict = {}
    self._result: BuildResult | None = None
    self._lock = threading.RLock()
    self._schema_synced = False
    #: Last plan produced by a schema sync (see ``secretary_orm.schema.SchemaPlan``).
    self.last_schema_plan = None
    #: Last constraint changes (see ``secretary_orm.constraints.ConstraintPlan``).
    self.last_constraint_plan = None
    self.ready = False

  # --- loading ------------------------------------------------------------

  def load(self) -> 'Registry':
    """(Re)build the models from every registered source."""

    with self._lock:
      sources = list(ModelRegistry.all_classes())
      self._result = self.builder.build({SOURCE_GROUP: sources}, {})
      self.models = self._result.models
      if self._result.changed:
        self._schema_synced = False
      self.ready = True
      return self

  def ensure_loaded(self) -> 'Registry':
    with self._lock:
      if not self.ready:
        self.load()
      if self.auto_sync and not self._schema_synced:
        self.sync_dirty()
      return self

  @property
  def build_result(self) -> BuildResult | None:
    return self._result

  def rebuild(self, force: bool = False) -> 'Registry':
    """Rebuild the models (e.g. after importing new ones) and sync the schema."""

    with self._lock:
      self.load()
      if force:
        self.init_models()
      else:
        self.sync_dirty()
      return self

  # --- schema -------------------------------------------------------------

  def _stored_models(self, models):
    return [model for model in models if not getattr(model, '_virtual', False)]

  def init_models(self, allow_drop: bool = False, allow_type_change: bool = True):
    """Synchronize the schema of every built model in one transaction.

    Columns and indexes are synced first, then constraints (foreign keys,
    selection checks, ``_sql_constraints``). Raises :class:`SchemaBlocked` when
    a declared change cannot be applied safely (the transaction is rolled back).
    """

    with self._lock:
      if not self.ready:
        self.load()
      with self.db.transaction():
        self.last_schema_plan = SchemaSyncer(self.db).sync_models(
          self._stored_models(self.models.values()),
          allow_drop=allow_drop,
          allow_type_change=allow_type_change,
        )
        self.last_constraint_plan = sync_constraints(self.db, self.models)
      self._schema_synced = True
      return self.last_schema_plan

  def preview(self, allow_drop: bool = False):
    """Return the plan for a full sync, without changing the database."""

    with self._lock:
      self.load()
      return SchemaSyncer(self.db).plan(
        self._stored_models(self.models.values()), allow_drop=allow_drop
      )

  def retired_fields(self) -> list[dict]:
    """Retired columns still present in the database (read-only)."""

    with self._lock:
      self.load()
      return SchemaSyncer(self.db).retired_fields(self.models.values())

  def sync_dirty(self) -> set[str]:
    """Synchronize only models whose build fingerprint changed.

    Safe changes are applied; blocked ones (type changes, drops) are logged and
    kept in ``last_schema_plan`` instead of raising, so a first model access
    never fails because of a pending destructive change. Use
    :meth:`init_models` to apply or surface those explicitly.
    """

    with self._lock:
      if self._result is None:
        return set()

      names = set(self._result.changed)
      models = self._stored_models(
        self.models[name] for name in names if name in self.models
      )
      if models:
        with self.db.transaction():
          # allow_unsafe: commit the safe part and report the blocked part
          # instead of raising (blocked changes were never executed).
          plan = SchemaSyncer(self.db).sync_models(
            models, allow_type_change=False, allow_unsafe=True
          )
          self.last_schema_plan = plan
          self.last_constraint_plan = sync_constraints(
            self.db, self.models, only={model._name for model in models}
          )
        if plan.blocked:
          logger.warning('%s', SchemaBlocked(plan))
      self._schema_synced = True
      return names

  # --- external references ------------------------------------------------

  def backfill_refs(self) -> int:
    """Create a ``sys.data`` external id for every record that has none."""

    with self._lock:
      self.ensure_loaded()
      return backfill(self)

  # --- accessors ----------------------------------------------------------

  def get(self, model_name: str):
    model = self.models.get(model_name)
    if model is None and '_' in model_name:
      # Accept ``sys_field`` as an alias for ``sys.field``.
      model = self.models.get(model_name.replace('_', '.'))
    return model

  def describe(self, model_name: str) -> dict | None:
    self.ensure_loaded()
    return self.builder.describe(model_name)

  def list_models(self) -> list[str]:
    return list(self.models)
