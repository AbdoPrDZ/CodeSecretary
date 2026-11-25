"""Generate the engine golden inputs: ``fixtures/<case>/context.json``.

Run: ``python engine/fixtures/gen_fixtures.py``. The expected outputs
(``expected.json``) are written by the golden test with ``SECRETARY_BLESS=1``
and must be reviewed by hand before committing (see engine/tests/golden.rs).

Each case is a ``Context`` (engine/src/model.rs): one local day, its time
zone, events in the wire format plus ``device_id`` (timestamps are the
server-adjusted ones), mapping rules and overrides.
"""

from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).parent
_seq = 0


def eid() -> str:
  global _seq
  _seq += 1
  return f'019a0000-0000-7000-8000-{_seq:012d}'


def ev(kind: str, start: str, end: str | None = None, *, device: int = 1, root=None, remote=None,
       **metadata) -> dict:
  return {
    'device_id': device,
    'event': {
      'event_id': eid(), 'schema_version': 1,
      'timestamp_start': start, 'timestamp_end': end if end is not None else (None if kind in POINTS else start),
      'source': 'agent@0.1.0', 'event_type': kind, 'project_hint': None,
      'repository': None if root is None and remote is None else {'root_path': root, 'remote_url': remote},
      'working_directory': None, 'metadata': metadata,
    },
  }


POINTS = {'editor.save', 'editor.workspace_open', 'git.commit', 'git.checkout', 'git.merge'}


def presence(start, end, device=1):
  return ev('presence.active', start, end, device=device)


def focus(start, end, app, title, device=1):
  return ev('window.focus', start, end, device=device, app=app, title=title)


def file_focus(start, end, root, path='src/main.rs', device=1):
  return ev('editor.file_focus', start, end, device=device, root=root, file_path=path, language='rust')


def rule(id_, project, signal, pattern, priority=0, shared=False):
  return {'id': id_, 'project_id': project, 'signal': signal, 'pattern': pattern,
          'priority': priority, 'shared': shared}


def t(day: str, hm: str) -> str:
  return f'{day}T{hm}:00Z' if hm.count(':') == 1 else f'{day}T{hm}Z'


D = '2026-11-20'
API, WEB = '/home/dev/api', '/home/dev/web'

CASES = {}

# Path rule on the editor; a browser window between two editor stretches of
# one repository inherits it (temporal); a chat window after it does not.
CASES['rule-and-temporal'] = {
  'local_day': D, 'time_zone': 'UTC',
  'rules': [rule(1, 1, 'path', API)],
  'events': [
    presence(t(D, '09:00'), t(D, '09:30')),
    presence(t(D, '09:32'), t(D, '10:00')),  # 2 min without input: still present
    focus(t(D, '09:00'), t(D, '09:20'), 'Code.exe', 'main.rs - api - Visual Studio Code'),
    file_focus(t(D, '09:00'), t(D, '09:20'), API),
    focus(t(D, '09:20'), t(D, '09:24'), 'chrome.exe', 'Rust docs'),
    focus(t(D, '09:24'), t(D, '09:50'), 'Code.exe', 'main.rs - api - Visual Studio Code'),
    file_focus(t(D, '09:24'), t(D, '09:50'), API),
    ev('git.commit', t(D, '09:49'), root=API, hash='a' * 40, branch='main'),
    focus(t(D, '09:50'), t(D, '09:58'), 'slack.exe', 'general'),
  ],
}

# Same project after an 8 min absence is a new session (the gap had no
# presence); a project switch splits; same project again after 6 present
# minutes merges; a 30 s blip is dropped (after merging, blueprint §11.2).
CASES['merge-gap-and-min-length'] = {
  'local_day': D, 'time_zone': 'UTC',
  'rules': [rule(1, 1, 'path', API), rule(2, 2, 'path', WEB)],
  'events': [
    presence(t(D, '10:00'), t(D, '10:20')),
    file_focus(t(D, '10:00'), t(D, '10:20'), API),
    presence(t(D, '10:28'), t(D, '11:00')),
    file_focus(t(D, '10:28'), t(D, '10:40'), API),
    file_focus(t(D, '10:40'), t(D, '10:45'), WEB),
    file_focus(t(D, '10:45'), t(D, '10:46'), API),
    file_focus(t(D, '10:52'), t(D, '10:55'), API),
    file_focus(t(D, '10:57:00'), t(D, '10:57:30'), WEB),
  ],
}

# Europe/Paris (UTC+1 in November): the local day is 23:00Z..23:00Z and each
# day only owns its own part of work that crosses midnight.
CASES['midnight-time-zone'] = {
  'local_day': D, 'time_zone': 'Europe/Paris',
  'rules': [rule(1, 1, 'path', API)],
  'events': [
    presence(t('2026-11-19', '22:30'), t('2026-11-19', '23:30')),
    file_focus(t('2026-11-19', '22:30'), t('2026-11-19', '23:30'), API),
    presence(t(D, '22:40'), t(D, '23:20')),
    file_focus(t(D, '22:40'), t(D, '23:20'), API),
  ],
}

