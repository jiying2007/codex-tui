#!/usr/bin/env python3
"""Analyze paired UX trials without granting live or release qualification.

Python 3.8+, standard library only. This tool never contacts a backend and never
outputs notes or trial identifiers. Exit 0: numeric targets met; 3: targets not
met; 2: invalid input. Even exit 0 is analysis-only, not verified field evidence.
"""
import argparse
import csv
import hashlib
import io
import json
import math
import re
import statistics
from pathlib import Path
from typing import Dict, List

FIELDS = (
    'trial_id', 'chain', 'scenario', 'terminal', 'session_state', 'duration_ms',
    'correct_target', 'accidental_mutation', 'notification_observed', 'notes',
)
SCENARIOS = {'needs-you', 'search', 'switch', 'review', 'notification', 'recovery',
             'cold-start', 'new-user', 'other'}
JUMP_SCENARIOS = {'needs-you', 'search', 'switch', 'review'}
MAX_BYTES = 5 * 1024 * 1024
MAX_ROWS = 20_000


class InvalidInput(ValueError):
    """Only stable reason codes are exposed, never untrusted cell contents."""


def require(condition: bool, code: str) -> None:
    if not condition:
        raise InvalidInput(code)


def boolean(value: str, code: str) -> bool:
    require(value in ('true', 'false'), code)
    return value == 'true'


def distribution(values: List[int]) -> Dict[str, float]:
    ordered = sorted(values)
    return {'p50Ms': statistics.median(ordered),
            'p95Ms': ordered[max(0, math.ceil(0.95 * len(ordered)) - 1)],
            'maxMs': ordered[-1]}


def parse(raw: bytes) -> List[dict]:
    require(0 < len(raw) <= MAX_BYTES, 'input-size-out-of-bound')
    try:
        text = raw.decode('utf-8-sig')
    except UnicodeDecodeError:
        raise InvalidInput('invalid-utf8') from None
    require('\x00' not in text, 'nul-in-csv')
    reader = csv.DictReader(io.StringIO(text, newline=''), strict=True)
    try:
        require(tuple(reader.fieldnames or ()) == FIELDS, 'csv-header-mismatch')
    except csv.Error:
        raise InvalidInput('invalid-csv-header') from None
    rows = []
    seen = set()
    try:
        for index, row in enumerate(reader, 2):
            prefix = 'row-%d:' % index
            require(len(rows) < MAX_ROWS, 'too-many-rows')
            require(None not in row and all(value is not None for value in row.values()),
                    prefix + 'column-count-mismatch')
            require(bool(re.fullmatch(r'T[0-9]{3,9}', row['trial_id'])), prefix + 'opaque-trial-id-required')
            require(row['chain'] in ('A', 'B'), prefix + 'invalid-chain')
            identity = (row['trial_id'], row['chain'])
            require(identity not in seen, prefix + 'duplicate-trial-chain')
            seen.add(identity)
            require(row['scenario'] in SCENARIOS, prefix + 'unknown-scenario')
            dimensions = re.fullmatch(r'([1-9][0-9]{0,2})x([1-9][0-9]{0,2})', row['terminal'])
            require(dimensions is not None, prefix + 'invalid-terminal')
            width, height = (int(value) for value in dimensions.groups())
            require(20 <= width <= 600 and 10 <= height <= 300, prefix + 'terminal-out-of-bound')
            require(row['session_state'] in ('cold', 'warm'), prefix + 'invalid-session-state')
            require(bool(re.fullmatch(r'[1-9][0-9]{0,8}', row['duration_ms'])), prefix + 'invalid-duration')
            row['duration_ms'] = int(row['duration_ms'])
            require(row['duration_ms'] <= 86_400_000, prefix + 'duration-out-of-bound')
            for field in ('correct_target', 'accidental_mutation'):
                row[field] = boolean(row[field], prefix + 'invalid-' + field)
            value = row['notification_observed']
            require(value in ('true', 'false', 'na'), prefix + 'invalid-notification')
            require(row['scenario'] == 'notification' or value == 'na',
                    prefix + 'notification-value-outside-notification-scenario')
            # Never keep notes, even in the in-memory representation used below.
            row.pop('notes')
            rows.append(row)
    except csv.Error:
        raise InvalidInput('invalid-csv') from None
    require(bool(rows), 'no-trials')
    return rows


