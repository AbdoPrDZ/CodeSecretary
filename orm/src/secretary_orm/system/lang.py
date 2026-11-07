"""``sys.lang`` / ``sys.multi_lang_char``: values for ``multi_language=True`` fields."""

from __future__ import annotations

from secretary_orm import fields, models
import secretary_orm.system.schema  # noqa: F401  (sys.field is a comodel)


class Language(models.Model):
  """Languages available for multi-language field values."""

  _name = 'sys.lang'
  _description = 'Languages available for multi-language values.'
  _label = 'name'

  name = fields.StringField(
    name='Name',
    required=True,
    help='Human-readable language name, for example English.',
  )
  code = fields.StringField(
    name='Code',
    size=10,
    required=True,
    unique=True,
    help='Language code, for example en, fr or ar.',
  )
  active = fields.BooleanField(
    name='Active',
    default=False,
    help='Inactive languages are ignored.',
  )
  is_default = fields.BooleanField(
    name='Default',
    help='Use this language as the default value of multi-language fields.',
  )


class MultiLangChar(models.Model):
  """Translation of one character field for one record and language."""

  _name = 'sys.multi_lang_char'
  _description = 'Translations of multi-language record fields.'
  _label = 'value'

  field_id = fields.Many2oneField(
    name='Field',
    comodel='sys.field',
    required=True,
    help='Model field metadata this translation belongs to.',
  )
  lang_id = fields.Many2oneField(
    name='Language',
    comodel='sys.lang',
    required=True,
    help='Language of this translation.',
  )
  res_id = fields.BigIntField(
    name='Record ID',
    required=True,
    help='ID of the record holding the translated field.',
  )
  value = fields.StringField(
    name='Value',
    required=True,
    help='Translated text.',
  )
  key = fields.StringField(
    name='Key',
    required=True,
    unique=True,
    help='Unique field/record/language identifier for this translation.',
  )
