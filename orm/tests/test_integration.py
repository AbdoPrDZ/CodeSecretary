"""End-to-end tests against a real PostgreSQL database.

Skipped unless ``ORM_TEST_DATABASE_URL`` points at a database the tests may
write to. Each run works in a fresh schema that is dropped afterwards.
"""

from __future__ import annotations

import os
import unittest
import uuid

URL = os.environ.get('ORM_TEST_DATABASE_URL')


def _snapshot_registry():
  """Model sources registered before a test class defines its own."""

  from secretary_orm.models import ModelRegistry

  return (
    dict(ModelRegistry._models),
    dict(ModelRegistry._new_models),
    {name: list(items) for name, items in ModelRegistry._extensions.items()},
  )


def _restore_registry(snapshot) -> None:
  """Put the global registry back (other test modules may have models in it)."""

  from secretary_orm.models import ModelRegistry

  models, new_models, extensions = snapshot
  ModelRegistry.clear()
  ModelRegistry._models.update(models)
  ModelRegistry._new_models.update(new_models)
  ModelRegistry._extensions.update(extensions)


@unittest.skipUnless(URL, 'set ORM_TEST_DATABASE_URL to run integration tests')
class SingleDatabaseTests(unittest.TestCase):
  @classmethod
  def setUpClass(cls) -> None:
    import psycopg

    from secretary_orm import Config, Environment, fields, models
    from secretary_orm.models import ModelRegistry
    from secretary_orm.system import drafts, lang, refs, schema

    cls._registry_snapshot = _snapshot_registry()
    # Register the sys.* models explicitly: their modules may already have been
    # imported (and their registration undone by another test's restore).
    for module in (schema, refs, lang, drafts):
      for obj in vars(module).values():
        if isinstance(obj, type) and obj.__module__ == module.__name__ and getattr(obj, '_name', None):
          ModelRegistry.register(obj)
    cls.schema = f'orm_test_{uuid.uuid4().hex[:8]}'
    with psycopg.connect(URL, autocommit=True) as conn:
      conn.execute(f'CREATE SCHEMA "{cls.schema}"')

    class Partner(models.Model):
      _name = 'test.partner'
      _label = 'name'

      name = fields.StringField(name='Name', required=True)
      email = fields.StringField(name='Email')

    class PartnerExtension(models.Model):
      _inherit = 'test.partner'

      phone = fields.StringField(name='Phone')

    class Order(models.Model):
      _name = 'test.order'

      reference = fields.StringField(name='Reference', required=True)
      partner_id = fields.Many2oneField('test.partner', name='Partner', required=True)
      amount = fields.FloatField(name='Amount', default=0.0)

    # The search_path makes the pool work inside the throwaway schema.
    dsn = f'{URL}{"&" if "?" in URL else "?"}options=-csearch_path%3D{cls.schema}'
    cls.env = Environment(Config(dsn=dsn, schema=cls.schema))

  @classmethod
  def tearDownClass(cls) -> None:
    import psycopg

    from secretary_orm import Environment

    Environment.reset()
    _restore_registry(cls._registry_snapshot)
    with psycopg.connect(URL, autocommit=True) as conn:
      conn.execute(f'DROP SCHEMA "{cls.schema}" CASCADE')

  def test_schema_is_created_on_first_access(self) -> None:
    Partner = self.env['test.partner']
    tables = set(self.env.db.list_tables())
    self.assertTrue({'test_partner', 'test_order', 'sys_field', 'sys_data'} <= tables)
    self.assertIn('phone', self.env.db.get_columns(Partner._table))

  def test_crud_relations_and_extension(self) -> None:
    Partner = self.env['test.partner']
    Order = self.env['test.order']

    partner = Partner.create({'name': 'Ada', 'email': 'ada@example.com', 'phone': '1'})
    order = Order.create({'reference': 'SO1', 'partner_id': partner.id, 'amount': 12.5})

    found = Order.search([('reference', '=', 'SO1')])
    self.assertEqual(len(found), 1)
    self.assertEqual(found[0].partner_id.name, 'Ada')
    self.assertEqual(found[0].amount, 12.5)

    self.assertTrue(Partner.browse(partner.id)[0].write({'phone': '2'}))
    self.assertEqual(Partner.browse(partner.id)[0].phone, '2')

    with self.assertRaises(ValueError):
      partner.unlink()  # still referenced by the order
    self.assertTrue(order.unlink())
    self.assertTrue(partner.unlink())
    self.assertEqual(Partner.search([('id', '=', partner.id)]), [])

  def test_transaction_rolls_back(self) -> None:
    Partner = self.env['test.partner']
    with self.assertRaises(RuntimeError):
      with self.env.db.transaction():
        Partner.create({'name': 'Rolled back'})
        raise RuntimeError('boom')
    self.assertEqual(Partner.search([('name', '=', 'Rolled back')]), [])

  def test_external_refs(self) -> None:
    Partner = self.env['test.partner']
    from secretary_orm.refs import model_module

    partner = Partner.create({'name': 'Grace', '_key': 'grace'})
    # Short keys expand to <owner>.<model>.<key>; the owner is the defining package.
    key = f'{model_module(Partner)}.test.partner.grace'
    self.assertEqual(self.env.ref(key).id, partner.id)
    with self.assertRaises(ValueError):
      self.env.ref('missing.key')


