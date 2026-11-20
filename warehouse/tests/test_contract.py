"""The generated contract (SH-9) is loaded and used instead of hand copies."""

from datetime import UTC, datetime, timedelta

import pytest

from secretary_warehouse import agent_config, contract, ingest
from secretary_warehouse.ingest import Rejected, validate_event

SCHEMAS = ('event', 'batch_request', 'batch_response', 'auth_config', 'register_device',
           'device_registered', 'heartbeat', 'heartbeat_response', 'api_error', 'agent_config')


@pytest.mark.parametrize('name', SCHEMAS)
def test_every_schema_is_valid_2020_12(name):
  assert contract.validator(name) is not None


def test_ingest_uses_the_contract_constants():
  assert ingest.EVENT_TYPES == contract.EVENT_TYPES
  assert len(contract.EVENT_TYPES) == 16
  assert ingest.INTERVAL_TYPES <= ingest.EVENT_TYPES
  assert ingest.MAX_EVENTS == 1000 and ingest.MAX_METADATA_BYTES == 8 * 1024
  assert ingest.MAX_FUTURE_SKEW == timedelta(minutes=5)
  assert {'missing_field', 'invalid_metadata', 'timestamp_in_future'} <= contract.REASON_CODES
  assert {'invalid_token', 'device_revoked', 'batch_too_large'} <= contract.ERROR_CODES


def test_agent_config_defaults_match_the_contract_and_schema():
  body = {
    'exclusions': {'paths': [], 'apps': [], 'domains': []},
    'pause_windows': [{'start': '12:00', 'end': '13:00', 'days': [1]}],
    'project_roots': [], 'redaction_version': agent_config.REDACTION_VERSION,
    **agent_config.DEFAULTS,
  }
  assert agent_config.DEFAULTS == {
    'upload_interval_secs': 60, 'upload_max_events': 500, 'heartbeat_interval_secs': 60,
    'buffer_max_age_days': 30, 'edit_aggregation_secs': 30,
  }
  contract.validator('agent_config').validate(body)
  bad = dict(body, pause_windows=[{'start': 'noon', 'end': '13:00'}])
  assert not contract.validator('agent_config').is_valid(bad)


def _event(**overrides):
  now = datetime.now(UTC).replace(microsecond=0)
  body = {
    'event_id': '019a5e1c-0000-7000-8000-000000000001',
    'schema_version': 1,
    'timestamp_start': (now - timedelta(minutes=2)).isoformat(),
    'timestamp_end': None,
    'source': 'agent@0.1.0',
    'event_type': 'git.checkout',
    'metadata': {'from_branch': 'main', 'to_branch': 'feature'},
  }
  body.update(overrides)
  return body


def test_metadata_shape_is_checked_per_type():
  now = datetime.now(UTC)
  validate_event(_event(), now, timedelta(0))
  for metadata in ({'from_branch': 5}, {'to_branch': ['x']}):
    with pytest.raises(Rejected) as exc:
      validate_event(_event(metadata=metadata), now, timedelta(0))
    assert exc.value.reason == 'invalid_metadata', metadata
  with pytest.raises(Rejected) as exc:
    validate_event(_event(event_type='agent.pause', metadata={'reason': 'lunch'}), now, timedelta(0))
  assert exc.value.reason == 'invalid_metadata'
  validate_event(_event(event_type='agent.pause', metadata={'reason': 'user'}), now, timedelta(0))


@pytest.mark.parametrize('model, name', [('RegisterDevice', 'register_device'), ('Heartbeat', 'heartbeat')])
def test_request_models_match_the_generated_schemas(model, name):
  # The pydantic models add database length limits; their fields must not drift.
  from secretary_warehouse import http

  fields = getattr(http, model).model_fields
  schema = contract.schema(name)
  assert set(fields) == set(schema['properties'])
  assert {f for f, info in fields.items() if info.is_required()} == set(schema.get('required', []))
