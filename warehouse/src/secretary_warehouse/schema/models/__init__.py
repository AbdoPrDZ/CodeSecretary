"""ORM models for every CodeSecretary table (the warehouse owns the schema).

Importing this package registers the models. Other Python services (the
dashboards) import it too, so every service shares one definition.
"""

from secretary_orm.models import ModelRegistry
from secretary_orm.system.schema import SysField, SysModel

from secretary_warehouse.schema.models import (  # noqa: F401
  ai,
  corrections,
  derived,
  devices,
  events,
  identity,
  jobs,
  projects,
  team,
  web,
)

# sys.model / sys.field: the ORM's rename detection and retired-column
# tracking. Registered explicitly (idempotent) rather than by import side effect.
for _model in (SysModel, SysField):
  ModelRegistry.register(_model)
