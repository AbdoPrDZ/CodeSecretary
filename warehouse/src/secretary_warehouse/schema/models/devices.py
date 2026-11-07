"""Registered agents (D14: one device per Keycloak device-login session)."""

from __future__ import annotations

from secretary_orm import fields, models


class Device(models.Model):
  _name = 'device'
  _table = 'devices'

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  name = fields.StringField(name='Name', required=True)
  os = fields.StringField(name='OS', size=32, required=True)
  agent_version = fields.StringField(name='Agent Version', size=64, default=None)
  # Keycloak session id (``sid``) of the device login; maps tokens to this device.
  auth_session_id = fields.StringField(name='Auth Session', required=True, unique=True)
  clock_offset_ms = fields.IntegerField(name='Clock Offset (ms)', default=0, required=True)
  queue_depth = fields.IntegerField(name='Queue Depth', default=None)
  last_seen_at = fields.DateTimeField(name='Last Seen', use_tz=True)
  revoked_at = fields.DateTimeField(name='Revoked At', use_tz=True)
