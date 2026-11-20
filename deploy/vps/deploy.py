"""Build the warehouse image here and run the stack on the VPS (no registry).

    python deploy/vps/deploy.py

1. Reads the VPS address (`HOST`) and the stack folder (`DIR`, default
   `/opt/secretary`) from `.ssh.env`; values are never printed.
2. Keeps, in `deploy/vps/secrets/` (git-ignored): `.env.deploy` with the
   deploy-owned values (public IP, image tag, Keycloak admin password, the
   RLS-bound app login's password) and a private CA + server certificate.
3. Builds `secretary-warehouse:<git sha>` locally and streams it into
   `docker load` on the VPS. Keycloak and Caddy are pulled by the VPS.
4. Database settings are the server owner's `DIR/.env.docker`
   (`SECRETARY_WH_DB_*`, `SECRETARY_KC_DB_*`); the databases and their owner
   logins are created by the server owner. Read on the VPS only, the deploy
   adds in `it-pg-db` only `secretary_app` (RLS-bound, no login),
   `secretary_warehouse` (login, member of it) and its CONNECT grant.
5. Uploads `compose.yml`, `Caddyfile`, `.env.deploy`, the server certificate
   and the realm (without the dev test user), writes `DIR/.env` =
   `.env.docker` + `.env.deploy`, then `docker compose up -d --wait`.

Re-running deploys a new image and keeps data, passwords and the CA.
"""

from __future__ import annotations

import io
import ipaddress
import json
import secrets
import subprocess
import sys
import tempfile
from pathlib import Path

import vps

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SECRETS = HERE / 'secrets'
DEFAULT_DIR = '/opt/secretary'
PG_CONTAINER = 'it-pg-db'

#: Runs on the VPS. The databases and their owner logins are the server
#: owner's (.env.docker); this only adds the RLS-bound runtime login and lets
#: it connect. Values reach psql as variables on stdin, never a command line.
DB_SETUP = r'''
set -eu
cd "$1"
set -a; . ./.env; set +a
docker exec -i "$2" sh -c 'psql -q -At -U "$POSTGRES_USER" -d postgres -f -' <<EOSQL
\set ON_ERROR_STOP on
\set wh_db '$SECRETARY_WH_DB_NAME'
\set app_pw '$WAREHOUSE_APP_PASSWORD'
SELECT 'CREATE ROLE secretary_app NOLOGIN NOBYPASSRLS'
 WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'secretary_app') \gexec
SELECT 'CREATE ROLE secretary_warehouse LOGIN'
 WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'secretary_warehouse') \gexec
ALTER ROLE secretary_app NOLOGIN NOBYPASSRLS;
ALTER ROLE secretary_warehouse LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD :'app_pw';
SELECT 'GRANT secretary_app TO secretary_warehouse'
 WHERE NOT pg_has_role('secretary_warehouse', 'secretary_app', 'MEMBER') \gexec
SELECT format('database %s missing', :'wh_db') WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'wh_db');
GRANT CONNECT ON DATABASE :"wh_db" TO secretary_app;
SELECT 'runtime login ready';
EOSQL
'''


def public_ip() -> str:
  host = vps.settings()['HOST'].partition(':')[0]
  try:
    return str(ipaddress.ip_address(host))
  except ValueError:
    sys.exit('HOST in .ssh.env must be an IP address for the IP certificate')


def read_env(path: Path) -> dict[str, str]:
  if not path.exists():
    return {}
  return dict(line.split('=', 1) for line in path.read_text(encoding='utf-8').splitlines()
              if '=' in line and not line.startswith('#'))


def ensure_deploy_env(ip: str, tag: str) -> Path:
  path = SECRETS / '.env.deploy'
  values = read_env(path)
  old = read_env(SECRETS / '.env')  # first layout; reuse its admin password
  values.setdefault('KEYCLOAK_ADMIN_PASSWORD', old.get('KEYCLOAK_ADMIN_PASSWORD') or secrets.token_urlsafe(24))
  values.setdefault('WAREHOUSE_APP_PASSWORD', secrets.token_urlsafe(24))
  values['PUBLIC_IP'] = ip
  values['WAREHOUSE_TAG'] = tag
  path.write_text('# Deploy-owned values (deploy/vps/deploy.py). Database settings: .env.docker\n'
                  + ''.join(f'{k}={v}\n' for k, v in values.items()), encoding='utf-8')
  return path


