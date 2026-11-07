"""AI (Phase 2 use; created now so tenancy is never retrofitted)."""

from __future__ import annotations

from secretary_orm import fields, models


class AiClassification(models.Model):
  _name = 'ai.classification'
  _table = 'ai_classifications'
  _track_refs = False
  _sql_constraints = [
    ('user_input_model_key', 'UNIQUE (user_id, input_hash, model_id)'),
    # AI never exceeds 0.8.
    ('confidence_range', 'CHECK (confidence >= 0 AND confidence <= 0.8)'),
  ]

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  input_hash = fields.StringField(name='Input Hash', size=64, required=True)  # cache key
  project_id = fields.Many2oneField('project', name='Project', ondelete='cascade')
  confidence = fields.FloatField(name='Confidence', required=True)
  model_id = fields.StringField(name='Model', size=128, required=True)


class AiRequestLog(models.Model):
  """What was sent to an AI provider, when, and why. Append-only; shown to the user."""

  _name = 'ai.request.log'
  _table = 'ai_request_log'
  _track_refs = False

  user_id = fields.Many2oneField('user', name='User', required=True, ondelete='cascade')
  provider = fields.StringField(name='Provider', size=64, required=True)
  model_id = fields.StringField(name='Model', size=128, required=True)
  purpose = fields.StringField(name='Purpose', size=32, required=True)  # 'classify', 'summarize', ...
  fields_sent = fields.JsonField(name='Fields Sent', required=True, default=[])  # e.g. ["domain", "title"]
  item_count = fields.IntegerField(name='Items', required=True, default=None)
