"""Generate redaction-corpus-v1.json next to this file (deterministic):

    python shared/fixtures/gen_redaction_corpus.py shared/fixtures/redaction-corpus-v1.json

Secrets are fake but realistically shaped. Each is stored split into parts so
the fixture itself never contains a contiguous token (keeps secret scanners
and push protection quiet); tests join the parts.
"""

import json
import random
import string
import sys

rng = random.Random(20261120)
A = string.ascii_letters + string.digits
U = string.ascii_uppercase + string.digits
H = '0123456789abcdef'
B64 = string.ascii_letters + string.digits + '+/'
URLSAFE = string.ascii_letters + string.digits + '-_'


def r(chars, n):
  # Guarantee a few digits and letters so "looks random" holds.
  s = [rng.choice(chars) for _ in range(n)]
  for i in range(min(3, n)):
    if any(c.isdigit() for c in chars):
      s[rng.randrange(n)] = rng.choice(string.digits)
  return ''.join(s)


def split(secret):
  n = len(secret)
  if n < 6:
    return [secret]
  a = n // 3
  b = 2 * n // 3
  return [secret[:a], secret[a:b], secret[b:]]


TITLE_TEMPLATES = [
  '{s}',
  'export TOKEN_VALUE={s} — bash',
  'curl -H "X-Data: {s}" https://api.example.com/v1 — Terminal',
  '.env — api — {s} — Visual Studio Code',
  'config.json: "value": "{s}" - Notepad',
  'Copied {s} to clipboard',
  'Login failed for {s}',
  '[{s}] deploy log — Chrome',
]


def entry(kind, secret, template=None):
  template = template or rng.choice(TITLE_TEMPLATES)
  return {'kind': kind, 'template': template, 'secret': split(secret)}


items = []
# Provider tokens (contexts vary).
for _ in range(6):
  items.append(entry('aws_access_key', 'AKIA' + r(U, 16)))
for prefix in ['ghp_', 'gho_', 'ghu_', 'ghs_', 'ghr_']:
  for _ in range(2):
    items.append(entry('github_token', prefix + r(A, 36)))
for _ in range(3):
  items.append(entry('github_pat', 'github_pat_' + r(A, 22) + '_' + r(A, 59)))
for _ in range(4):
  items.append(entry('gitlab_pat', 'glpat-' + r(URLSAFE, 20)))
for p in ['xoxb-', 'xoxp-', 'xoxa-', 'xoxs-']:
  items.append(entry('slack_token', p + r(string.digits, 12) + '-' + r(string.digits, 12) + '-' + r(A, 24)))
for _ in range(3):
  items.append(entry('slack_webhook', 'https://hooks.slack.com/services/T' + r(U, 8) + '/B' + r(U, 8) + '/' + r(A, 24)))
for _ in range(2):
  items.append(entry('discord_webhook', 'https://discord.com/api/webhooks/' + r(string.digits, 18) + '/' + r(URLSAFE, 68)))
for p in ['sk_live_', 'sk_test_', 'rk_live_', 'pk_live_']:
  items.append(entry('stripe_key', p + r(A, 24)))
for _ in range(2):
  items.append(entry('stripe_webhook', 'whsec_' + r(A, 32)))
for _ in range(4):
  items.append(entry('google_api_key', 'AIza' + r(URLSAFE, 35)))
for _ in range(2):
  items.append(entry('google_oauth', 'ya29.' + r(URLSAFE, 60)))
for p in ['sk-', 'sk-proj-', 'sk-ant-api03-', 'sk-svcacct-']:
  items.append(entry('ai_api_key', p + r(URLSAFE, 48)))
for _ in range(3):
  items.append(entry('npm_token', 'npm_' + r(A, 36)))
for _ in range(2):
  items.append(entry('pypi_token', 'pypi-AgEIcHlwaS5vcmc' + r(URLSAFE, 60)))
for _ in range(2):
  items.append(entry('sendgrid', 'SG.' + r(URLSAFE, 22) + '.' + r(URLSAFE, 43)))
for p in ['AC', 'SK']:
  items.append(entry('twilio', p + r(H, 32)))
items.append(entry('digitalocean', 'dop_v1_' + r(H, 64)))
for p in ['shpat_', 'shpss_']:
  items.append(entry('shopify', p + r(H, 32)))
for _ in range(2):
  items.append(entry('huggingface', 'hf_' + r(A, 34)))

# JWTs.
for _ in range(6):
  jwt = 'eyJ' + r(URLSAFE, 30) + '.eyJ' + r(URLSAFE, 60) + '.' + r(URLSAFE, 43)
  items.append(entry('jwt', jwt))

# Authorization headers.
for scheme in ['Bearer', 'bearer', 'Token', 'Basic']:
  for _ in range(2):
    secret = r(A + '-._', 40) if scheme != 'Basic' else r(B64, 32) + '=='
    items.append(entry('auth_header', secret, 'Authorization: ' + scheme + ' {s}'))

