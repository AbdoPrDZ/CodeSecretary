"""Privacy export and delete of a user's collected data (WH-12, WH-13).

- :func:`export_events` streams all of the user's raw events as NDJSON, read
  through RLS as that user.
- :func:`delete` calls ``app_privacy_delete`` (``post.sql``), the only path
  that removes events; the user comes from the transaction's tenant context.

Neither function logs event metadata.
"""

from __future__ import annotations

import json
from collections.abc import Iterator
from datetime import date, datetime
from uuid import UUID

from secretary_orm import Database

from secretary_warehouse.db import set_tenant

SCOPES = ('day', 'device', 'all')
#: Rows fetched from the server-side cursor per round trip.
EXPORT_BATCH = 2000

_EXPORT_SQL = """
  SELECT e.event_id, e.event_type, e.source, e.schema_version, d.id AS device_id, d.name AS device_name,
         e.timestamp_start, e.timestamp_end, e.server_adjusted_start, e.server_adjusted_end,
         e.project_hint, e.repository_root, e.repository_remote, e.working_directory, e.metadata,
         e.batch_id, e.created_at
  FROM activity_events e JOIN devices d ON d.id = e.device_id
  ORDER BY e.server_adjusted_start, e.event_id
"""


class PrivacyError(Exception):
  def __init__(self, code: str):
    super().__init__(code)
    self.code = code


def export_events(db: Database, user_id: int) -> Iterator[str]:
  """All of the user's raw events, one JSON object per line, oldest first.

  Holds one pooled connection while the caller iterates (a streaming
  response). It uses its own connection and transaction rather than
  :func:`~secretary_warehouse.db.user_transaction`, because a streaming
  response may resume the generator on another thread.
  """

  with db.connection() as conn, conn.transaction():
    set_tenant(conn, user_id)
    with conn.cursor(name='privacy_export') as cur:
      cur.itersize = EXPORT_BATCH
      cur.execute(_EXPORT_SQL)
      for row in cur:
        yield json.dumps(dict(row), default=_json, ensure_ascii=False, separators=(',', ':')) + '\n'


def delete(conn, scope: str, day: date | None = None, device_id: int | None = None) -> dict:
  """Delete the tenant's events of one local day, one device, or all, with
  what was derived from them; the affected days are queued for the engine.

  ``conn`` must be inside :func:`~secretary_warehouse.db.user_transaction`.
  """

  if scope not in SCOPES:
    raise PrivacyError('invalid_scope')
  if scope == 'day' and day is None:
    raise PrivacyError('invalid_day')
  if scope == 'device' and device_id is None:
    raise PrivacyError('invalid_device')
  if scope == 'device':
    # The function raises on a foreign or unknown device; check first for a clean 404.
    if conn.execute('SELECT 1 FROM devices WHERE id = %s', [device_id]).fetchone() is None:
      raise PrivacyError('device_not_found')
  row = conn.execute('SELECT * FROM app_privacy_delete(%s, %s, %s)', [scope, day, device_id]).fetchone()
  return {'deleted_events': int(row['deleted_events']), 'queued_days': int(row['queued_days'])}


def _json(value):
  if isinstance(value, datetime):
    return value.isoformat()
  if isinstance(value, (date, UUID)):
    return str(value)
  raise TypeError(type(value).__name__)
