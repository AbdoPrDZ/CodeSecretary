"""Agent request authentication: Keycloak access tokens (D14, WAREHOUSE §4.2).

1. :class:`TokenVerifier` checks the JWT itself: RS256 signature against the
   realm JWKS (cached, refetched on an unknown ``kid``), ``iss``, ``exp`` /
   ``nbf`` / ``iat`` with 30 s leeway, ``aud`` contains ``secretary-warehouse``,
   ``azp`` is ``secretary-agent``, ``typ`` is ``Bearer``, and ``sub``, ``sid``
   and ``email`` are present.
2. :class:`Authenticator` maps the token to tenant identity: ``sub`` → user via
   ``app_resolve_identity()`` (provisions on first sight), ``sid`` → the device
   registered for that Keycloak session. Results are cached for at most
   :data:`CACHE_TTL` seconds, so a revoked device is rejected within a minute
   even though its access token is still valid.

Never log tokens or claims; log error kinds only.
"""

from __future__ import annotations

import logging
import threading
import time
from dataclasses import dataclass
from typing import Protocol

import jwt
from secretary_orm import Database

from secretary_warehouse.db import system_transaction, user_transaction

logger = logging.getLogger('secretary_warehouse.auth')

#: Clock leeway for ``exp`` / ``nbf`` / ``iat``.
LEEWAY_SECONDS = 30
#: How long a (subject, session) → (user, device) lookup is reused.
CACHE_TTL = 60.0
ALGORITHMS = ['RS256']


@dataclass(frozen=True)
class AuthSettings:
  issuer: str = 'http://localhost:8180/realms/secretary'
  jwks_url: str = 'http://localhost:8180/realms/secretary/protocol/openid-connect/certs'
  client_id: str = 'secretary-agent'
  audience: str = 'secretary-warehouse'

  @classmethod
  def from_config(cls, config) -> 'AuthSettings':
    return cls(
      issuer=config.keycloak_issuer,
      jwks_url=config.keycloak_jwks_url,
      client_id=config.keycloak_agent_client_id,
      audience=config.keycloak_audience,
    )


class AuthError(Exception):
  """The request is not authenticated (or not allowed); maps to an HTTP error."""

  def __init__(self, status: int, code: str, kind: str = ''):
    super().__init__(code)
    self.status = status
    self.code = code
    self.kind = kind  # for logs: why, without any token content


class AuthUnavailable(Exception):
  """Keys could not be fetched from Keycloak: a server problem, not the client's (503)."""


def invalid_token(kind: str) -> AuthError:
  return AuthError(401, 'invalid_token', kind)


class KeyResolver(Protocol):
  def get_signing_key_from_jwt(self, token: str): ...


@dataclass(frozen=True)
class TokenClaims:
  issuer: str
  subject: str
  session_id: str
  email: str
  name: str | None


class TokenVerifier:
  def __init__(self, settings: AuthSettings, keys: KeyResolver | None = None):
    self.settings = settings
    # PyJWKClient caches the key set and refetches it when a token names an unknown kid.
    self._keys = keys or jwt.PyJWKClient(settings.jwks_url, cache_keys=True, lifespan=3600, timeout=5)

  def verify(self, token: str) -> TokenClaims:
    try:
      header = jwt.get_unverified_header(token)
    except jwt.PyJWTError as exc:
      raise invalid_token(type(exc).__name__) from None
    if header.get('alg') not in ALGORITHMS:
      raise invalid_token('algorithm')

    try:
      key = self._keys.get_signing_key_from_jwt(token).key
    except jwt.PyJWKClientConnectionError as exc:
      raise AuthUnavailable(type(exc).__name__) from None
    except jwt.PyJWTError as exc:  # unknown kid even after a refetch, malformed key set
      raise invalid_token(type(exc).__name__) from None

    try:
      claims = jwt.decode(
        token,
        key,
        algorithms=ALGORITHMS,
        audience=self.settings.audience,
        issuer=self.settings.issuer,
        leeway=LEEWAY_SECONDS,
        options={'require': ['exp', 'iat', 'iss', 'aud', 'sub']},
      )
    except jwt.PyJWTError as exc:
      raise invalid_token(type(exc).__name__) from None

    if claims.get('typ') != 'Bearer':
      raise invalid_token('token_type')  # refresh / ID / offline tokens are not access tokens
    if claims.get('azp') != self.settings.client_id:
      raise invalid_token('client')
    session_id, email = claims.get('sid'), claims.get('email')
    if not isinstance(session_id, str) or not session_id:
      raise invalid_token('no_session')
    if not isinstance(email, str) or not email:
      raise invalid_token('no_email')
    name = claims.get('name')
    return TokenClaims(
      issuer=claims['iss'],
      subject=claims['sub'],
      session_id=session_id,
      email=email,
      name=name if isinstance(name, str) and name else None,
    )


