#!/usr/bin/env python3
"""Apply explicitly proposed frontend p95 budgets to an existing JSON report.
Not a terminal SLO, benchmark execution, upstream CI gate or release authority.
Python 3.8+; no dependencies, backend access or network calls.
"""
import argparse
import hashlib
import json
import math
import re
from pathlib import Path

# Review proposals only: the repository has not adopted these as production gates.
BUDGETS_MS = {
    'registry-input-and-render-10k': 25,
    'board-navigation-and-render-10k': 25,
    'thread-revision-and-render-10k': 25,
    'planning-ui-snapshot-10k': 5,
    'planning-ui-commit-10k': 25,
    'list-navigation-and-render-10k': 25,
    'review-queue-navigation-and-render-10k': 25,
    'planning-ui-rebase-10k': 50,
    'planning-ui-stale-retry-50k': 50,
}
COVERAGE = set(BUDGETS_MS) | {'planning-worker-compute-10k', 'headless-local-cards-10k'}
TIMINGS = ('p50Ms', 'p95Ms', 'p99Ms', 'maxMs')
MAX_BYTES = 1024 * 1024


def require(condition, code):
    if not condition:
        raise ValueError(code)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate-json-key')
        result[key] = value
    return result


def evaluate(raw, source_sha):
    require(0 < len(raw) <= MAX_BYTES, 'size-out-of-bound')
    require(bool(re.fullmatch(r'[0-9a-f]{40}', source_sha)), 'invalid-source-sha')
    report = json.loads(raw, object_pairs_hook=unique_object)
    require(isinstance(report, dict), 'report-not-object')
    require(report.get('schema') == 'codex-tui/interaction-performance/v1', 'schema-mismatch')
    require(report.get('sourceSha') == source_sha, 'source-mismatch')
    require(report.get('sampleQualified') is True, 'samples-not-qualified')
    for field, minimum in [('warmupIterations', 20), ('iterations', 200)]:
        require(type(report.get(field)) is int and report[field] >= minimum, 'insufficient-' + field)
    for field, expected in [('viewportWidth', 160), ('viewportHeight', 40), ('rows', 10000),
                            ('modelInferenceCalls', 0)]:
        require(type(report.get(field)) is int and report[field] == expected, 'invalid-' + field)
    require(report.get('authority') == 'synthetic-cpu-path-diagnostic-not-terminal-slo', 'authority-mismatch')
    for field in ('humanTimeSavings', 'modelTokenSavings'):
        require(field in report and report[field] is None, 'unsupported-savings-claim')
    require(isinstance(report.get('source'), str) and bool(report['source'].strip()), 'source-label-missing')
    fixtures = report.get('fixtures')
    require(isinstance(fixtures, list) and len(fixtures) == len(COVERAGE), 'fixture-count-mismatch')
    require(all(isinstance(f, dict) and isinstance(f.get('fixture'), str) for f in fixtures), 'invalid-fixture')
    require({f['fixture'] for f in fixtures} == COVERAGE, 'fixture-coverage-mismatch')
    result = []
    for fixture in fixtures:
        name = fixture['fixture']
        rows = 50000 if name.endswith('50k') else 10000
        require(type(fixture.get('rows')) is int and fixture['rows'] == rows, 'fixture-row-mismatch')
        values = [fixture.get(field) for field in TIMINGS]
        require(all(type(v) in (float, int) and 0 <= v <= 86_400_000 and math.isfinite(v) for v in values), 'invalid-timing')
        require(values == sorted(values), 'unordered-timing')
        if name in BUDGETS_MS:
            budget = BUDGETS_MS[name]
            result.append({'fixture': name, 'observedP95Ms': fixture['p95Ms'],
                           'proposedBudgetMs': budget, 'withinProposedBudget': fixture['p95Ms'] <= budget})
    return {
        'schema': 'codex-tui/frontend-budget-analysis/v1',
        'sourceSha': source_sha, 'inputSha256': hashlib.sha256(raw).hexdigest(),
        'authority': 'proposed-budget-analysis-not-adopted-repository-gate',
        'benchmarkExecutedByThisTool': False,
        'results': result, 'withinAllProposedBudgets': all(r['withinProposedBudget'] for r in result),
        'terminalSloVerified': False, 'stableReady': False, 'publicationAllowed': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        with args.report.open('rb') as handle:
            raw = handle.read(MAX_BYTES + 1)
        result = evaluate(raw, args.source_sha)
        with args.output.open('x', encoding='utf-8', newline='\n') as handle:
            json.dump(result, handle, sort_keys=True, indent=2, allow_nan=False)
            handle.write('\n')
    except (OSError, ValueError, TypeError, KeyError):
        print('Evidence refused; check input identity, structure and unique output path.')
        return 2
    print('Proposed budget analysis saved; not a new benchmark or terminal qualification.')
    return 0 if result['withinAllProposedBudgets'] else 3


if __name__ == '__main__':
    raise SystemExit(main())
