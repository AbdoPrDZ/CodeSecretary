"""Agent configuration (WH-9): ``GET /v1/agent/config`` with an ``ETag``.

The body is ``secretary_shared::config::AgentConfig`` (Rust); its schema and
defaults come from the generated :mod:`secretary_warehouse.contract`. Built
from the user's ``exclusions`` rows under RLS:

- ``path`` / ``app`` / ``domain`` rows → ``exclusions.paths|apps|domains``;
- ``time_window`` rows → ``pause_windows`` (device local time, ISO weekdays).

Project roots have no storage yet (overview settings, later), so they are
always empty. The agent timings are the blueprint Appendix A defaults.

The ``ETag`` is a hash of the body, so it changes exactly when the config does;
an agent sending it back in ``If-None-Match`` gets ``304`` with no body.
"""

from __future__ import annotations

import hashlib
import json
from datetime import time

from secretary_orm import Database

from secretary_warehouse import contract
from secretary_warehouse.db import user_transaction

REDACTION_VERSION = contract.REDACTION_VERSION
#: ``secretary_shared::config::AgentConfig::default()`` timings (blueprint Appendix A).
DEFAULTS = {
  key: value for key, value in contract.AGENT_CONFIG_DEFAULTS.items()
  if key not in ('exclusions', 'pause_windows', 'project_roots', 'redaction_version')
}

_KINDS = {'path': 'paths', 'app': 'apps', 'domain': 'domains'}


def build(db: Database, user_id: int) -> dict:
  with user_transaction(db, user_id) as conn:
    rows = conn.execute(
      """SELECT kind, pattern, window_start, window_end, days_of_week
         FROM exclusions WHERE user_id = %s ORDER BY id""",
      [user_id],
    ).fetchall()

  exclusions = {'paths': [], 'apps': [], 'domains': []}
  pause_windows = []
  for row in rows:
    if row['kind'] == 'time_window':
      pause_windows.append({
        'start': _hh_mm(row['window_start']),
        'end': _hh_mm(row['window_end']),
        'days': _days(row['days_of_week']),
      })
    elif row['kind'] in _KINDS and row['pattern']:
      exclusions[_KINDS[row['kind']]].append(row['pattern'])

  return {
    'exclusions': exclusions,
    'pause_windows': pause_windows,
    'project_roots': [],
    'redaction_version': REDACTION_VERSION,
    **DEFAULTS,
  }


def etag(config: dict) -> str:
  canonical = json.dumps(config, sort_keys=True, separators=(',', ':'), ensure_ascii=False)
  return '"' + hashlib.sha256(canonical.encode()).hexdigest()[:32] + '"'


def matches(if_none_match: str | None, tag: str) -> bool:
  """RFC 9110 weak comparison of ``If-None-Match`` against ``tag``."""

  if not if_none_match:
    return False
  candidates = [c.strip() for c in if_none_match.split(',')]
  return '*' in candidates or tag in (c.removeprefix('W/') for c in candidates)


def _hh_mm(value: time) -> str:
  return value.strftime('%H:%M')


def _days(value) -> list[int]:
  if isinstance(value, str):
    value = json.loads(value)
  if not isinstance(value, list):
    return []  # NULL: every day
  return sorted({d for d in value if isinstance(d, int) and not isinstance(d, bool) and 1 <= d <= 7})