@dataclass(frozen=True)
class Identity:
  """A verified token mapped to its user."""

  user_id: int
  session_id: str


@dataclass(frozen=True)
class Principal:
  """A verified token from a registered, non-revoked device."""

  user_id: int
  device_id: int
  session_id: str


@dataclass
class _Entry:
  user_id: int
  device_id: int | None
  revoked: bool
  expires: float


class Authenticator:
  def __init__(self, db: Database, verifier: TokenVerifier, cache_ttl: float = CACHE_TTL):
    self.db = db
    self.verifier = verifier
    self.cache_ttl = cache_ttl
    self._cache: dict[tuple[str, str], _Entry] = {}
    self._lock = threading.Lock()

  def identify(self, authorization: str | None) -> Identity:
    """Valid token → user (provisioned on first sight). No device needed."""

    claims = self.verifier.verify(bearer(authorization))
    entry = self._lookup(claims)
    return Identity(user_id=entry.user_id, session_id=claims.session_id)

  def device(self, authorization: str | None) -> Principal:
    """Valid token whose session belongs to a registered, non-revoked device."""

    claims = self.verifier.verify(bearer(authorization))
    entry = self._lookup(claims)
    if entry.revoked:
      raise AuthError(401, 'device_revoked', 'revoked')
    if entry.device_id is None:
      raise AuthError(403, 'device_not_registered', 'unregistered')
    return Principal(user_id=entry.user_id, device_id=entry.device_id, session_id=claims.session_id)

  def forget(self, subject_session: tuple[str, str] | None = None) -> None:
    """Drop cached lookups (all, or one ``(sub, sid)``), e.g. after registering or revoking."""

    with self._lock:
      if subject_session is None:
        self._cache.clear()
      else:
        self._cache.pop(subject_session, None)

  def _lookup(self, claims: TokenClaims) -> _Entry:
    key = (claims.subject, claims.session_id)
    now = time.monotonic()
    with self._lock:
      entry = self._cache.get(key)
      if entry is not None and entry.expires > now:
        return entry

    with system_transaction(self.db) as conn:
      row = conn.execute(
        'SELECT app_resolve_identity(%s, %s, %s, %s) AS user_id',
        [claims.issuer, claims.subject, claims.email, claims.name],
      ).fetchone()
    user_id = int(_value(row, 'user_id'))

    with user_transaction(self.db, user_id) as conn:
      device = conn.execute(
        'SELECT id, revoked_at FROM devices WHERE auth_session_id = %s', [claims.session_id]
      ).fetchone()

    entry = _Entry(
      user_id=user_id,
      device_id=None if device is None else int(_value(device, 'id', 0)),
      revoked=device is not None and _value(device, 'revoked_at', 1) is not None,
      expires=now + self.cache_ttl,
    )
    with self._lock:
      self._cache[key] = entry
    return entry


def bearer(authorization: str | None) -> str:
  if not authorization:
    raise invalid_token('missing')
  scheme, _, token = authorization.partition(' ')
  token = token.strip()
  if scheme.lower() != 'bearer' or not token:
    raise invalid_token('scheme')
  return token


def _value(row, name: str, index: int = 0):
  """Column from a dict row or a tuple row (the pool's row factory is not ours to assume)."""

  if isinstance(row, dict):
    return row[name]
  return row[index]
