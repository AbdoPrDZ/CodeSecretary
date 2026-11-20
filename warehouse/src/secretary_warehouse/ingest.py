"""Batch ingest (WH-5): validate, apply clock offset, insert idempotently.

The wire format is defined in ``shared/`` (``secretary_shared::api::BatchRequest``
and ``event::Event``). Until SH-9 generates a JSON Schema from it, this module
mirrors it by hand: change both together.

Per batch:

1. Body: at most :data:`MAX_BODY_BYTES` as sent (gzip or plain JSON) and
   :data:`MAX_DECOMPRESSED_BYTES` after decompression; at most
   :data:`MAX_EVENTS` events. Violations reject the whole batch.
2. Each event is validated on its own; invalid ones are reported in
   ``rejected`` with a short reason and the rest are still stored.
3. ``server_adjusted_* = device timestamp - clock_offset_ms``.
4. ``INSERT ... ON CONFLICT (event_id, timestamp_start) DO NOTHING``: a resent
   event is reported in ``duplicates`` and not stored twice.

Not yet: raw archive (WH-6) and engine job enqueue (WH-7).

Never log event contents; counts only.
"""

from __future__ import annotations

import gzip
import json
import uuid
import zlib
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta

from secretary_orm import Database

from secretary_warehouse.db import user_transaction

SCHEMA_VERSIONS = frozenset({1})
#: ``secretary_shared::event::EVENT_TYPES`` (Phase 1).
EVENT_TYPES = frozenset({
  'presence.active', 'presence.idle', 'window.focus',
  'editor.workspace_open', 'editor.file_focus', 'editor.edit_burst', 'editor.save',
  'editor.debug_session',
  'git.commit', 'git.checkout', 'git.merge', 'git.backfill_commit',
  'agent.heartbeat', 'agent.pause', 'agent.resume', 'agent.buffer_overflow',
})

MAX_EVENTS = 1000
MAX_BODY_BYTES = 1024 * 1024
MAX_DECOMPRESSED_BYTES = 16 * 1024 * 1024
MAX_METADATA_BYTES = 8 * 1024
MAX_TEXT = 2048
#: Events may not start (server-adjusted) later than this after receipt.
MAX_FUTURE_SKEW = timedelta(minutes=5)
#: Older events are refused: partitions exist 12 months back (``app_ensure_event_partitions``).
MAX_AGE = timedelta(days=360)
#: A device clock further off than this is broken, not skewed.
MAX_CLOCK_OFFSET_MS = 24 * 3600 * 1000


class BatchError(Exception):
  """The whole batch is refused (HTTP 400 / 413)."""

  def __init__(self, status: int, code: str):
    super().__init__(code)
    self.status = status
    self.code = code


class Rejected(Exception):
  def __init__(self, reason: str):
    super().__init__(reason)
    self.reason = reason


@dataclass
class BatchResult:
  accepted: list[str] = field(default_factory=list)
  duplicates: list[str] = field(default_factory=list)
  rejected: list[dict] = field(default_factory=list)

  def as_json(self, server_time: datetime) -> dict:
    return {
      'accepted': self.accepted,
      'duplicates': self.duplicates,
      'rejected': self.rejected,
      'server_time': iso(server_time),
    }


def decode_body(raw: bytes, content_encoding: str | None) -> dict:
  if len(raw) > MAX_BODY_BYTES:
    raise BatchError(413, 'batch_too_large')
  encoding = (content_encoding or 'identity').strip().lower()
  if encoding == 'gzip':
    try:
      # Bounded decompression: a small body must not expand without limit.
      inflater = zlib.decompressobj(16 + zlib.MAX_WBITS)
      raw = inflater.decompress(raw, MAX_DECOMPRESSED_BYTES + 1)
      if len(raw) > MAX_DECOMPRESSED_BYTES or inflater.unconsumed_tail:
        raise BatchError(413, 'batch_too_large')
    except (zlib.error, gzip.BadGzipFile, EOFError):
      raise BatchError(400, 'invalid_body') from None
  elif encoding != 'identity':
    raise BatchError(400, 'invalid_body')
  try:
    body = json.loads(raw)
  except (UnicodeDecodeError, ValueError):
    raise BatchError(400, 'invalid_body') from None
  if not isinstance(body, dict):
    raise BatchError(400, 'invalid_body')
  return body


def ingest(db: Database, user_id: int, device_id: int, body: dict, now: datetime | None = None) -> BatchResult:
  now = now or datetime.now(UTC)
  events = body.get('events')
  offset = body.get('clock_offset_ms', 0)
  try:
    batch_id = uuid.UUID(str(body.get('batch_id')))
  except ValueError:
    raise BatchError(400, 'invalid_body') from None
  if not isinstance(events, list) or not _is_int(offset) or abs(offset) > MAX_CLOCK_OFFSET_MS:
    raise BatchError(400, 'invalid_body')
  if len(events) > MAX_EVENTS:
    raise BatchError(413, 'batch_too_large')

  result = BatchResult()
  rows, order = [], []
  for event in events:
    event_id = event.get('event_id') if isinstance(event, dict) else None
    try:
      row = validate_event(event, now, timedelta(milliseconds=offset))
    except Rejected as rejection:
      result.rejected.append({'event_id': event_id if isinstance(event_id, str) else None,
                              'reason': rejection.reason})
      continue
    rows.append(row)
    order.append(row['event_id'])

  if rows:
    with user_transaction(db, user_id) as conn:
      inserted = {
        str(r['event_id'])
        for r in conn.execute(INSERT_SQL, {
          'user_id': user_id, 'device_id': device_id, 'batch_id': str(batch_id),
          'rows': json.dumps(rows),
        }).fetchall()
      }
    seen = set()
    for event_id in order:
      if event_id in seen:
        continue  # repeated inside the batch: reported once
      seen.add(event_id)
      (result.accepted if event_id in inserted else result.duplicates).append(event_id)
  return result