@unittest.skipUnless(URL, 'set ORM_TEST_DATABASE_URL to run integration tests')
class ConstraintTests(unittest.TestCase):
  """Foreign keys, selection checks, ``_sql_constraints`` and the new field types."""

  @classmethod
  def setUpClass(cls) -> None:
    import psycopg

    from secretary_orm import Config, Environment, fields, models

    cls._registry_snapshot = _snapshot_registry()
    cls.schema = f'orm_test_{uuid.uuid4().hex[:8]}'
    with psycopg.connect(URL, autocommit=True) as conn:
      conn.execute(f'CREATE SCHEMA "{cls.schema}"')

    class Owner(models.Model):
      _name = 'c.owner'

      name = fields.StringField(name='Name', required=True)

    class Item(models.Model):
      _name = 'c.item'
      _sql_constraints = [
        ('owner_code_key', 'UNIQUE (owner_id, code)'),
        ('qty_positive', 'CHECK (qty > 0)'),
      ]

      owner_id = fields.Many2oneField('c.owner', name='Owner', required=True, ondelete='cascade')
      parent_id = fields.Many2oneField('c.item', name='Parent', ondelete='set null')
      code = fields.StringField(name='Code', required=True)
      qty = fields.IntegerField(name='Qty', default=1)
      state = fields.SelectionField([('a', 'A'), ('b', 'B')], name='State')
      ref = fields.UuidField(name='Ref')
      at = fields.TimeField(name='At')
      small = fields.SmallIntField(name='Small')

    dsn = f'{URL}{"&" if "?" in URL else "?"}options=-csearch_path%3D{cls.schema}'
    cls.env = Environment(Config(dsn=dsn, schema=cls.schema), auto_sync=False)
    cls.env.registry().init_models()

  @classmethod
  def tearDownClass(cls) -> None:
    import psycopg

    from secretary_orm import Environment

    Environment.reset()
    _restore_registry(cls._registry_snapshot)
    with psycopg.connect(URL, autocommit=True) as conn:
      conn.execute(f'DROP SCHEMA "{cls.schema}" CASCADE')

  def _constraints(self, table):
    rows = self.env.db.fetchall(
      "SELECT conname FROM pg_constraint c JOIN pg_class t ON t.oid = c.conrelid "
      "JOIN pg_namespace n ON n.oid = t.relnamespace WHERE n.nspname = %s AND t.relname = %s",
      [self.schema, table],
    )
    return {row['conname'] for row in rows}

  def test_constraints_are_created(self) -> None:
    self.assertTrue({
      'c_item_owner_id_fk', 'c_item_parent_id_fk', 'c_item_state_sel',
      'c_item_owner_code_key', 'c_item_qty_positive',
    } <= self._constraints('c_item'))

  def test_resync_is_a_no_op(self) -> None:
    self.env.registry().init_models()
    self.assertFalse(self.env.registry().last_constraint_plan.changed)

  def test_database_enforces_constraints(self) -> None:
    import datetime

    import psycopg

    Owner, Item = self.env['c.owner'], self.env['c.item']
    owner = Owner.create({'name': 'o'})
    parent = Item.create({'owner_id': owner.id, 'code': 'p', 'state': 'a'})
    child = Item.create({
      'owner_id': owner.id, 'code': 'c', 'parent_id': parent.id,
      'ref': str(uuid.uuid4()), 'at': datetime.time(9, 30), 'small': 3,
    })
    self.assertIsInstance(Item.browse(child.id)[0].ref, uuid.UUID)
    # An unset many2one is stored as NULL (not 0), so foreign keys accept it.
    self.assertIsNone(self.env.db.fetchone('SELECT parent_id FROM c_item WHERE id = %s', [parent.id])['parent_id'])

    for bad in ({'code': 'p'}, {'code': 'x', 'qty': 0}, {'code': 'y', 'state': 'zzz'}):
      with self.assertRaises(psycopg.errors.IntegrityError):
        with self.env.db.transaction():
          Item.create({'owner_id': owner.id, **bad})

    # ON DELETE in the database (bypassing the ORM's own reference check).
    self.env.db.execute('DELETE FROM c_item WHERE id = %s', [parent.id])
    self.assertEqual(self.env.db.fetchone('SELECT parent_id FROM c_item WHERE id = %s', [child.id])['parent_id'], None)
    self.env.db.execute('DELETE FROM c_owner WHERE id = %s', [owner.id])
    self.assertEqual(Item.search([('owner_id', '=', owner.id)]), [])

  def test_changed_and_removed_constraints_are_replaced(self) -> None:
    Item = self.env.registry().get('c.item')
    source = Item._sources[0]
    original = source.__dict__['_sql_constraints']
    try:
      source._sql_constraints = [('owner_code_key', 'UNIQUE (owner_id, code, qty)')]
      self.env.registry().load()
      self.env.registry().init_models()
      plan = self.env.registry().last_constraint_plan
      self.assertIn('c_item_owner_code_key', plan.replaced)
      self.assertIn('c_item_qty_positive', plan.dropped)
    finally:
      source._sql_constraints = original
      self.env.registry().load()
      self.env.registry().init_models()
    self.assertIn('c_item_qty_positive', self._constraints('c_item'))
