"""Headless check of the agent's Keycloak device login (D14). Stdlib only.

    python deploy/keycloak/smoke_device_flow.py [--issuer URL] [--user U] [--password P]

Runs the OAuth 2.0 device authorization grant against the dev realm the way the
agent will, filling the browser step in automatically, then checks what the
warehouse relies on: issuer, ``aud`` = secretary-warehouse, ``azp`` =
secretary-agent, ``sub``, ``sid``, an offline refresh token, and refresh-token
rotation. Exit code 0 when every check passes.

Prints claim names and checks only, never tokens.
"""

from __future__ import annotations

import argparse
import base64
import html
import http.cookiejar
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

CLIENT_ID = 'secretary-agent'
AUDIENCE = 'secretary-warehouse'


class LocalhostIsSecure(http.cookiejar.DefaultCookiePolicy):
  """Send ``Secure`` cookies to localhost over plain HTTP, like browsers do
  (Keycloak marks its login cookies Secure even in dev mode)."""

  def return_ok_secure(self, cookie, request):
    host = urllib.parse.urlsplit(request.get_full_url()).hostname
    return host in ('localhost', '127.0.0.1') or super().return_ok_secure(cookie, request)


def post_form(opener, url, data):
  body = urllib.parse.urlencode(data).encode()
  try:
    with opener.open(urllib.request.Request(url, data=body, method='POST')) as response:
      return response.status, response.read().decode()
  except urllib.error.HTTPError as err:
    return err.code, err.read().decode()


def claims(jwt: str) -> dict:
  payload = jwt.split('.')[1]
  return json.loads(base64.urlsafe_b64decode(payload + '=' * (-len(payload) % 4)))


def form_action(page: str) -> str:
  match = re.search(r'<form[^>]*action="([^"]+)"', page)
  if not match:
    raise SystemExit('no form on the Keycloak page (unexpected login screen)')
  return html.unescape(match.group(1))


ISSUER_ORIGIN = 'http://localhost:8180'


def absolute(url: str) -> str:
  """Keycloak pages use relative form actions on some screens."""

  return urllib.parse.urljoin(ISSUER_ORIGIN + '/', url)


def main() -> int:
  parser = argparse.ArgumentParser()
  parser.add_argument('--issuer', default='http://localhost:8180/realms/secretary')
  parser.add_argument('--user', default='dev@example.test')
  parser.add_argument('--password', default='dev')
  args = parser.parse_args()
  oidc = f'{args.issuer}/protocol/openid-connect'
  global ISSUER_ORIGIN
  parts = urllib.parse.urlsplit(args.issuer)
  ISSUER_ORIGIN = f'{parts.scheme}://{parts.netloc}'

  api = urllib.request.build_opener()
  browser = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar(policy=LocalhostIsSecure())))
  failures = []

  def check(name, ok):
    print(f"  [{'ok' if ok else 'FAIL'}] {name}")
    if not ok:
      failures.append(name)

  # 1. Agent: ask for a device code.
  status, body = post_form(api, f'{oidc}/auth/device', {
    'client_id': CLIENT_ID, 'scope': 'openid offline_access',
  })
  if status != 200:
    print(f'device authorization failed: HTTP {status}')
    return 1
  device = json.loads(body)
  print(f"device code issued; user code {device['user_code']}, expires in {device['expires_in']}s")

  # 2. "Browser": open the verification link and sign in.
  with browser.open(device['verification_uri_complete']) as response:
    page = response.read().decode()
  if 'kc-form-login' not in page and 'username' not in page:
    # Some Keycloak versions first ask to confirm the user code.
    status, page = post_form(browser, absolute(form_action(page)), {'device_user_code': device['user_code']})
  status, page = post_form(browser, absolute(form_action(page)), {
    'username': args.user, 'password': args.password, 'credentialId': '',
  })
  check('user signs in on the device page', status == 200 and 'Invalid username' not in page)

  # Keycloak always asks the user to confirm a device grant ("Grant access?").
  if 'login-actions/consent' in page:
    code = re.search(r'name="code"[^>]*value="([^"]*)"', page)
    status, page = post_form(browser, absolute(form_action(page)), {
      'code': code.group(1) if code else '', 'accept': 'Yes',
    })
    check('user grants the agent access', status == 200)

  # 3. Agent: poll for tokens.
  tokens = None
  deadline = time.monotonic() + 30
  while time.monotonic() < deadline:
    status, body = post_form(api, f'{oidc}/token', {
      'grant_type': 'urn:ietf:params:oauth:grant-type:device_code',
      'device_code': device['device_code'], 'client_id': CLIENT_ID,
    })
    data = json.loads(body)
    if status == 200:
      tokens = data
      break
    if data.get('error') not in ('authorization_pending', 'slow_down'):
      print(f"token polling failed: {data.get('error')}")
      return 1
    time.sleep(device.get('interval', 5))
  check('agent receives tokens', tokens is not None)
  if tokens is None:
    return 1

  access = claims(tokens['access_token'])
  refresh = claims(tokens['refresh_token'])
  audience = access.get('aud')
  audience = audience if isinstance(audience, list) else [audience]
  check(f"iss = {args.issuer}", access.get('iss') == args.issuer)
  check(f'aud contains {AUDIENCE}', AUDIENCE in audience)
  check(f'azp = {CLIENT_ID}', access.get('azp') == CLIENT_ID)
  check('sub present (-> users)', bool(access.get('sub')))
  check('sid present (-> devices.auth_session_id)', bool(access.get('sid')))
  check('access token lifetime <= 300 s', access['exp'] - access['iat'] <= 300)
  check('refresh token is offline', refresh.get('typ') == 'Offline')
  check('email claim present', bool(access.get('email')))

  # 4. Refresh rotates: the new token works, the old one is rejected.
  status, body = post_form(api, f'{oidc}/token', {
    'grant_type': 'refresh_token', 'refresh_token': tokens['refresh_token'], 'client_id': CLIENT_ID,
  })
  refreshed = json.loads(body)
  check('refresh succeeds', status == 200)
  if status == 200:
    check('sid unchanged after refresh', claims(refreshed['access_token']).get('sid') == access.get('sid'))
  status, _ = post_form(api, f'{oidc}/token', {
    'grant_type': 'refresh_token', 'refresh_token': tokens['refresh_token'], 'client_id': CLIENT_ID,
  })
  check('old refresh token is rejected (rotation)', status == 400)

  print('all checks passed' if not failures else f'{len(failures)} check(s) failed')
  return 0 if not failures else 1


if __name__ == '__main__':
  sys.exit(main())
