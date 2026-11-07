"""Database access layer.

``Database`` wraps a pooled PostgreSQL connection and exposes the small helpers
used by models and the schema syncer. The ORM works with exactly one database.
"""

from __future__ import annotations

from contextlib import contextmanager
from contextvars import ContextVar

from psycopg.rows import dict_row
from psycopg_pool import ConnectionPool

from secretary_orm.config import Config
from secretary_orm.dialect import PostgresDialect

#: Connection bound to the current transaction, shared by Database helpers and
#: model CRUD so a block of ORM calls commits atomically.
_active_transaction: ContextVar = ContextVar('orm_active_transaction', default=None)


class Database:
  """The PostgreSQL database plus its connection pool."""

  def __init__(self, config: Config):
    self.config = config
    self.schema = config.schema
    self.dialect = PostgresDialect(config.schema)
    self._pool: ConnectionPool | None = None

  # --- lifecycle ----------------------------------------------------------

  def start(self) -> None:
    if self._pool is None:
      self._pool = ConnectionPool(
        conninfo=self.config.dsn,
        min_size=self.config.pool_min,
        max_size=self.config.pool_max,
        open=False,
        timeout=self.config.pool_timeout,
        kwargs={'row_factory': dict_row},
      )
      self._pool.open(wait=True)

  def close(self) -> None:
    if self._pool is not None:
      self._pool.close()
      self._pool = None

  @contextmanager
  def connection(self):
    current = _active_transaction.get()
    if current is not None and current[0] is self:
      yield current[1]
      return
    self.start()
    with self._pool.connection() as conn:
      yield conn

  @contextmanager
  def transaction(self):
    """Run a block inside a single transaction (committed on success).

    Nested ``transaction()`` calls reuse the active connection, so model CRUD
    inside the block shares the transaction.
    """

    current = _active_transaction.get()
    if current is not None and current[0] is self:
      yield current[1]
      return

    self.start()
    with self._pool.connection() as conn:
      with conn.transaction():
        token = _active_transaction.set((self, conn))
        try:
          yield conn
        finally:
          _active_transaction.reset(token)

  # --- query helpers ------------------------------------------------------

  def execute(self, sql: str, params=None) -> int:
    """Execute a statement and return the affected row count."""

    with self.connection() as conn:
      with conn.cursor() as cur:
        cur.execute(sql, params)
        return cur.rowcount

  def fetchall(self, sql: str, params=None) -> list[dict]:
    with self.connection() as conn:
      with conn.cursor() as cur:
        cur.execute(sql, params)
        return cur.fetchall() or []

  def fetchone(self, sql: str, params=None) -> dict | None:
    with self.connection() as conn:
      with conn.cursor() as cur:
        cur.execute(sql, params)
        return cur.fetchone()

  # --- introspection ------------------------------------------------------

  def table_exists(self, table: str) -> bool:
    row = self.fetchone(
      "SELECT 1 FROM information_schema.tables "
      "WHERE table_schema = %s AND table_name = %s AND table_type = 'BASE TABLE'",
      [self.schema, table],
    )
    return row is not None

  def list_tables(self) -> list[str]:
    rows = self.fetchall(self.dialect.tables_query(), [self.schema])
    return [r['table_name'] for r in rows]

  def get_columns(self, table: str) -> dict:
    rows = self.fetchall(self.dialect.columns_query(), [self.schema, table])
    return {r['column_name']: r for r in rows}

  def get_primary_key(self, table: str) -> list[str]:
    rows = self.fetchall(self.dialect.primary_key_query(), [self.schema, table])
    return [r['column_name'] for r in rows]

  def get_indexes(self, table: str) -> dict:
    rows = self.fetchall(self.dialect.indexes_query(), [self.schema, table])
    return {r['indexname']: r['indexdef'] for r in rows}

  def drop_table(self, table: str) -> None:
    self.execute(self.dialect.drop_table_sql(table))
