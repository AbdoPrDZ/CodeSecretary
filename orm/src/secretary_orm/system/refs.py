"""``sys.data``: external ids (``create(_key=...)``, ``Model.register_ref``, ``Environment.ref``).

Every ``create`` on a model with ``_track_refs`` (the default) then also writes a ``sys.data`` row."""

from __future__ import annotations

from secretary_orm import fields, models


class SysData(models.Model):
  """External identifiers for records (similar to Odoo XML IDs)."""

  _name = 'sys.data'
  _description = 'External identifiers for records.'
  _label = 'key'
  _track_refs = False

  module = fields.StringField(
    name='Module',
    required=True,
    help='Owner (package) of this external identifier.',
  )
  model = fields.StringField(
    name='Model',
    required=True,
    help='Model name of the referenced record.',
  )
  name = fields.StringField(
    name='Name',
    required=True,
    help='Short key name, without the owner prefix.',
  )
  key = fields.StringField(
    name='Key',
    required=True,
    unique=True,
    help='Full unique external identifier used by env.ref().',
  )
  res_id = fields.BigIntField(
    name='Record ID',
    required=True,
    help='ID of the referenced record.',
  )
