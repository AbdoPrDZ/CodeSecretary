"""``sys.model`` / ``sys.field``: rename detection and retired-column tracking for the schema syncer."""

from __future__ import annotations

from secretary_orm import fields, models


class SysModel(models.Model):
  """Models that have been synchronized into this database."""

  _name = 'sys.model'
  _description = 'Models synchronized into the database.'

  name = fields.StringField(
    name='Model',
    required=True,
    unique=True,
    help='Logical model name.',
  )
  table_name = fields.StringField(
    name='Table',
    help='Physical database table for this model.',
  )


class SysField(models.Model):
  """Field to column mapping, used for deterministic rename detection."""

  _name = 'sys.field'
  _description = 'Field-to-column mappings used by schema synchronization.'

  model = fields.StringField(
    name='Model',
    required=True,
    help='Model that owns this field.',
  )
  field_name = fields.StringField(
    name='Field',
    required=True,
    help='Python field name on the model.',
  )
  column_name = fields.StringField(
    name='Column',
    required=True,
    help='Physical database column name.',
  )
  ttype = fields.StringField(
    name='Type',
    help='Framework field type.',
  )
  label = fields.StringField(
    name='Label',
    help='Human-readable field label.',
  )
  state = fields.SelectionField(
    selection=[('active', 'Active'), ('retired', 'Retired')],
    name='Status',
    default='active',
    required=True,
    help='Retired fields are no longer declared but their column still exists.',
  )
