"""Raw activity events: immutable, insert-only, partitioned by month.

The table itself is created by ``sql/pre.sql`` as a partitioned table (the ORM
cannot create partitions); the ORM then adds and checks every column below.

Partition key is the device-reported ``timestamp_start``: Postgres requires
unique keys on partitioned tables to include the partition key, and
``timestamp_start`` never changes for a given event, so
``(event_id, timestamp_start)`` deduplicates resends.
"""

from __future__ import annotations

from secretary_orm import fields, models


class ActivityEvent(models.Model):
  _name = 'activity.event'
  _table = 'activity_events'
  _track_refs = False
  _sql_constraints = [
    ('event_key', 'UNIQUE (event_id, timestamp_start)'),
    ('end_after_start', 'CHECK (timestamp_end IS NULL OR timestamp_end >= timestamp_start)'),
    ('adjusted_end_after_start',
     'CHECK (server_adjusted_end IS NULL OR server_adjusted_end >= server_adjusted_start)'),
  ]

  event_id = fields.UuidField(name='Event ID', required=True)  # UUIDv7 from the collector
  # From the access token, never the payload.
  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  device_id = fields.Many2oneField('device', name='Device', required=True, ondelete='cascade')
  batch_id = fields.UuidField(name='Batch', required=True)  # key of the raw archive object
  schema_version = fields.SmallIntField(name='Schema Version', required=True, default=None)
  event_type = fields.StringField(name='Type', size=64, required=True)
  source = fields.StringField(name='Source', size=128, required=True)  # e.g. 'vscode@0.3.1'
  timestamp_start = fields.DateTimeField(name='Start', use_tz=True, required=True)
  timestamp_end = fields.DateTimeField(name='End', use_tz=True)
  server_adjusted_start = fields.DateTimeField(name='Adjusted Start', use_tz=True, required=True)
  server_adjusted_end = fields.DateTimeField(name='Adjusted End', use_tz=True)
  project_hint = fields.StringField(name='Project Hint', default=None)
  repository_root = fields.TextField(name='Repository Root', default=None)
  repository_remote = fields.TextField(name='Repository Remote', default=None)  # normalized
  working_directory = fields.TextField(name='Working Directory', default=None)
  # Already redacted by the agent.
  metadata = fields.JsonField(name='Metadata', required=True, default={})
