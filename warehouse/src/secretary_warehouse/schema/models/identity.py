"""Identity and tenancy: users, identities, workspaces, memberships, invitations.

Users are identified by their Keycloak account (D14): ``user_identities``
maps ``(issuer, sub)`` to a user. Workspace roles live here, never in Keycloak.
"""

from __future__ import annotations

from secretary_orm import fields, models

WORKSPACE_ROLES = [('owner', 'Owner'), ('admin', 'Admin'), ('member', 'Member'), ('viewer', 'Viewer')]


class User(models.Model):
  _name = 'user'
  _table = 'users'
  _description = 'A person. Created on first sign-in (app_resolve_identity).'
  _label = 'email'

  email = fields.StringField(name='Email', size=320, required=True)
  display_name = fields.StringField(name='Display Name', default=None)
  # IANA name; defines the user's "local day".
  time_zone = fields.StringField(name='Time Zone', size=64, default='UTC', required=True)
  deleted_at = fields.DateTimeField(name='Deleted At', use_tz=True)


class UserIdentity(models.Model):
  _name = 'user.identity'
  _table = 'user_identities'
  _description = "Keycloak account (issuer + subject) of a user."
  _sql_constraints = [('provider_subject_key', 'UNIQUE (provider, subject)')]

  provider = fields.StringField(name='Issuer', size=512, required=True)
  subject = fields.StringField(name='Subject', required=True)
  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')


class Workspace(models.Model):
  _name = 'workspace'
  _table = 'workspaces'
  _description = 'Tenant boundary: one personal workspace per user; team workspaces (Phase 3).'
  _sql_constraints = [
    ('personal_owner_key', 'UNIQUE (personal_owner_id)'),
    ('personal_owner_check', "CHECK ((kind = 'personal') = (personal_owner_id IS NOT NULL))"),
  ]

  kind = fields.SelectionField([('personal', 'Personal'), ('team', 'Team')], name='Kind', required=True)
  name = fields.StringField(name='Name', required=True)
  # Set only for personal workspaces: exactly one per user.
  personal_owner_id = fields.Many2oneField('user', name='Personal Owner', ondelete='cascade')
  # Team policy: may ordinary members see each other's submitted totals?
  members_see_submissions = fields.BooleanField(name='Members See Submissions', default=False)


class Membership(models.Model):
  _name = 'membership'
  _table = 'memberships'
  _sql_constraints = [('workspace_user_key', 'UNIQUE (workspace_id, user_id)')]

  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True, ondelete='cascade')
  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  role = fields.SelectionField(WORKSPACE_ROLES, name='Role', required=True)


class Invitation(models.Model):
  _name = 'invitation'
  _table = 'invitations'
  _sql_constraints = [('role_not_owner', "CHECK (role <> 'owner')")]

  workspace_id = fields.Many2oneField('workspace', name='Workspace', required=True, ondelete='cascade')
  email = fields.StringField(name='Email', size=320, required=True)
  role = fields.SelectionField(WORKSPACE_ROLES, name='Role', required=True)
  # Hex SHA-256 of the invite token; the token itself is never stored.
  token_hash = fields.StringField(name='Token Hash', size=64, required=True, unique=True)
  invited_by = fields.Many2oneField('user', name='Invited By', ondelete='set null')
  expires_at = fields.DateTimeField(name='Expires At', use_tz=True, required=True)
  accepted_at = fields.DateTimeField(name='Accepted At', use_tz=True)