def realm_without_test_users() -> bytes:
  realm = json.loads((ROOT / 'deploy/keycloak/import/secretary-realm.json').read_text(encoding='utf-8'))
  realm.pop('users', None)  # the dev user (dev@example.test / dev) must not exist on a server
  return json.dumps(realm, indent=2).encode()


def sh(*args: str) -> str:
  return subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True).stdout.strip()


def upload_text(client, data: bytes | str, remote: str, mode: int = 0o644) -> None:
  if isinstance(data, str):
    data = data.encode()
  data = data.replace(b'\r\n', b'\n')  # Windows checkouts: files must stay LF
  with client.open_sftp() as sftp:
    sftp.putfo(io.BytesIO(data), remote)
    sftp.chmod(remote, mode)


def step(message: str) -> None:
  print(f'==> {message}', flush=True)


def main() -> int:
  ip = public_ip()
  remote = vps.settings().get('DIR', DEFAULT_DIR).rstrip('/')
  if not remote.startswith('/opt/') or ' ' in remote:
    sys.exit('DIR in .ssh.env must be a folder under /opt/ without spaces')
  SECRETS.mkdir(exist_ok=True)
  tag = sh('git', 'rev-parse', '--short', 'HEAD')
  if sh('git', 'status', '--porcelain', '--', 'warehouse', 'orm'):
    tag += '-dirty'

  step('certificates')
  subprocess.run([sys.executable, str(HERE / 'make_certs.py'), ip, str(SECRETS / 'certs')], check=True)
  deploy_env = ensure_deploy_env(ip, tag)

  step(f'building secretary-warehouse:{tag}')
  subprocess.run(['docker', 'build', '-q', '-f', 'warehouse/Dockerfile', '-t', f'secretary-warehouse:{tag}', '.'],
                 cwd=ROOT, check=True)
  with tempfile.TemporaryDirectory() as tmp:
    tar = Path(tmp) / 'warehouse.tar'
    subprocess.run(['docker', 'save', '-o', str(tar), f'secretary-warehouse:{tag}'], check=True)
    size = tar.stat().st_size / 1e6

    client = vps.connect()
    try:
      if vps.run(client, f'test -f {remote}/.env.docker') != 0:
        sys.exit(f'{remote}/.env.docker (SECRETARY_WH_DB_*, SECRETARY_KC_DB_*) is missing on the VPS')

      step(f'uploading the image ({size:.0f} MB) into docker load')
      if vps.load(client, str(tar)) != 0:
        return 1

      step(f'files → {remote}')
      vps.run(client, f'mkdir -p {remote}/certs {remote}/keycloak-import && chmod 700 {remote}')
      upload_text(client, (HERE / 'compose.yml').read_bytes(), f'{remote}/compose.yml')
      upload_text(client, (HERE / 'Caddyfile').read_bytes(), f'{remote}/Caddyfile')
      upload_text(client, deploy_env.read_bytes(), f'{remote}/.env.deploy', 0o600)
      upload_text(client, (SECRETS / 'certs/server.crt').read_bytes(), f'{remote}/certs/server.crt')
      # Caddy reads the key as another user inside its container.
      upload_text(client, (SECRETS / 'certs/server.key').read_bytes(), f'{remote}/certs/server.key', 0o644)
      upload_text(client, realm_without_test_users(), f'{remote}/keycloak-import/secretary-realm.json')
      vps.run(client, f'cd {remote} && {{ echo "# Generated by deploy.py from .env.docker + .env.deploy; edit those."; '
                      f'cat .env.docker; echo; cat .env.deploy; }} > .env && chmod 600 .env')

      step(f'runtime login in {PG_CONTAINER}')
      upload_text(client, DB_SETUP, f'{remote}/.db-setup.sh', 0o700)
      code = vps.run(client, f'sh {remote}/.db-setup.sh {remote} {PG_CONTAINER}; s=$?; '
                             f'rm -f {remote}/.db-setup.sh; exit $s')
      if code != 0:
        return code

      step('docker compose up (pulls Keycloak and Caddy on first run)')
      code = vps.run(client, f"cd {remote} && bash -o pipefail -c "
                             f"'docker compose up -d --wait --remove-orphans 2>&1 | tail -15'")
      vps.run(client, f'cd {remote} && docker compose ps --format "{{{{.Service}}}}: {{{{.Status}}}}"')
      return code
    finally:
      client.close()


if __name__ == '__main__':
  sys.exit(main())
