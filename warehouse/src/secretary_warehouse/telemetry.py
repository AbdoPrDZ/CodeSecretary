"""Logging setup.

PRIVACY RULE: never log event metadata (titles, URLs, paths, commands),
request bodies, tokens, or connection strings. Log IDs, counts, status codes,
durations, and error kinds only.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone

_STANDARD = set(vars(logging.makeLogRecord({}))) | {'message', 'asctime', 'color_message'}


class JsonFormatter(logging.Formatter):
  def format(self, record: logging.LogRecord) -> str:
    entry = {
      'timestamp': datetime.fromtimestamp(record.created, timezone.utc).isoformat(),
      'level': record.levelname,
      'target': record.name,
      'message': record.getMessage(),
    }
    # Structured fields passed with ``extra={...}``.
    entry.update({k: v for k, v in vars(record).items() if k not in _STANDARD})
    if record.exc_info:
      entry['error'] = self.formatException(record.exc_info)
    return json.dumps(entry, default=str)


def init(log_format: str = 'text', level: str = 'info') -> None:
  handler = logging.StreamHandler()
  if log_format == 'json':
    handler.setFormatter(JsonFormatter())
  else:
    handler.setFormatter(logging.Formatter('%(asctime)s %(levelname)s %(name)s: %(message)s'))

  root = logging.getLogger()
  root.handlers[:] = [handler]
  root.setLevel(level.upper())
  # Uvicorn logs through the root handler, so both formats apply to it too.
  for name in ('uvicorn', 'uvicorn.error', 'uvicorn.access'):
    logger = logging.getLogger(name)
    logger.handlers.clear()
    logger.propagate = True
