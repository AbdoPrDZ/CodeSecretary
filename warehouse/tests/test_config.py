"""Configuration from environment variables."""

from __future__ import annotations

import pytest

from secretary_warehouse.config import Config, ConfigError


def test_defaults_with_only_database_url():
  config = Config.from_lookup({'DATABASE_URL': 'postgresql://x'})
  assert (config.host, config.port) == ('0.0.0.0', 8080)
  assert config.database_max_connections == 10
  assert config.log_format == 'text'
  assert config.log_level == 'info'
  assert config.migration_database_url is None


@pytest.mark.parametrize('value', [None, '  '])
def test_database_url_is_required(value):
  lookup = {} if value is None else {'DATABASE_URL': value}
  with pytest.raises(ConfigError, match='DATABASE_URL is required'):
    Config.from_lookup(lookup)


@pytest.mark.parametrize('name,value', [
  ('WAREHOUSE_BIND', 'nope'),
  ('WAREHOUSE_BIND', '0.0.0.0:99999'),
  ('DATABASE_MAX_CONNECTIONS', '0'),
  ('DATABASE_MAX_CONNECTIONS', 'ten'),
  ('LOG_FORMAT', 'xml'),
  ('LOG_LEVEL', 'loud'),
])
def test_invalid_values_are_rejected(name, value):
  with pytest.raises(ConfigError, match=name):
    Config.from_lookup({'DATABASE_URL': 'postgresql://x', name: value})


def test_repr_never_prints_connection_strings():
  config = Config.from_lookup({
    'DATABASE_URL': 'postgresql://u:secret@h/db',
    'DATABASE_MIGRATION_URL': 'postgresql://o:secret2@h/db',
  })
  assert 'secret' not in repr(config)
