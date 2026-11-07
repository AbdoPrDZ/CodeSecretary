"""Table constraints derived from model declarations.

The column/index syncer (:mod:`secretary_orm.schema`) never creates
constraints. This module adds them, idempotently, after the columns exist:

* **Foreign keys** for every :class:`~secretary_orm.fields.Many2oneField` that
  declares ``ondelete`` -> ``<table>_<column>_fk``.
* **Selection checks** for every
  :class:`~secretary_orm.fields.SelectionField` -> ``<table>_<column>_sel``.
* **Model constraints** from ``_sql_constraints = [(name, definition), ...]``
  (Odoo style, e.g. ``('event_key', 'UNIQUE (event_id, timestamp_start)')``)
  -> ``<table>_<name>``.

Each managed constraint carries a comment ``orm:<hash of its definition>``. A
constraint whose definition changed is dropped and recreated; a managed
constraint that is no longer declared is dropped. Constraints without that
comment (written by hand in SQL) are never touched.

Adding a constraint validates existing rows; if they violate it, the statement
fails and, inside a transaction, nothing is applied.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass, field as dc_field

from secretary_orm import fields as field_types
from secretary_orm.dialect import quote_ident, quote_table

#: PostgreSQL identifiers are limited to 63 bytes.
MAX_IDENTIFIER = 63

COMMENT_PREFIX = 'orm:'


@dataclass
class ConstraintPlan:
  created: list = dc_field(default_factory=list)
  replaced: list = dc_field(default_factory=list)
  dropped: list = dc_field(default_factory=list)

  @property
  def changed(self) -> bool:
    return bool(self.created or self.replaced or self.dropped)


def _literal(value) -> str:
  return "'" + str(value).replace("'", "''") + "'"


def _name(table: str, suffix: str) -> str:
  name = f'{table}_{suffix}'
  if len(name.encode('utf-8')) > MAX_IDENTIFIER:
    digest = hashlib.sha1(name.encode('utf-8')).hexdigest()[:8]
    name = f'{name[:MAX_IDENTIFIER - 9]}_{digest}'
  return name


def declared_constraints(model, models: dict) -> dict[str, str]:
  """``constraint name -> definition`` for one built model."""

  table = model._table
  declared: dict[str, str] = {}

  for field in model._fields.values():
    column = quote_ident(field.column_name)

    if isinstance(field, field_types.Many2oneField) and field.ondelete:
      target = models.get(field.comodel)
      if target is None or getattr(target, '_virtual', False):
        raise ValueError(
          f"Model '{model._name}': field '{field.attr_name}' references unknown "
          f"model '{field.comodel}', so its foreign key cannot be created."
        )
      declared[_name(table, f'{field.column_name}_fk')] = (
        f'FOREIGN KEY ({column}) REFERENCES {quote_table(target._table)} '
        f'({quote_ident(target._fields["id"].column_name)}) '
        f'ON DELETE {field.ondelete.upper()}'
      )

    if isinstance(field, field_types.SelectionField) and field.selection:
      values = ', '.join(_literal(value) for value, _label in field.selection)
      declared[_name(table, f'{field.column_name}_sel')] = f'CHECK ({column} IN ({values}))'

  for entry in getattr(model, '_sql_constraints', ()) or ():
    name, definition = entry[0], entry[1]
    declared[_name(table, name)] = definition

  return declared


def _digest(definition: str) -> str:
  return COMMENT_PREFIX + hashlib.sha256(definition.encode('utf-8')).hexdigest()[:16]


def _existing(cur, schema: str, table: str) -> dict[str, str | None]:
  """Constraint name -> comment for ``table``."""

  cur.execute(
    'SELECT c.conname, obj_description(c.oid, %s) AS comment '
    'FROM pg_constraint c '
    'JOIN pg_class t ON t.oid = c.conrelid '
    'JOIN pg_namespace n ON n.oid = t.relnamespace '
    'WHERE n.nspname = %s AND t.relname = %s',
    ['pg_constraint', schema, table],
  )
  return {row['conname']: row['comment'] for row in cur.fetchall()}


def sync_constraints(db, models: dict, only=None) -> ConstraintPlan:
  """Create, replace or drop managed constraints.

  ``models`` maps model names to built classes (all of them, so foreign keys
  can resolve their targets). ``only`` limits the sync to those model names.
  Runs on the caller's transaction when there is one.
  """

  plan = ConstraintPlan()
  selected = [
    model for name, model in models.items()
    if (only is None or name in only) and not getattr(model, '_virtual', False)
  ]

  drops: list[tuple[str, str]] = []
  creates: list[tuple[str, str, str, bool]] = []  # table, name, definition, is_fk

  with db.connection() as conn:
    with conn.transaction():
      with conn.cursor() as cur:
        for model in selected:
          table = model._table
          declared = declared_constraints(model, models)
          existing = _existing(cur, db.schema, table)

          for name, comment in existing.items():
            managed = bool(comment and comment.startswith(COMMENT_PREFIX))
            if managed and name not in declared:
              drops.append((table, name))
              plan.dropped.append(name)

          for name, definition in declared.items():
            comment = existing.get(name, False)
            if comment == _digest(definition):
              continue
            if comment is not False:
              if not (comment and comment.startswith(COMMENT_PREFIX)):
                raise ValueError(
                  f"Constraint '{name}' on '{table}' exists but is not managed by "
                  f"the ORM; rename the declaration or drop the constraint."
                )
              drops.append((table, name))
              plan.replaced.append(name)
            else:
              plan.created.append(name)
            is_fk = definition.lstrip().upper().startswith('FOREIGN KEY')
            creates.append((table, name, definition, is_fk))

        # Drops first; then unique/check constraints, which composite foreign
        # keys may reference; then foreign keys.
        for table, name in drops:
          cur.execute(
            f'ALTER TABLE {quote_table(table)} DROP CONSTRAINT IF EXISTS {quote_ident(name)}'
          )
        for table, name, definition, _is_fk in sorted(creates, key=lambda item: item[3]):
          cur.execute(
            f'ALTER TABLE {quote_table(table)} '
            f'ADD CONSTRAINT {quote_ident(name)} {definition}'
          )
          cur.execute(
            f'COMMENT ON CONSTRAINT {quote_ident(name)} ON {quote_table(table)} '
            f'IS {_literal(_digest(definition))}'
          )

  return plan
