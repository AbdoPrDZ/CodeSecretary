"""ORM environment: the one database and its model registry.

Models reach the database through ``Environment.instance``, so an application
creates exactly one environment at startup::

    from secretary_orm import Config, Environment

    env = Environment(Config.from_env())
    Partner = env['res.partner']
"""

from __future__ import annotations

from secretary_orm.config import Config
from secretary_orm.db import Database
from secretary_orm.registry import Registry


class _EnvironmentMeta(type):
  @property
  def instance(cls) -> 'Environment':
    if cls._instance is None:
      raise RuntimeError('Environment is not initialized. Create Environment(config) first.')
    return cls._instance


class Environment(metaclass=_EnvironmentMeta):
  _instance: 'Environment | None' = None

  def __init__(self, config: Config | None = None, *, database: Database | None = None,
               auto_sync: bool = True):
    """Create the process-wide environment.

    ``auto_sync`` makes the first model access create/alter tables to match the
    declared models (the J-ERP behaviour). Turn it off when the schema is
    managed by migrations and call :meth:`Registry.init_models` explicitly.
    """

    if database is None:
      database = Database(config or Config.from_env())
    self.db = database
    self._registry = Registry(database, auto_sync=auto_sync)
    type(self)._instance = self

  @classmethod
  def reset(cls) -> None:
    """Close the current environment (mainly for tests)."""

    if cls._instance is not None:
      cls._instance.db.close()
    cls._instance = None

  def close(self) -> None:
    self.db.close()
    if type(self)._instance is self:
      type(self)._instance = None

  # --- models -------------------------------------------------------------

  def registry(self) -> Registry:
    return self._registry

  def __getitem__(self, model_name: str):
    registry = self._registry.ensure_loaded()
    model = registry.get(model_name)
    if model is None:
      raise KeyError(f'Unknown model "{model_name}".')
    return model

  def __contains__(self, model_name: str) -> bool:
    return self._registry.ensure_loaded().get(model_name) is not None

  def ref(self, key: str):
    """Return the record referenced by an external key (needs ``secretary_orm.system``)."""

    registry = self._registry.ensure_loaded()

    Data = registry.get('sys.data')
    if Data is None:
      raise RuntimeError("The 'sys.data' model is not registered; import secretary_orm.system.")

    rows = Data.search([('key', '=', str(key))], limit=1)
    if not rows:
      raise ValueError(f'External ID not found: {key}')

    data = rows[0]
    Model = registry.get(data.model)
    if Model is None:
      raise ValueError(f"External ID '{key}' points to an unknown model '{data.model}'.")

    records = Model.browse(data.res_id)
    if not records:
      raise ValueError(f"External ID '{key}' points to a missing {data.model}#{data.res_id}.")
    return records[0]
