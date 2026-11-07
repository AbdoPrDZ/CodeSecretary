"""User corrections: engine inputs reapplied on every recalculation, never edits to derived rows."""

from __future__ import annotations

from secretary_orm import fields, models


class AssignmentOverride(models.Model):
  """Manual project choice for one event (assignment method 'user', confidence 1.0)."""

  _name = 'assignment.override'
  _table = 'assignment_overrides'
  _sql_constraints = [('user_event_key', 'UNIQUE (user_id, event_id)')]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  event_id = fields.UuidField(name='Event ID', required=True)
  local_day = fields.DateField(name='Local Day', required=True)
  # NULL = "not project work".
  project_id = fields.Many2oneField('project', name='Project', ondelete='cascade')


class TimeAdjustment(models.Model):
  """Time-range correction anchored to time and project, so it survives recalculation."""

  _name = 'time.adjustment'
  _table = 'time_adjustments'
  _sql_constraints = [
    ('positive', 'CHECK (ends_at > starts_at)'),
    ('target', "CHECK (kind = 'remove' OR project_id IS NOT NULL)"),
  ]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  local_day = fields.DateField(name='Local Day', required=True)
  kind = fields.SelectionField(
    [('add', 'Add'), ('remove', 'Remove'), ('reassign', 'Reassign')], name='Kind', required=True,
  )
  starts_at = fields.DateTimeField(name='Starts', use_tz=True, required=True)
  ends_at = fields.DateTimeField(name='Ends', use_tz=True, required=True)
  project_id = fields.Many2oneField('project', name='Project', ondelete='cascade')  # add / reassign target
  # Optional filter for remove / reassign.
  from_project_id = fields.Many2oneField('project', name='From Project', ondelete='cascade')
  note = fields.TextField(name='Note', default=None)
  deleted_at = fields.DateTimeField(name='Deleted At', use_tz=True)  # undo keeps history
