"""Projects and the configuration that maps activity to them."""

from __future__ import annotations

from secretary_orm import fields, models

MAPPING_SIGNALS = [
  ('path', 'Path'), ('remote', 'Remote'), ('domain', 'Domain'), ('title', 'Title'), ('app', 'App'),
]

# Pins a row to the project's own workspace (composite target: projects_id_workspace_key).
_PROJECT_IN_WORKSPACE = (
  'project_workspace_fk',
  'FOREIGN KEY (project_id, workspace_id) REFERENCES projects (id, workspace_id) ON DELETE CASCADE',
)


class Project(models.Model):
  _name = 'project'
  _table = 'projects'
  _sql_constraints = [
    ('id_workspace_key', 'UNIQUE (id, workspace_id)'),
    ('budget_positive', 'CHECK (budget_minutes IS NULL OR budget_minutes > 0)'),
  ]

  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True, ondelete='cascade')
  name = fields.StringField(name='Name', required=True)
  color = fields.StringField(name='Color', size=16, default=None)
  client_name = fields.StringField(name='Client', default=None)
  billable = fields.BooleanField(name='Billable', default=True)
  waiting_time_billable = fields.BooleanField(name='Waiting Time Billable', default=False)
  hourly_rate = fields.NumericField(name='Hourly Rate', precision=12, scale=2, default=None)
  currency = fields.StringField(name='Currency', size=3, default=None)
  budget_minutes = fields.IntegerField(name='Budget (min)', default=None)
  created_by = fields.Many2oneField('user', name='Created By', ondelete='set null')
  archived_at = fields.DateTimeField(name='Archived At', use_tz=True)


class ProjectLink(models.Model):
  """A member's link from their own repos/paths to a (usually shared) project."""

  _name = 'project.link'
  _table = 'project_links'
  _sql_constraints = [
    _PROJECT_IN_WORKSPACE,
    ('has_target', 'CHECK (repository_remote IS NOT NULL OR local_path IS NOT NULL)'),
  ]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  project_id = fields.Many2oneField('project', name='Project', required=True)
  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True)
  repository_remote = fields.TextField(name='Repository Remote', default=None)  # normalized
  local_path = fields.TextField(name='Local Path', default=None)


class ProjectMapping(models.Model):
  """Deterministic rule signal -> project.

  ``user_id`` set = personal rule (only its owner sees it); NULL = shared team
  rule (members see, admins manage).
  """

  _name = 'project.mapping'
  _table = 'project_mappings'
  _sql_constraints = [_PROJECT_IN_WORKSPACE]

  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True)
  project_id = fields.Many2oneField('project', name='Project', required=True)
  user_id = fields.Many2oneField('user', name='User', ondelete='cascade')
  signal = fields.SelectionField(MAPPING_SIGNALS, name='Signal', required=True)
  pattern = fields.TextField(name='Pattern', required=True)
  priority = fields.IntegerField(name='Priority', default=0, required=True)
  created_by = fields.Many2oneField('user', name='Created By', ondelete='set null')


class Exclusion(models.Model):
  _name = 'exclusion'
  _table = 'exclusions'
  _sql_constraints = [(
    'shape',
    "CHECK ((kind = 'time_window' AND window_start IS NOT NULL AND window_end IS NOT NULL)"
    " OR (kind <> 'time_window' AND pattern IS NOT NULL))",
  )]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  kind = fields.SelectionField(
    [('path', 'Path'), ('app', 'App'), ('domain', 'Domain'), ('time_window', 'Time Window')],
    name='Kind', required=True,
  )
  pattern = fields.TextField(name='Pattern', default=None)  # path / app / domain pattern
  window_start = fields.TimeField(name='Window Start')  # time_window: local time of day
  window_end = fields.TimeField(name='Window End')
  # time_window: ISO weekdays [1 = Monday ... 7 = Sunday]; NULL = every day.
  days_of_week = fields.JsonField(name='Days of Week')


class RuleProposal(models.Model):
  _name = 'rule.proposal'
  _table = 'rule_proposals'
  # Rejected proposals are never repeated.
  _sql_constraints = [('user_signal_pattern_key', 'UNIQUE (user_id, signal, pattern)')]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  project_id = fields.Many2oneField('project', name='Project', required=True, ondelete='cascade')
  signal = fields.SelectionField(MAPPING_SIGNALS, name='Signal', required=True)
  pattern = fields.TextField(name='Pattern', required=True)
  evidence_count = fields.IntegerField(name='Evidence', default=0, required=True)
  status = fields.SelectionField(
    [('pending', 'Pending'), ('accepted', 'Accepted'), ('rejected', 'Rejected')],
    name='Status', default='pending', required=True,
  )
  decided_at = fields.DateTimeField(name='Decided At', use_tz=True)
