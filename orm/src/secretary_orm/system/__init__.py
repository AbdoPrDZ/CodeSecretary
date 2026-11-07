"""Optional system models.

Import a submodule to enable one feature (for example
``import secretary_orm.system.schema``), or ``import secretary_orm.system.all``
for all of them. Importing the package itself registers nothing.

| Model | Enables |
|---|---|
| ``schema``: ``sys.model`` / ``sys.field`` | Deterministic rename detection and retired-column tracking in the schema syncer |
| ``refs``: ``sys.data`` | External ids: ``create(_key=...)``, ``Model.register_ref`` and ``Environment.ref`` |
| ``lang``: ``sys.lang`` / ``sys.multi_lang_char`` | ``multi_language=True`` fields |
| ``drafts``: ``temp.model`` | Server-side draft records (``Model.new`` / drafts API) |

Without them these features are inactive and only the application's own tables
are created.
"""
