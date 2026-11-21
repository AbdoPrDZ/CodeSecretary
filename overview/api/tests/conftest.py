"""Reuse the warehouse's database fixtures (fresh migrated database per test,
assertions as an RLS-bound app role)."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

_path = Path(__file__).resolve().parents[3] / 'warehouse' / 'tests' / 'conftest.py'
_spec = importlib.util.spec_from_file_location('warehouse_conftest', _path)
_module = importlib.util.module_from_spec(_spec)
sys.modules['warehouse_conftest'] = _module  # dataclasses need the module registered
_spec.loader.exec_module(_module)

test_db = _module.test_db
app_env = _module.app_env
