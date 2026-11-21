"""Dashboard sign-in sessions (OV-2): server-side, one row per browser sign-in.

The cookie holds a random token; only its SHA-256 is stored. Lookup before a
user context exists goes through ``app_web_session_user()`` (SECURITY
DEFINER); everything else is RLS-scoped to the session's user.
"""

from __future__ import annotations

from secretary_orm import fields, models


class WebSession(models.Model):
  _name = 'web.session'
  _table = 'web_sessions'

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  token_hash = fields.StringField(name='Token Hash', size=64, required=True, unique=True)
  # Double-submit CSRF token, returned by /api/me and required on mutations.
  csrf_token = fields.StringField(name='CSRF Token', size=64, required=True)
  # Keycloak session (``sid``) and ID token, for RP-initiated logout.
  auth_session_id = fields.StringField(name='Auth Session', default=None)
  id_token = fields.TextField(name='ID Token', default=None)
  expires_at = fields.DateTimeField(name='Expires At', use_tz=True, required=True)
  last_seen_at = fields.DateTimeField(name='Last Seen', use_tz=True)
