"""Run commands on, and copy files to, the VPS named in `.ssh.env`.

`.ssh.env` (repository root, git-ignored) holds `HOST`, `USER` and `PASS`
(and `DIR`, the stack folder, used by deploy.py).
Their values are never printed; output only shows what the remote command
prints.

    python deploy/vps/vps.py run "docker version"
    python deploy/vps/vps.py put <local file> <remote path>
    python deploy/vps/vps.py load <image tar>      # stream into `docker load`

Requires `paramiko`.
"""

from __future__ import annotations

import sys
from pathlib import Path

import paramiko

ROOT = Path(__file__).resolve().parents[2]


def settings() -> dict[str, str]:
  values = {}
  for line in (ROOT / '.ssh.env').read_text(encoding='utf-8').splitlines():
    line = line.strip()
    if not line or line.startswith('#') or '=' not in line:
      continue
    key, value = line.split('=', 1)
    values[key.strip().removeprefix('export ').strip()] = value.strip().strip('"').strip("'")
  missing = {'HOST', 'USER', 'PASS'} - values.keys()
  if missing:
    sys.exit(f'.ssh.env is missing: {", ".join(sorted(missing))}')
  return values


def connect() -> paramiko.SSHClient:
  s = settings()
  host, _, port = s['HOST'].partition(':')
  client = paramiko.SSHClient()
  client.load_system_host_keys()
  # First contact: accept and remember the host key (the user named this host).
  client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
  client.connect(host, port=int(port or 22), username=s['USER'], password=s['PASS'],
                 look_for_keys=False, allow_agent=False, timeout=20)
  return client


def run(client: paramiko.SSHClient, command: str, stdin: bytes | None = None) -> int:
  # `sudo -S` reads the password from stdin when a command needs it.
  if command.startswith('sudo '):
    command = 'sudo -S -p "" ' + command[len('sudo '):]
    stdin = (settings()['PASS'] + '\n').encode() + (stdin or b'')
  chan = client.get_transport().open_session()
  chan.exec_command(command)
  if stdin is not None:
    chan.sendall(stdin)
    chan.shutdown_write()
  out = chan.makefile('rb')
  err = chan.makefile_stderr('rb')
  for line in out:
    sys.stdout.write(line.decode(errors='replace'))
  sys.stderr.write(err.read().decode(errors='replace'))
  return chan.recv_exit_status()


def put(client: paramiko.SSHClient, local: str, remote: str) -> None:
  with client.open_sftp() as sftp:
    sftp.put(local, remote)


def load(client: paramiko.SSHClient, tar: str) -> int:
  """Stream an image tarball into `docker load` (no temp file on the VPS)."""

  chan = client.get_transport().open_session()
  chan.exec_command('docker load')
  with open(tar, 'rb') as f:
    while chunk := f.read(1 << 20):
      chan.sendall(chunk)
  chan.shutdown_write()
  sys.stdout.write(chan.makefile('rb').read().decode(errors='replace'))
  sys.stderr.write(chan.makefile_stderr('rb').read().decode(errors='replace'))
  return chan.recv_exit_status()


def main() -> int:
  if len(sys.argv) < 3:
    sys.exit(__doc__)
  action, *args = sys.argv[1:]
  client = connect()
  try:
    if action == 'run':
      return run(client, ' '.join(args))
    if action == 'put':
      put(client, args[0], args[1])
      return 0
    if action == 'load':
      return load(client, args[0])
    sys.exit(f'unknown action {action!r}')
  finally:
    client.close()


if __name__ == '__main__':
  sys.exit(main())
