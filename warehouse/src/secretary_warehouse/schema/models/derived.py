"""Derived data, written only by the engine and replaced per (user, local day).

No foreign keys to ``activity_events``: a FK into a partitioned table must
reference its full key. Derived rows are rebuilt from events on every
recalculation, and privacy delete removes them explicitly.
"""

from __future__ import annotations

from secretary_orm import fields, models


class EventAssignment(models.Model):
  _name = 'event.assignment'
  _table = 'event_assignments'
  _track_refs = False
  _sql_constraints = [
    ('user_event_key', 'UNIQUE (user_id, event_id)'),
    ('confidence_range', 'CHECK (confidence >= 0 AND confidence <= 1)'),
  ]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  event_id = fields.UuidField(name='Event ID', required=True)
  local_day = fields.DateField(name='Local Day', required=True)
  # NULL = unassigned (inbox).
  project_id = fields.Many2oneField('project', name='Project', ondelete='cascade')
  confidence = fields.FloatField(name='Confidence', required=True)
  method = fields.SelectionField(
    [('user', 'User'), ('rule', 'Rule'), ('shared_rule', 'Shared Rule'),
     ('structural', 'Structural'), ('temporal', 'Temporal'), ('ai', 'AI')],
    name='Method', required=True,
  )
  rule_id = fields.BigIntField(name='Rule', default=None)  # project_mappings.id
  model_id = fields.StringField(name='AI Model', size=128, default=None)
  algorithm_version = fields.IntegerField(name='Algorithm Version', required=True, default=None)


class WorkSession(models.Model):
  _name = 'work.session'
  _table = 'work_sessions'
  _track_refs = False
  _sql_constraints = [('positive', 'CHECK (ends_at > starts_at)')]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  local_day = fields.DateField(name='Local Day', required=True)
  project_id = fields.Many2oneField('project', name='Project', ondelete='set null')  # NULL = unassigned
  kind = fields.SelectionField([('active', 'Active'), ('waiting', 'Waiting')], name='Kind', required=True)
  starts_at = fields.DateTimeField(name='Starts', use_tz=True, required=True)
  ends_at = fields.DateTimeField(name='Ends', use_tz=True, required=True)
  low_confidence = fields.BooleanField(name='Low Confidence', default=False)  # overlaps a coverage gap
  estimated = fields.BooleanField(name='Estimated', default=False)  # Git backfill
  algorithm_version = fields.IntegerField(name='Algorithm Version', required=True, default=None)
  config_hash = fields.StringField(name='Config Hash', size=64, required=True)


class SessionEvidence(models.Model):
  _name = 'session.evidence'
  _table = 'session_evidence'
  _track_refs = False
  _sql_constraints = [('session_event_key', 'UNIQUE (session_id, event_id)')]

  session_id = fields.Many2oneField('work.session', name='Session', required=True, ondelete='cascade')
  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  event_id = fields.UuidField(name='Event ID', required=True)


class CoverageGap(models.Model):
  _name = 'coverage.gap'
  _table = 'coverage_gaps'
  _track_refs = False
  _sql_constraints = [('positive', 'CHECK (ends_at > starts_at)')]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  device_id = fields.Many2oneField('device', name='Device', required=True, ondelete='cascade')
  collector = fields.StringField(name='Collector', size=64, required=True)
  local_day = fields.DateField(name='Local Day', required=True)
  starts_at = fields.DateTimeField(name='Starts', use_tz=True, required=True)
  ends_at = fields.DateTimeField(name='Ends', use_tz=True, required=True)