# KEY=value where the key names a secret (secret = the value).
keys = ['API_KEY', 'api_key', 'SECRET_KEY', 'DB_PASSWORD', 'password', 'client_secret',
        'GITHUB_TOKEN', 'access_token', 'refresh_token', 'aws_secret_access_key',
        'PRIVATE_KEY_PASSPHRASE', 'x-api-key', 'auth', 'session_cookie', 'MYSQL_PWD',
        'webhook_signature', 'STRIPE_SECRET', 'npm_config_auth', 'Credentials', 'passwd']
for key in keys:
  value = r(A, rng.randrange(10, 28))
  sep = rng.choice(['=', ': ', '="', "='", ' = '])
  close = '"' if sep.endswith('"') else ("'" if sep.endswith("'") else '')
  items.append(entry('key_value', value, key + sep + '{s}' + close + ' — env'))
# Short human passwords still go when the key says so.
for key, value in [('password', 'hunter2!'), ('DB_PASSWORD', 'Winter2026'), ('pwd', 'letmein99'),
                   ('PASSWORD', 'correct horse'.replace(' ', '-'))]:
  items.append(entry('key_value_short', value, key + '={s}'))

# Credentials in URLs (secret = the password part).
for scheme in ['postgres', 'postgresql', 'mysql', 'mongodb+srv', 'redis', 'amqp', 'https', 'ftp']:
  password = r(A, 16)
  items.append(entry('url_credentials', password,
                     scheme + '://admin:{s}@db.internal.example:5432/app'))
for _ in range(3):
  items.append(entry('url_token_userinfo', 'ghp_' + r(A, 36),
                     'git clone https://{s}@github.com/acme/api.git'))

# URLs with tokens in the query or fragment (secret = the token).
for param in ['token', 'access_token', 'sig', 'code', 'key', 'state', 'reset']:
  value = r(A, 32)
  items.append(entry('url_query', value,
                     'https://app.example.com/callback?' + param + '={s}&x=1 — Chrome'))
items.append(entry('url_fragment', r(URLSAFE, 40),
                   'https://app.example.com/#access_token={s}'))
for _ in range(3):
  items.append(entry('url_path_token', r(H, 40),
                     'https://app.example.com/reset-password/{s} — Firefox'))

# Private keys.
for kind in ['RSA PRIVATE KEY', 'PRIVATE KEY', 'OPENSSH PRIVATE KEY', 'EC PRIVATE KEY']:
  body = r(B64, 64)
  items.append(entry('private_key', body,
                     '-----BEGIN ' + kind + '-----\n{s}\n-----END ' + kind + '-----'))

# Azure.
items.append(entry('azure_storage', r(B64, 86) + '==',
                   'DefaultEndpointsProtocol=https;AccountName=acme;AccountKey={s};EndpointSuffix=core.windows.net'))
items.append(entry('azure_sas', r(A + '%', 44),
                   'https://acme.blob.core.windows.net/c/f.txt?sv=2022-11-02&sig={s}'))

# Generic high-entropy tokens with no recognizable prefix.
for _ in range(6):
  items.append(entry('generic_hex', r(H, 40)))
for _ in range(6):
  items.append(entry('generic_base64', r(B64, 44) + '='))

# E-mail addresses (window titles only).
for _ in range(5):
  user = ''.join(rng.choice(string.ascii_lowercase) for _ in range(7)) + '.' + \
    ''.join(rng.choice(string.ascii_lowercase) for _ in range(5))
  items.append({'kind': 'email', 'template': 'Inbox — {s} — Mail',
                'secret': split(user + '@example.com'), 'titles_only': True})

benign = [
  'main.rs - api - Visual Studio Code',
  'Pull request #42 · acme/api — Google Chrome',
  'Keyboard Shortcuts — Settings',
  'README.md — secretary — Visual Studio Code',
  'auth.rs - api - Visual Studio Code',
  'Token budget planning — Notion',
  'git log --oneline a1b2c3d',
  'cargo test -p secretary-shared — PowerShell',
  'Issue 1234: Session handling refactor',
  'Rust 1.89 release notes',
  'src/components/UserProfileSettings.tsx — web',
  'localhost:3000 — Firefox',
]

doc = {
  'version': 1,
  'note': 'Fake, generated secrets (gen_redaction_corpus.py, seed 20261120). '
          'Each secret is split into parts; join them and substitute into {s}. '
          'After redaction the joined secret must not appear. '
          'Benign titles must come out unchanged.',
  'items': items,
  'benign_titles': benign,
}
with open(sys.argv[1], 'w', encoding='utf-8', newline='\n') as f:
  json.dump(doc, f, indent=1, ensure_ascii=False)
  f.write('\n')
print(len(items), 'secrets')