def summarize(pairs: List[dict]) -> dict:
    chains = {}
    for chain in ('A', 'B'):
        values = [pair[chain] for pair in pairs]
        notifications = [r for r in values if r['scenario'] == 'notification']
        jumps = [r for r in values if r['scenario'] in JUMP_SCENARIOS]
        chains[chain] = {
            'attempts': len(values),
            'duration': distribution([r['duration_ms'] for r in values]),
            'incorrectTargets': sum(not r['correct_target'] for r in values),
            'accidentalMutations': sum(r['accidental_mutation'] for r in values),
            'jumpAttempts': len(jumps),
            'correctJumps': sum(r['correct_target'] for r in jumps),
            'notificationObserved': {v: sum(r['notification_observed'] == v for r in notifications)
                                     for v in ('true', 'false', 'na')},
        }
    reductions = [100 * (p['A']['duration_ms'] - p['B']['duration_ms']) / p['A']['duration_ms']
                  for p in pairs]
    return {
        'pairs': len(pairs), 'chains': chains,
        'pairedMedianReductionPct': statistics.median(reductions),
        'ratioOfMediansReductionPct': 100 * (1 - chains['B']['duration']['p50Ms'] /
                                           chains['A']['duration']['p50Ms']),
    }


def analyze(raw: bytes, source_sha: str, binary_sha256: str, data_kind: str = 'unverified') -> dict:
    require(bool(re.fullmatch(r'[0-9a-f]{40}', source_sha)), 'invalid-source-sha')
    require(bool(re.fullmatch(r'[0-9a-f]{64}', binary_sha256)), 'invalid-binary-sha256')
    require(data_kind in ('unverified', 'recorded', 'synthetic'), 'invalid-data-kind')
    rows = parse(raw)
    keyed = {}
    for row in rows:
        keyed.setdefault(row['trial_id'], {})[row['chain']] = row
    pairs = []
    for pair in keyed.values():
        require(set(pair) == {'A', 'B'}, 'unpaired-trial')
        require(all(pair['A'][field] == pair['B'][field]
                    for field in ('scenario', 'terminal', 'session_state')), 'pair-context-mismatch')
        pairs.append(pair)
    aggregate = summarize(pairs)
    groups = {}
    for pair in pairs:
        row = pair['A']
        key = (row['scenario'], row['terminal'], row['session_state'])
        groups.setdefault(key, []).append(pair)
    blockers = []
    if len(pairs) < 30:
        blockers.append('fewer-than-30-paired-tasks')
    if aggregate['pairedMedianReductionPct'] < 25:
        blockers.append('paired-median-reduction-below-25-percent')
    candidate = aggregate['chains']['B']
    if candidate['jumpAttempts'] < 100:
        blockers.append('fewer-than-100-candidate-jumps')
    if candidate['jumpAttempts'] and candidate['correctJumps'] / candidate['jumpAttempts'] < 0.99:
        blockers.append('candidate-jump-correctness-below-99-percent')
    if any(aggregate['chains'][chain]['accidentalMutations'] for chain in ('A', 'B')):
        blockers.append('accidental-mutation-observed')
    return {
        'schema': 'codex-tui/ux-analysis/v1',
        'authority': 'analysis-only-not-live-qualification',
        'dataKindDeclared': data_kind,
        'sourceShaDeclared': source_sha, 'binarySha256Declared': binary_sha256,
        'inputSha256': hashlib.sha256(raw).hexdigest(),
        'inputIdentityVerifiedAgainstBinary': False,
        'experimentDesignVerified': False, 'measurementAuthenticityVerified': False,
        'repositoryAndSessionCountsVerified': False,
        'method': 'paired median fractional reduction; nearest-rank p95; all failures retained',
        'aggregate': aggregate,
        'groups': [dict(scenario=k[0], terminal=k[1], sessionState=k[2], **summarize(v))
                   for k, v in sorted(groups.items())],
        'computedTargetScope': ['paired-reduction-and-sample-count', 'jump-count-and-correctness',
                                'zero-accidental-mutations'],
        'quantitativeTargetsMet': not blockers, 'quantitativeBlockers': blockers,
        'otherTargetsNotQualified': ['needs-you-within-5s', 'new-user-within-10min',
                                    'observed-notification-delivery', 'live-stability'],
        'liveQualification': 'NOT_VERIFIED',
        'stableReady': False, 'publicationAllowed': False,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('csv', type=Path)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--binary-sha256', required=True)
    parser.add_argument('--data-kind', choices=['unverified', 'recorded', 'synthetic'], default='unverified')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        # Read at most the limit plus one byte; a large source cannot allocate unbounded memory.
        with args.csv.open('rb') as handle:
            raw = handle.read(MAX_BYTES + 1)
        report = analyze(raw, args.source_sha, args.binary_sha256, args.data_kind)
        with args.output.open('x', encoding='utf-8', newline='\n') as handle:
            json.dump(report, handle, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False)
            handle.write('\n')
    except InvalidInput as error:
        print('Input refused: ' + str(error))  # Reason code only, never source cells.
        return 2
    except OSError:
        print('Input/output refused; check paths and use a new output filename.')
        return 2
    print('Analysis saved; this is not live qualification or release authorization.')
    return 0 if report['quantitativeTargetsMet'] else 3


if __name__ == '__main__':
    raise SystemExit(main())
