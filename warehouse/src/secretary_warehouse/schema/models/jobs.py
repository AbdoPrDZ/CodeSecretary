"""Engine job queue (Postgres SKIP LOCKED; no extra infrastructure).

System table, deliberately WITHOUT RLS: the engine claims jobs across all users
before it knows whose day it is processing. It holds only
(user_id, local_day, reason), never activity data, and is not exposed by any
user-facing endpoint. After claiming a job the engine sets app.user_id to the
job's user for all data access.

At most one queued job per (user, day): enqueue is an upsert on the partial
unique index ``processing_jobs_queued_ux`` (``sql/post.sql``).
"""

from __future__ import annotations

from secretary_orm import fields, models


class ProcessingJob(models.Model):
  _name = 'processing.job'
  _table = 'processing_jobs'
  _track_refs = False

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  local_day = fields.DateField(name='Local Day', required=True)
  # 'ingest', 'rules_changed', 'correction', 'algorithm_upgrade', ...
  reason = fields.StringField(name='Reason', size=64, required=True)
  status = fields.SelectionField(
    [('queued', 'Queued'), ('running', 'Running'), ('done', 'Done'), ('failed', 'Failed')],
    name='Status', default='queued', required=True,
  )
  # Debounce: ingest pushes this ~2 min out.
  run_after = fields.DateTimeField(name='Run After', use_tz=True, required=True, auto_now_add=True)
  attempts = fields.IntegerField(name='Attempts', default=0, required=True)
  locked_at = fields.DateTimeField(name='Locked At', use_tz=True)
  locked_by = fields.StringField(name='Locked By', size=128, default=None)
  last_error = fields.StringField(name='Last Error', size=128, default=None)  # error kind only
