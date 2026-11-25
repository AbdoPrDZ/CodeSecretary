"""Server-side dashboard sessions (OV-2) in ``web_sessions``.

The cookie carries a random token; the table stores only its SHA-256, so a
database leak does not hand out sessions. The token resolves to a user through
``app_web_session_user()`` (SECURITY DEFINER); every other read and write runs
in that user's RLS context.
"""

from __future__ import annotations

import hashlib
import secrets
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta

from secretary_orm import Database

from secretary_warehouse.db import set_tenant, system_transaction, user_transaction

COOKIE = 'sec_session'


@dataclass(frozen=True)
class Session:
  user_id: int
  token_hash: str
  csrf_token: str
  id_token: str | None


def _hash(token: str) -> str:
  return hashlib.sha256(token.encode()).hexdigest()


def resolve_user(db: Database, issuer: str, subject: str, email: str, name: str | None) -> int:
  """The user for a Keycloak identity; provisions user + personal workspace (OV-3)."""

  with system_transaction(db) as conn:
    row = conn.execute('SELECT app_resolve_identity(%s, %s, %s, %s) AS user_id',
                       [issuer, subject, email, name]).fetchone()
  return int(row['user_id'])


def create(db: Database, user_id: int, hours: int, auth_session_id: str | None,
           id_token: str | None) -> str:
  token = secrets.token_urlsafe(32)
  with user_transaction(db, user_id) as conn:
    conn.execute('DELETE FROM web_sessions WHERE user_id = %s AND expires_at <= now()', [user_id])
    conn.execute(
      """INSERT INTO web_sessions (user_id, token_hash, csrf_token, auth_session_id, id_token,
                                   expires_at, last_seen_at)
         VALUES (%s, %s, %s, %s, %s, %s, now())""",
      [user_id, _hash(token), secrets.token_urlsafe(32), auth_session_id, id_token,
       datetime.now(UTC) + timedelta(hours=hours)],
    )
  return token


#: ``last_seen_at`` is refreshed at most this often (not on every request).
TOUCH_INTERVAL = timedelta(minutes=5)


def load(db: Database, token: str | None) -> Session | None:
  """The session for a cookie token: one transaction, and a write only every
  :data:`TOUCH_INTERVAL` (this runs on every API request)."""

  if not token or len(token) > 128:
    return None
  token_hash = _hash(token)
  with system_transaction(db) as conn:
    row = conn.execute('SELECT app_web_session_user(%s) AS user_id', [token_hash]).fetchone()
    if row is None or row['user_id'] is None:
      return None
    user_id = int(row['user_id'])
    set_tenant(conn, user_id)
    session = conn.execute(
      """SELECT csrf_token, id_token, last_seen_at < now() - %s AS stale
         FROM web_sessions WHERE token_hash = %s AND expires_at > now()""",
      [TOUCH_INTERVAL, token_hash],
    ).fetchone()
    if session is None:
      return None
    if session['stale'] is not False:
      conn.execute('UPDATE web_sessions SET last_seen_at = now() WHERE token_hash = %s', [token_hash])
  return Session(user_id=user_id, token_hash=token_hash, csrf_token=session['csrf_token'],
                 id_token=session['id_token'])


def delete(db: Database, session: Session) -> None:
  with user_transaction(db, session.user_id) as conn:
    conn.execute('DELETE FROM web_sessions WHERE token_hash = %s', [session.token_hash])