# Personal rules beat shared ones; a user override wins over both, and an
# override to "not project work" credits nothing. Time outside presence
# never counts.
_not_work = ev('window.focus', t(D, '14:30'), t(D, '14:40'), app='figma.exe', title='Holiday card')
_moved = ev('window.focus', t(D, '14:40'), t(D, '14:50'), app='figma.exe', title='Landing page')
CASES['overrides-and-shared-rules'] = {
  'local_day': D, 'time_zone': 'UTC',
  'rules': [rule(10, 3, 'app', 'figma', shared=True), rule(11, 4, 'title', 'landing')],
  'overrides': {_not_work['event']['event_id']: None, _moved['event']['event_id']: 5},
  'events': [
    presence(t(D, '14:00'), t(D, '15:00')),
    ev('window.focus', t(D, '14:00'), t(D, '14:15'), app='figma.exe', title='Brand'),        # shared -> 3
    ev('window.focus', t(D, '14:15'), t(D, '14:30'), app='figma.exe', title='Landing v2'),   # personal -> 4
    _not_work,
    _moved,
    ev('window.focus', t(D, '15:00'), t(D, '15:20'), app='figma.exe', title='Brand'),        # idle: dropped
  ],
}

# Two devices: the laptop's editor and the desktop's terminal overlap. The
# focused window outranks the editor while both are decided.
CASES['two-devices'] = {
  'local_day': D, 'time_zone': 'UTC',
  'rules': [rule(1, 1, 'title', 'ops'), rule(2, 2, 'path', WEB)],
  'events': [
    presence(t(D, '09:00'), t(D, '09:30'), device=1),
    file_focus(t(D, '09:00'), t(D, '09:30'), WEB, device=1),
    presence(t(D, '09:20'), t(D, '10:00'), device=2),
    focus(t(D, '09:20'), t(D, '10:00'), 'WindowsTerminal.exe', 'ops: deploy', device=2),
  ],
}

# Commits and saves credit 2 min before them; a long unmatched window with no
# decided neighbour stays unassigned and is reported as such.
CASES['point-credit-and-unassigned'] = {
  'local_day': D, 'time_zone': 'UTC',
  'rules': [rule(1, 1, 'remote', 'github.com/acme/api')],
  'events': [
    presence(t(D, '16:00'), t(D, '17:00')),
    ev('git.commit', t(D, '16:10'), remote='github.com/acme/api', hash='b' * 40),
    ev('editor.save', t(D, '16:11'), remote='github.com/acme/api', root=API, file_path='src/lib.rs'),
    focus(t(D, '16:20'), t(D, '16:50'), 'notepad.exe', 'notes.txt'),
  ],
}


def adj(id_, kind, start, end, project=None, from_project=None):
  return {'id': id_, 'kind': kind, 'start': start, 'end': end, 'project_id': project,
          'from_project_id': from_project}


# User corrections (EN-10), applied in id order: a split (reassign part of a
# session), a trim (remove part, only from project 2), a remove whose
# project filter matches nothing, unassigned time moved to a project, and
# manual time (one entry crossing midnight is cut to this day).
CASES['adjustments'] = {
  'local_day': D, 'time_zone': 'UTC',
  'rules': [rule(1, 1, 'path', API), rule(2, 2, 'path', WEB)],
  'adjustments': [
    adj(1, 'reassign', t(D, '09:30'), t(D, '09:45'), project=3),
    adj(2, 'remove', t(D, '10:15'), t(D, '10:30'), from_project=2),
    adj(3, 'remove', t(D, '10:00'), t(D, '11:30'), from_project=1),
    adj(4, 'reassign', t(D, '11:00'), t(D, '11:30'), project=1),
    adj(5, 'add', t(D, '13:00'), t(D, '14:00'), project=4),
    adj(6, 'add', t(D, '23:30'), t('2026-11-21', '00:30'), project=4),
  ],
  'events': [
    presence(t(D, '09:00'), t(D, '11:30')),
    file_focus(t(D, '09:00'), t(D, '10:00'), API),
    file_focus(t(D, '10:00'), t(D, '11:00'), WEB),
    focus(t(D, '11:00'), t(D, '11:30'), 'notepad.exe', 'notes.txt'),
  ],
}


def main() -> None:
  for name, case in CASES.items():
    folder = HERE / name
    folder.mkdir(exist_ok=True)
    (folder / 'context.json').write_text(json.dumps(case, indent=2) + '\n', encoding='utf-8', newline='\n')
    print('wrote', folder / 'context.json')


if __name__ == '__main__':
  main()
