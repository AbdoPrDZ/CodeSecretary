"""Project mapping rules (D17) for the dashboard: pattern normalization and
matching, a copy of ``shared/src/rules.rs`` for the match preview.

The engine applies rules from the Rust crate; this module must give the same
answers. Both are tested against ``shared/fixtures/rule-match-v1.json``.
"""

from __future__ import annotations

SIGNALS = ('path', 'remote', 'domain', 'app', 'title')
MAX_PATTERN = 500


def normalize_pattern(signal: str, pattern: str) -> str | None:
  p = pattern.strip()
  if not p or len(p) > MAX_PATTERN:
    return None
  if signal == 'path':
    p = p.replace('\\', '/')
    out = p.rstrip('/') or '/'
  elif signal == 'remote':
    out = _normalize_remote(p)
  elif signal == 'domain':
    host = p.split('://', 1)[1] if '://' in p else p
    for sep in '/?#':
      host = host.split(sep, 1)[0]
    host = host.rsplit('@', 1)[-1].split(':', 1)[0]
    out = _strip_prefix(host, '*.').rstrip('.').lower()
  elif signal in ('app', 'title'):
    out = p
  else:
    return None
  return out or None


def _normalize_remote(p: str) -> str:
  if '://' in p:
    rest = p.split('://', 1)[1]
    authority, _, path = rest.partition('/')
    host = authority.rsplit('@', 1)[-1].split(':', 1)[0]
    joined = f'{host}/{path}'
  else:
    user_host, colon, path = p.partition(':')
    if colon and '/' not in user_host:
      joined = f'{user_host.rsplit("@", 1)[-1]}/{path}'
    else:
      joined = p
  trimmed = joined.lower().strip('/')
  if trimmed.endswith('.git'):
    trimmed = trimmed[:-4]
  return trimmed.rstrip('/')


def _strip_prefix(text: str, prefix: str) -> str:
  while text.startswith(prefix):
    text = text[len(prefix):]
  return text


def matches_value(signal: str, pattern: str, value: str) -> bool:
  if signal in ('path', 'remote'):
    return _path_matches(_normalize_path(pattern), _normalize_path(value))
  if signal == 'domain':
    return _domain_matches(pattern, value)
  if signal == 'app':
    app = _app_name(value)
    return bool(app) and app == _app_name(pattern)
  if signal == 'title':
    pattern, title = pattern.lower(), value.lower()
    if '*' in pattern or '?' in pattern:
      return _glob_one(pattern, title)
    return pattern in title
  return False


# -- copies of the exclusion matchers in shared/src/config.rs ----------------

def _normalize_path(path: str) -> str:
  p = path.strip().replace('\\', '/').lower()
  while len(p) > 1 and p.endswith('/'):
    p = p[:-1]
  return p


def _app_name(app: str) -> str:
  name = app.strip().replace('\\', '/').rsplit('/', 1)[-1].lower()
  return name[:-4] if name.endswith('.exe') else name


def _domain_matches(pattern: str, domain: str) -> bool:
  domain = domain.strip().rstrip('.').lower()
  d = _strip_prefix(pattern.strip(), '*.').rstrip('.').lower()
  return bool(d) and (domain == d or (domain.endswith(d) and domain[:-len(d)].endswith('.')))


def _path_matches(pattern: str, path: str) -> bool:
  if not pattern:
    return False
  if '*' not in pattern and '?' not in pattern:
    if path == pattern:
      return True
    return path.startswith(pattern) and (path[len(pattern):].startswith('/') or pattern.endswith('/'))
  pat = pattern.split('/')
  segs = path.split('/')
  return any(_glob_segments(pat, segs[:n]) for n in range(1, len(segs) + 1))


def _glob_segments(pattern: list[str], path: list[str]) -> bool:
  if not pattern:
    return not path
  first, rest = pattern[0], pattern[1:]
  if first == '**':
    return any(_glob_segments(rest, path[skip:]) for skip in range(len(path) + 1))
  return bool(path) and _glob_one(first, path[0]) and _glob_segments(rest, path[1:])


def _glob_one(pattern: str, text: str) -> bool:
  p, t = pattern, text
  pi = ti = mark = 0
  star = None
  while ti < len(t):
    if pi < len(p) and (p[pi] == '?' or p[pi] == t[ti]):
      pi += 1
      ti += 1
    elif pi < len(p) and p[pi] == '*':
      star, mark = pi, ti
      pi += 1
    elif star is not None:
      pi = star + 1
      mark += 1
      ti = mark
    else:
      return False
  return all(c == '*' for c in p[pi:])


# -- evidence of stored events ------------------------------------------------

#: SQL expressions per signal, over ``activity_events``; the preview groups by
#: them. Mirrors ``Evidence::from_event`` (editor file paths are relative to
#: the repository root).
EVIDENCE_COLUMNS = {
  'path': ("repository_root", "working_directory", "metadata->>'workspace_path'",
           "metadata->>'file_path'"),
  'remote': ("repository_remote", "metadata->>'remote_url'"),
  'domain': ("metadata->>'domain'",),
  'app': ("metadata->>'app'",),
  'title': ("metadata->>'title'",),
}


def evidence_values(signal: str, columns: tuple) -> list[str]:
  """The values of one grouped row (in ``EVIDENCE_COLUMNS`` order)."""

  if signal == 'path':
    root, cwd, workspace, file_path = columns
    values = [v for v in (root, cwd, workspace) if v]
    if file_path:
      values.append(f"{root.rstrip('/' + chr(92))}/{file_path}" if root else file_path)
    return values
  return [v for v in columns if v]


def resolve(rules: list[dict], evidence: dict[str, list[str]]) -> dict | None:
  """The winning rule: highest priority, then longest pattern, then oldest."""

  hits = [r for r in rules
          if any(matches_value(r['signal'], r['pattern'], v) for v in evidence.get(r['signal'], ()))]
  if not hits:
    return None
  return min(hits, key=lambda r: (-r['priority'], -len(r['pattern']), r['id']))