def validate_event(event, now: datetime, offset: timedelta) -> dict:
  if not isinstance(event, dict):
    raise Rejected('invalid_event')

  try:
    event_id = uuid.UUID(event.get('event_id') or '')
  except (TypeError, ValueError, AttributeError):
    raise Rejected('invalid_event_id') from None
  if event_id.version != 7:
    raise Rejected('invalid_event_id')

  version = event.get('schema_version')
  if not _is_int(version) or version not in SCHEMA_VERSIONS:
    raise Rejected('unsupported_schema_version')

  event_type = event.get('event_type')
  if event_type not in EVENT_TYPES:
    raise Rejected('unknown_event_type')

  source = event.get('source')
  if not isinstance(source, str) or len(source) > 128:
    raise Rejected('invalid_source')
  name, at, ver = source.partition('@')
  if not (name and at and ver):
    raise Rejected('invalid_source')

  start = _timestamp(event.get('timestamp_start'), required=True)
  end = _timestamp(event.get('timestamp_end'), required=False)
  if end is not None and end < start:
    raise Rejected('invalid_timestamps')
  adjusted_start = start - offset
  adjusted_end = None if end is None else end - offset
  latest = now + MAX_FUTURE_SKEW
  if adjusted_start > latest or (adjusted_end is not None and adjusted_end > latest):
    raise Rejected('timestamp_in_future')
  # The partition key is the device timestamp, so bound it too (partitions: 12 back, 3 ahead).
  if start < now - MAX_AGE or adjusted_start < now - MAX_AGE or start > now + timedelta(days=60):
    raise Rejected('timestamp_out_of_range')

  metadata = event.get('metadata')
  if not isinstance(metadata, dict):
    raise Rejected('invalid_metadata')
  metadata_json = json.dumps(metadata, separators=(',', ':'))
  if len(metadata_json.encode()) > MAX_METADATA_BYTES:
    raise Rejected('metadata_too_large')

  repository = event.get('repository')
  if repository is not None and not isinstance(repository, dict):
    raise Rejected('invalid_repository')
  repository = repository or {}

  return {
    'event_id': str(event_id),
    'schema_version': version,
    'event_type': event_type,
    'source': source,
    'timestamp_start': iso(start),
    'timestamp_end': None if end is None else iso(end),
    'server_adjusted_start': iso(adjusted_start),
    'server_adjusted_end': None if adjusted_end is None else iso(adjusted_end),
    'project_hint': _text(event.get('project_hint'), 'invalid_project_hint', 255),
    'repository_root': _text(repository.get('root_path'), 'invalid_repository'),
    'repository_remote': _text(repository.get('remote_url'), 'invalid_repository'),
    'working_directory': _text(event.get('working_directory'), 'invalid_working_directory'),
    'metadata': metadata,
  }


INSERT_SQL = """
INSERT INTO activity_events (
    event_id, user_id, device_id, batch_id, schema_version, event_type, source,
    timestamp_start, timestamp_end, server_adjusted_start, server_adjusted_end,
    project_hint, repository_root, repository_remote, working_directory, metadata)
SELECT r.event_id, %(user_id)s, %(device_id)s, %(batch_id)s::uuid, r.schema_version, r.event_type, r.source,
       r.timestamp_start, r.timestamp_end, r.server_adjusted_start, r.server_adjusted_end,
       r.project_hint, r.repository_root, r.repository_remote, r.working_directory,
       COALESCE(r.metadata, '{}'::jsonb)
FROM jsonb_to_recordset(%(rows)s::jsonb) AS r(
    event_id uuid, schema_version smallint, event_type text, source text,
    timestamp_start timestamptz, timestamp_end timestamptz,
    server_adjusted_start timestamptz, server_adjusted_end timestamptz,
    project_hint text, repository_root text, repository_remote text, working_directory text,
    metadata jsonb)
ON CONFLICT (event_id, timestamp_start) DO NOTHING
RETURNING event_id
"""


def iso(value: datetime) -> str:
  return value.astimezone(UTC).isoformat().replace('+00:00', 'Z')


def _is_int(value) -> bool:
  return isinstance(value, int) and not isinstance(value, bool)


def _timestamp(value, required: bool) -> datetime | None:
  if value is None:
    if required:
      raise Rejected('invalid_timestamps')
    return None
  if not isinstance(value, str) or len(value) > 40:
    raise Rejected('invalid_timestamps')
  try:
    parsed = datetime.fromisoformat(value)
  except ValueError:
    raise Rejected('invalid_timestamps') from None
  if parsed.tzinfo is None:
    raise Rejected('invalid_timestamps')  # device times must be UTC / offset-qualified
  return parsed.astimezone(UTC)


def _text(value, reason: str, limit: int = MAX_TEXT) -> str | None:
  if value is None:
    return None
  if not isinstance(value, str) or len(value) > limit:
    raise Rejected(reason)
  return value
