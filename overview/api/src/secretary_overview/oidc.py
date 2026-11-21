"""Keycloak sign-in for the dashboard (OV-2): authorization code + PKCE.

Browser-facing URLs (authorize, end-session) use the public issuer; the token
exchange and signing keys go through ``KEYCLOAK_INTERNAL_URL`` (with
``KC_HOSTNAME_BACKCHANNEL_DYNAMIC`` Keycloak answers discovery on that URL
with backchannel endpoints on the same host). The login state (state, nonce,
PKCE verifier, where to return) lives in a short-lived HMAC-signed cookie.

Never log tokens, codes or claims; error kinds only.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import secrets
import time
from dataclasses import dataclass
from urllib.parse import urlencode

import httpx
import jwt

from secretary_overview.config import Config

LOGIN_TTL = 600  # seconds the browser has to finish signing in


class SignInError(Exception):
  """Sign-in failed; the message is a short kind, safe to log."""


@dataclass(frozen=True)
class LoginState:
  state: str
  nonce: str
  verifier: str
  next: str
  expires: float


@dataclass(frozen=True)
class SignedIn:
  issuer: str
  subject: str
  email: str
  name: str | None
  session_id: str | None
  id_token: str


def safe_next(value: str | None) -> str:
  """Only local paths: no scheme, no host, no protocol-relative `//`."""

  if not value or not value.startswith('/') or value.startswith('//') or '\\' in value:
    return '/'
  return value


class Oidc:
  def __init__(self, config: Config, http: httpx.Client | None = None, keys=None):
    self.config = config
    self.http = http or httpx.Client(timeout=10)
    self._keys = keys
    self._discovery: dict | None = None
    self._discovered_at = 0.0

  # -- endpoints ---------------------------------------------------------------

  def discovery(self) -> dict:
    if self._discovery is None or time.monotonic() - self._discovered_at > 3600:
      url = self.config.keycloak_internal_url + '/.well-known/openid-configuration'
      try:
        response = self.http.get(url)
        response.raise_for_status()
        doc = response.json()
      except (httpx.HTTPError, ValueError):
        raise SignInError('discovery_failed') from None
      if doc.get('issuer') != self.config.keycloak_issuer:
        raise SignInError('issuer_mismatch')
      self._discovery, self._discovered_at = doc, time.monotonic()
    return self._discovery

  def _jwks(self):
    if self._keys is None:
      self._keys = jwt.PyJWKClient(self.discovery()['jwks_uri'], cache_keys=True, lifespan=3600)
    return self._keys

  # -- login state cookie ----------------------------------------------------------

  def new_login(self, next_path: str | None) -> tuple[LoginState, str]:
    login = LoginState(
      state=secrets.token_urlsafe(24),
      nonce=secrets.token_urlsafe(24),
      verifier=secrets.token_urlsafe(48),
      next=safe_next(next_path),
      expires=time.time() + LOGIN_TTL,
    )
    return login, self._sign(login)

  def _sign(self, login: LoginState) -> str:
    body = base64.urlsafe_b64encode(json.dumps(login.__dict__).encode()).decode()
    mac = hmac.new(self.config.session_secret.encode(), body.encode(), hashlib.sha256).hexdigest()
    return f'{body}.{mac}'

  def read_login(self, cookie: str | None, state: str | None) -> LoginState:
    if not cookie or '.' not in cookie or not state:
      raise SignInError('missing_state')
    body, mac = cookie.rsplit('.', 1)
    expected = hmac.new(self.config.session_secret.encode(), body.encode(), hashlib.sha256).hexdigest()
    if not hmac.compare_digest(mac, expected):
      raise SignInError('bad_state')
    try:
      login = LoginState(**json.loads(base64.urlsafe_b64decode(body)))
    except (ValueError, TypeError):
      raise SignInError('bad_state') from None
    if login.expires < time.time():
      raise SignInError('state_expired')
    if not hmac.compare_digest(login.state, state):
      raise SignInError('state_mismatch')
    return login

  # -- flow ------------------------------------------------------------------

  def authorize_url(self, login: LoginState) -> str:
    challenge = base64.urlsafe_b64encode(hashlib.sha256(login.verifier.encode()).digest()).rstrip(b'=')
    params = {
      'response_type': 'code',
      'client_id': self.config.oidc_client_id,
      'redirect_uri': self.config.redirect_uri,
      'scope': 'openid email profile',
      'state': login.state,
      'nonce': login.nonce,
      'code_challenge': challenge.decode(),
      'code_challenge_method': 'S256',
    }
    return self.discovery()['authorization_endpoint'] + '?' + urlencode(params)

  def finish(self, code: str | None, login: LoginState) -> SignedIn:
    if not code:
      raise SignInError('missing_code')
    try:
      response = self.http.post(self.discovery()['token_endpoint'], data={
        'grant_type': 'authorization_code',
        'code': code,
        'redirect_uri': self.config.redirect_uri,
        'code_verifier': login.verifier,
        'client_id': self.config.oidc_client_id,
        'client_secret': self.config.oidc_client_secret,
      })
    except httpx.HTTPError:
      raise SignInError('token_endpoint_unreachable') from None
    if response.status_code != 200:
      raise SignInError('code_rejected')
    id_token = response.json().get('id_token')
    if not id_token:
      raise SignInError('no_id_token')
    claims = self.verify_id_token(id_token, login.nonce)
    email = claims.get('email')
    if not email:
      raise SignInError('no_email')
    return SignedIn(
      issuer=claims['iss'],
      subject=claims['sub'],
      email=email,
      name=claims.get('name'),
      session_id=claims.get('sid'),
      id_token=id_token,
    )

  def verify_id_token(self, id_token: str, nonce: str) -> dict:
    try:
      key = self._jwks().get_signing_key_from_jwt(id_token).key
      claims = jwt.decode(
        id_token, key, algorithms=['RS256'],
        audience=self.config.oidc_client_id, issuer=self.config.keycloak_issuer,
        leeway=30, options={'require': ['exp', 'iat', 'iss', 'aud', 'sub']},
      )
    except jwt.PyJWKClientError:
      raise SignInError('keys_unavailable') from None
    except jwt.InvalidTokenError:
      raise SignInError('invalid_id_token') from None
    if not hmac.compare_digest(str(claims.get('nonce', '')), nonce):
      raise SignInError('nonce_mismatch')
    return claims

  def end_session_url(self, id_token: str | None) -> str:
    params = {'client_id': self.config.oidc_client_id,
              'post_logout_redirect_uri': self.config.public_url + '/'}
    if id_token:
      params['id_token_hint'] = id_token
    try:
      endpoint = self.discovery()['end_session_endpoint']
    except SignInError:
      return self.config.public_url + '/'
    return endpoint + '?' + urlencode(params)
