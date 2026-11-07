"""secretary-orm: a declarative PostgreSQL ORM for a single database.

Extracted from the J-ERP framework, without its multi-database manager,
module system, HTTP layer and UI.
"""

from secretary_orm import fields, models
from secretary_orm.api import api_expose, depends_on, on_change
from secretary_orm.config import Config
from secretary_orm.db import Database
from secretary_orm.env import Environment
from secretary_orm.models import Model, RecordSet
from secretary_orm.schema import SchemaBlocked, SchemaPlan

__all__ = [
  'Config',
  'Database',
  'Environment',
  'Model',
  'RecordSet',
  'SchemaBlocked',
  'SchemaPlan',
  'api_expose',
  'depends_on',
  'fields',
  'models',
  'on_change',
]
