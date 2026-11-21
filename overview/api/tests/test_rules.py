"""The dashboard's rule matcher gives the same answers as shared/src/rules.rs."""

import json
from pathlib import Path

import pytest

from secretary_overview import rules

FIXTURE = json.loads((Path(__file__).parents[3] / 'shared/fixtures/rule-match-v1.json')
                     .read_text(encoding='utf-8'))


@pytest.mark.parametrize('case', FIXTURE['normalize'], ids=lambda c: f"{c['signal']}:{c['pattern']}")
def test_normalize(case):
  assert rules.normalize_pattern(case['signal'], case['pattern']) == case['expected']


@pytest.mark.parametrize('case', FIXTURE['match'], ids=lambda c: f"{c['signal']}:{c['pattern']}:{c['value']}")
def test_match(case):
  pattern = rules.normalize_pattern(case['signal'], case['pattern'])
  assert rules.matches_value(case['signal'], pattern, case['value']) is case['matches']


def test_evidence_and_resolution_match_the_rust_test():
  evidence = {
    'app': rules.evidence_values('app', ('Code.exe',)),
    'title': ['main.rs - api - Visual Studio Code'],
    'remote': rules.evidence_values('remote', ('github.com/acme/api', None)),
    'path': rules.evidence_values('path', (r'C:\work\acme\api', None, None, 'src/main.rs')),
  }
  assert evidence['path'] == [r'C:\work\acme\api', r'C:\work\acme\api/src/main.rs']

  def rule(id_, signal, pattern, priority=0):
    return {'id': id_, 'signal': signal, 'pattern': rules.normalize_pattern(signal, pattern),
            'priority': priority}

  all_rules = [rule(1, 'app', 'code'), rule(2, 'remote', 'github.com/acme'),
               rule(3, 'path', 'c:/work/acme/api/**'), rule(4, 'title', 'nothing here', 9)]
  assert rules.resolve(all_rules, evidence)['id'] == 3
  all_rules[0]['priority'] = 5
  assert rules.resolve(all_rules, evidence)['id'] == 1
  assert rules.resolve(all_rules[3:], evidence) is None
