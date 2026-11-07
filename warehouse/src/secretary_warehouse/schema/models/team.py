"""Team features (Phase 3 use): submissions and the audit log.

Submissions are the ONLY per-user time data another user can ever see
("private until submitted"). Drafts are visible to their owner only.
"""

from __future__ import annotations

from secretary_orm import fields, models


class Submission(models.Model):
  _name = 'submission'
  _table = 'submissions'
  _sql_constraints = [
    ('project_workspace_fk',
     'FOREIGN KEY (project_id, workspace_id) REFERENCES projects (id, workspace_id) ON DELETE CASCADE'),
    ('period', 'CHECK (period_end >= period_start)'),
    ('total_positive', 'CHECK (total_seconds >= 0)'),
    ('waiting_positive', 'CHECK (waiting_seconds >= 0)'),
  ]

  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True)
  project_id = fields.Many2oneField('project', name='Project', required=True)
  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  period_start = fields.DateField(name='Period Start', required=True)
  period_end = fields.DateField(name='Period End', required=True)
  total_seconds = fields.IntegerField(name='Total (s)', required=True, default=None)
  waiting_seconds = fields.IntegerField(name='Waiting (s)', default=0, required=True)
  summary = fields.TextField(name='Summary', default=None)  # optional, user-approved
  status = fields.SelectionField(
    [('draft', 'Draft'), ('submitted', 'Submitted'), ('approved', 'Approved'), ('rejected', 'Rejected')],
    name='Status', default='draft', required=True,
  )
  revision = fields.IntegerField(name='Revision', default=1, required=True)
  supersedes = fields.Many2oneField('submission', name='Supersedes', ondelete='set null')
  submitted_at = fields.DateTimeField(name='Submitted At', use_tz=True)
  decided_at = fields.DateTimeField(name='Decided At', use_tz=True)
  decided_by = fields.Many2oneField('user', name='Decided By', ondelete='set null')


class AuditLog(models.Model):
  """Admin and security actions. Append-only.

  Members may insert but only admins may read, so insert rows with plain SQL:
  the ORM's ``create`` uses ``RETURNING``, which needs read access.
  """

  _name = 'audit.log'
  _table = 'audit_log'
  _track_refs = False

  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True, ondelete='cascade')
  actor_user_id = fields.Many2oneField('user', name='Actor', ondelete='set null')
  action = fields.StringField(name='Action', size=64, required=True)  # e.g. 'member.removed'
  target_type = fields.StringField(name='Target Type', size=64, default=None)
  target_id = fields.BigIntField(name='Target', default=None)
  details = fields.JsonField(name='Details', required=True, default={})  # never raw activity data
