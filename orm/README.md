# secretary-orm

A declarative PostgreSQL ORM for **one database**, taken from the J-ERP framework (`jerp/`). The models, fields, recordsets, `_inherit` / `_inherits` composition and the safe schema syncer are unchanged. Removed: J-ERP's multi-database manager (create/drop/duplicate databases, one registry per database), the module system, seeds, menus, the HTTP layer and the UI.

**Status:** used by the warehouse, which defines every CodeSecretary table with it (D15). Additions over J-ERP: `UuidField`, `TimeField`, `SmallIntField`, real foreign keys (`Many2oneField(ondelete=...)`), selection `CHECK`s and `_sql_constraints` ([constraints.py](src/secretary_orm/constraints.py)); unset many2one/number fields with `default=None` store NULL; auto timestamps get `DEFAULT now()`.

## Install

```sh
pip install -e orm            # Python ≥ 3.13 (tested on 3.13); pulls psycopg 3 + psycopg-pool
```

## Use

```python
from secretary_orm import Config, Environment, fields, models


class Partner(models.Model):
  _name = 'res.partner'
  _label = 'name'

  name = fields.StringField(name='Name', required=True)
  email = fields.StringField(name='Email')


class Order(models.Model):
  _name = 'sale.order'

  partner_id = fields.Many2oneField('res.partner', name='Customer', required=True)
  amount = fields.FloatField(name='Amount', default=0.0)


env = Environment(Config.from_env())     # or Config(dsn='postgresql://...')
Partner = env['res.partner']             # first access builds models and syncs tables
ada = Partner.create({'name': 'Ada'})
env['sale.order'].create({'partner_id': ada.id, 'amount': 12.5})

with env.db.transaction():               # ORM calls inside share one transaction
  for order in env['sale.order'].search([('amount', '>', 10)], order='id'):
    order.write({'amount': order.amount * 2})
```

- **Configuration:** `Config.from_env()` reads `ORM_DATABASE_URL`, or `ORM_DB_HOST` / `PORT` / `USER` / `PASSWORD` / `NAME` / `SSLMODE`, plus `ORM_DB_SCHEMA` and `ORM_DB_POOL_MIN` / `ORM_DB_POOL_MAX`. See [config.py](src/secretary_orm/config.py).
- **Schema sync:** by default the first model access creates tables and adds columns and indexes. Changes that could lose data (type changes, drops) are only logged. To apply them explicitly:
  - `env.registry().preview()` returns the planned changes; `format_plan()` in `schema.py` renders them.
  - `env.registry().init_models(allow_drop=...)` applies them.
  - Pass `Environment(config, auto_sync=False)` when another tool owns the schema.
- **Adding models later:** after importing new model classes, call `env.registry().rebuild()`.
- **Optional system models:** `import secretary_orm.system.all` registers `sys.model` / `sys.field` (rename detection and retired columns), `sys.data` (external ids: `create(_key=...)`, `env.ref()`), `sys.lang` / `sys.multi_lang_char` (`multi_language=True` fields) and `temp.model` (draft records). Import one submodule (`system.schema`, `system.refs`, `system.lang`, `system.drafts`) to enable only that feature. Without them, those features are off.
- **Translations:** labels and error messages go through `secretary_orm.i18n.translate`; install a catalog with `set_translator(func)`.

## Tests

```sh
python -m unittest discover -s orm/tests                  # unit tests (no database)
ORM_TEST_DATABASE_URL=postgresql://secretary:secretary@localhost:5432/secretary \
  python -m unittest discover -s orm/tests                # + integration (fresh schema, dropped after)
```

Run from the repo root with `PYTHONPATH=orm/src`, or after `pip install -e orm`.
