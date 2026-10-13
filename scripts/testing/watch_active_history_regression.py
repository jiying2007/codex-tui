#!/usr/bin/env python3
"""History-active owners must not disappear with their cached presentation pages."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import tempfile

from app_server_transport_regression import require
from watch_cache_regression import Fleet, MOCK, WORKERS, identity, sample, validate_counts

CASES = ('completed-history-control', 'active-history-completion', 'active-history-failure')


def fixture(active):
    # This is deliberately independent from Registry activity. The existing
    # single-thread history-complete regression has the same metadata premise.
    old = "'active' if name.startswith('fleet-') else 'idle'"
    require(MOCK.count(old) == 1, 'Registry fixture contract changed')
    mock = MOCK.replace(old, "'idle'")
    old = "'status':'completed'"
    require(mock.count(old) == 1, 'turn fixture contract changed')
    if active:
        mock = mock.replace(old, "'status':'inProgress' if not (home/'complete').exists() else 'completed'")
    old = "  elif m=='thread/read' and ident in ('ghost','anchor')"
    require(mock.count(old) == 1, 'failure fixture contract changed')
    injected = """  elif m=='thread/read' and ident=='fleet-000' and (home/'fail-first').exists():
   log.write(json.dumps({'seq':seq,'method':'fixture/read-rejected','params':p})+'\\n')
   emit({'id':r['id'],'error':{'code':-32000,'message':'active fixture read rejected'}});continue
"""
    return mock.replace(old, injected+old)


def case(binary, output, home, mode):
    fleet = Fleet(binary, output, home, fixture(mode != CASES[0]))
    try:
        fleet.app.wait(lambda: '33 total' in fleet.app.screen(), 'idle Registry fixture missing')
        for ident in WORKERS:
            fleet.choose(ident)
            fleet.app.send(b'\x1b')
        # First prove every history was actually loaded before checking ownership.
        rows = fleet.records()
        validate_counts(rows, 0, WORKERS, 1)
        validate_counts(rows, 0, WORKERS, 1, 'thread/items/list')
        fleet.clear_filter()
        phase = fleet.boundary([{'method':'item/completed','params':{'threadId':x}} for x in WORKERS])
        expected = 0 if mode == CASES[0] else 1
        validate_counts(fleet.records(), phase, WORKERS, expected)
        validate_counts(fleet.records(), phase, WORKERS, expected, 'thread/items/list')
        if mode == CASES[0]:
            return {'mode':mode,'passed':True,'sessions':32,'noIdleReads':True,
                    'resources':sample(fleet.app.proc.pid),'terminal':fleet.app.finish()}

        if mode == 'active-history-failure':
            (home/'fail-first').touch()
            phase = fleet.boundary([{'method':'item/completed','params':{'threadId':x}} for x in WORKERS])
            rows = fleet.records()
            validate_counts(rows, phase, WORKERS, 1)
            validate_counts(rows, phase, [WORKERS[0]], 1, 'fixture/read-rejected')
            validate_counts(rows, phase, [WORKERS[0]], 0, 'thread/items/list')
            validate_counts(rows, phase, WORKERS[1:], 1, 'thread/items/list')
            # UI events are delivered on their ordinary channel, independently
            # from the actor snapshot channel. Drain before a fresh event batch.
            fleet.app.drain(.1)
            phase = fleet.boundary([{'method':'item/completed','params':{'threadId':x}} for x in WORKERS])
            validate_counts(fleet.records(), phase, [WORKERS[0]], 0)
            validate_counts(fleet.records(), phase, WORKERS[1:], 1)
            remaining = WORKERS[1:]
        else:
            # A live provisional owner has no Registry row at all. Its history
            # must survive navigation and the same 32-page presentation churn.
            fleet.open_target(True)
            # Keep the provisional return owner while exposing Registry rows,
            # since a never-registered Thread has no visible Registry fence.
            fleet.app.send(b'/')
            fleet.app.send(b'\x7f'*len(fleet.filter))
            # Exact search loads item pages, not current turns. First observe a
            # real full-history refresh while navigation still owns this target.
            phase = fleet.boundary([{'method':'item/completed','params':{'threadId':'ghost'}}])
            rows = fleet.records()
            validate_counts(rows, phase, ['ghost'], 1)
            validate_counts(rows, phase, ['ghost'], 1, 'thread/turns/list')
            validate_counts(rows, phase, ['ghost'], 1, 'thread/items/list')
            fleet.app.drain(.1)
            fleet.app.send(b'\x1b')  # Return to the still-owned Thread.
            fleet.app.send(b'\x1b')  # Leave only after current turns were observed.
            # Read the exact old hit again: its missing turns must not erase the
            # just-observed active owner when the user leaves that surface.
            fleet.open_target(True)
            fleet.app.send(b'\x1b')
            fleet.refresh(WORKERS)
            phase = fleet.boundary([{'method':'item/completed','params':{'threadId':'ghost'}}])
            validate_counts(fleet.records(), phase, ['ghost'], 1)
            remaining = WORKERS + ('ghost',)
        (home/'complete').touch()
        phase = fleet.boundary([{'method':'item/completed','params':{'threadId':x}} for x in remaining])
        validate_counts(fleet.records(), phase, remaining, 1)
        fleet.app.drain(.1)
        phase = fleet.boundary([{'method':'item/completed','params':{'threadId':x}}
                                for x in WORKERS + ('ghost',)])
        validate_counts(fleet.records(), phase, WORKERS + ('ghost',), 0)
        return {'mode':mode,'passed':True,'sessions':32,'activeOwnersSurvivedEviction':True,
                'historySettlementReleasesOwners':True,'provisionalOwnerExercised':mode==CASES[1],
                'resources':sample(fleet.app.proc.pid),'terminal':fleet.app.finish()}
    finally:
        fleet.close()


def run_cases(binary, output):
    checks = []
    for mode in CASES:
        out = output/mode; out.mkdir(parents=True, exist_ok=False)
        try:
            with tempfile.TemporaryDirectory(prefix='ctui-history-owner-') as d:
                result = case(binary, out, Path(d), mode)
        except Exception as error:
            result = {'mode':mode,'passed':False,'error':str(error)}
        checks.append(result)
        print(json.dumps(result), flush=True)
    return checks


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary',type=Path); parser.add_argument('output',type=Path)
    args = parser.parse_args()
    require(sys.platform=='linux', 'Linux PTY and /proc required')
    args.output.mkdir(parents=True,exist_ok=False)
    binary = args.binary.resolve(strict=True)
    checks = run_cases(binary,args.output)
    result = {'schema':'codex-tui/active-history-cache-regression/v1',
              'sourceSha':identity(binary,args.output),
              'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
              'realCodexQualification':False,'realSshQualification':False,'checks':checks,
              'status':'passed' if all(x['passed'] for x in checks) else 'failed'}
    (args.output/'receipt.json').write_text(json.dumps(result,indent=2)+'\n')
    return 0 if result['status']=='passed' else 1


if __name__=='__main__': sys.exit(main())
