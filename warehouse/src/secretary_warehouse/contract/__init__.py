"""The device-API contract, generated from ``shared/`` (SH-9). Do not edit the JSON.

``shared/src/codegen.rs`` writes the files in this package:

- ``constants.json``: event types, limits, reason/error codes, defaults;
- ``<name>.schema.json``: JSON Schema (2020-12) of the event envelope, the
  device API bodies and ``AgentConfig``.

A Rust test fails while they are stale; regenerate with
``SECRETARY_BLESS=1 cargo test -p secretary-shared --features codegen generated``.
"""

from __future__ import annotations

import json
from functools import cache
from importlib.resources import files

from jsonschema import Draft202012Validator


def _load(name: str) -> dict:
  return json.loads(files(__name__).joinpath(name).read_text(encoding='utf-8'))


CONSTANTS: dict = _load('constants.json')

SCHEMA_VERSION: int = CONSTANTS['schema_version']
EVENT_TYPES: frozenset[str] = frozenset(CONSTANTS['event_types'])
INTERVAL_TYPES: frozenset[str] = frozenset(CONSTANTS['interval_types'])
REQUIRED_FIELDS: tuple[str, ...] = tuple(CONSTANTS['required_fields'])
LIMITS: dict = CONSTANTS['limits']
REASON_CODES: frozenset[str] = frozenset(CONSTANTS['reason_codes'])
ERROR_CODES: frozenset[str] = frozenset(CONSTANTS['error_codes'])
REDACTION_VERSION: int = CONSTANTS['redaction_version']
AGENT_CONFIG_DEFAULTS: dict = CONSTANTS['agent_config_defaults']


def schema(name: str) -> dict:
  return _load(f'{name}.schema.json')


@cache
def validator(name: str) -> Draft202012Validator:
  """Compiled validator for ``<name>.schema.json`` (formats are not asserted)."""

  document = schema(name)
  Draft202012Validator.check_schema(document)
  return Draft202012Validator(document)
