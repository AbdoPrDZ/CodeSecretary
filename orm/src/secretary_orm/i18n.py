"""Translation hook for labels and error messages.

The ORM itself ships no catalogs: every lookup returns the default text unless
the application installs a translator with :func:`set_translator`.
"""

from __future__ import annotations

from typing import Callable

#: ``(key, default) -> str | None``; ``None`` falls back to the default.
_translator: Callable[[str, str | None], str | None] | None = None


def set_translator(func: Callable[[str, str | None], str | None] | None) -> None:
  global _translator
  _translator = func


def translate(key: str, default: str | None = None) -> str:
  if _translator is not None:
    value = _translator(key, default)
    if value:
      return value
  return default if default is not None else key


def _t(key: str, default: str | None = None, **params) -> str:
  """Translate a message, with ``{name}`` interpolation."""

  value = translate(key, default)
  for name, replacement in params.items():
    value = value.replace(f'{{{name}}}', str(replacement))
  return value
