"""Configuration from environment variables.

| Variable | Default | Meaning |
|---|---|---|
| ``DATABASE_URL`` | required | A login role in ``secretary_app`` (RLS applies), never the table owner |
| ``OVERVIEW_BIND`` | ``0.0.0.0:8090`` | Listen address |
| ``PUBLIC_URL`` | ``http://localhost:8090`` | Where browsers reach the dashboard; redirect URIs and cookies derive from it |
| ``KEYCLOAK_ISSUER`` | ``http://localhost:8180/realms/secretary`` | Public realm issuer (the ID token's ``iss``) |
| ``KEYCLOAK_INTERNAL_URL`` | the issuer | Realm URL for server-to-Keycloak calls (inside Compose: ``http://keycloak:8080/realms/secretary``) |
| ``OIDC_CLIENT_ID`` | ``secretary-dashboard`` | Confidential client (auth code + PKCE) |
| ``OIDC_CLIENT_SECRET`` | required | Its secret |
| ``SESSION_SECRET`` | required | Signs the short-lived login-state cookie (32+ characters) |
| ``SESSION_HOURS`` | ``12`` | Dashboard session lifetime |
| ``STATIC_DIR`` | unset | Built web app to serve (``overview/web/dist``) |
| ``DATABASE_MAX_CONNECTIONS`` | ``10`` | Pool size |
| ``LOG_FORMAT`` / ``LOG_LEVEL`` | ``text`` / ``info`` | As in the warehouse |
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from typing import Callable, Mapping
from urllib.parse import urlsplit


class ConfigError(ValueError):
  pass


@dataclass(frozen=True)
class Config:
  database_url: str = field(repr=False)
  oidc_client_secret: str = field(repr=False)
  session_secret: str = field(repr=False)
  host: str = '0.0.0.0'
  port: int = 8090
  public_url: str = 'http://localhost:8090'
  keycloak_issuer: str = 'http://localhost:8180/realms/secretary'
  keycloak_internal_url: str = 'http://localhost:8180/realms/secretary'
  oidc_client_id: str = 'secretary-dashboard'
  session_hours: int = 12
  static_dir: str | None = None
  database_max_connections: int = 10
  log_format: str = 'text'
  log_level: str = 'info'

  @property
  def secure_cookies(self) -> bool:
    return urlsplit(self.public_url).scheme == 'https'

  @property
  def origin(self) -> str:
    parts = urlsplit(self.public_url)
    return f'{parts.scheme}://{parts.netloc}'

  @property
  def redirect_uri(self) -> str:
    return self.public_url.rstrip('/') + '/auth/callback'

  @classmethod
  def from_env(cls) -> 'Config':
    return cls.from_lookup(os.environ.get)

  @classmethod
  def from_lookup(cls, lookup: Callable[[str], str | None] | Mapping[str, str]) -> 'Config':
    raw = lookup.get if isinstance(lookup, Mapping) else lookup

    def get(name: str, default: str | None = None) -> str | None:
      value = raw(name)
      return value.strip() if value is not None and value.strip() else default

    def required(name: str) -> str:
      value = get(name)
      if value is None:
        raise ConfigError(f'{name} is required')
      return value

    secret = required('SESSION_SECRET')
    if len(secret) < 32:
      raise ConfigError('SESSION_SECRET must be at least 32 characters')
    host, _, port = (get('OVERVIEW_BIND') or '0.0.0.0:8090').rpartition(':')
    if not host or not port.isdigit():
      raise ConfigError('OVERVIEW_BIND must be host:port')
    issuer = get('KEYCLOAK_ISSUER', cls.keycloak_issuer).rstrip('/')
    try:
      hours = int(get('SESSION_HOURS', '12'))
      max_connections = int(get('DATABASE_MAX_CONNECTIONS', '10'))
    except ValueError as exc:
      raise ConfigError(f'invalid number: {exc}') from None
    public_url = get('PUBLIC_URL', cls.public_url).rstrip('/')
    if urlsplit(public_url).scheme not in ('http', 'https'):
      raise ConfigError('PUBLIC_URL must be an http(s) URL')
    return cls(
      database_url=required('DATABASE_URL'),
      oidc_client_secret=required('OIDC_CLIENT_SECRET'),
      session_secret=secret,
      host=host,
      port=int(port),
      public_url=public_url,
      keycloak_issuer=issuer,
      keycloak_internal_url=get('KEYCLOAK_INTERNAL_URL', issuer).rstrip('/'),
      oidc_client_id=get('OIDC_CLIENT_ID', cls.oidc_client_id),
      session_hours=hours,
      static_dir=get('STATIC_DIR'),
      database_max_connections=max_connections,
      log_format=get('LOG_FORMAT', 'text'),
      log_level=get('LOG_LEVEL', 'info'),
    )
