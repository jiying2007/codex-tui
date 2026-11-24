#!/usr/bin/env python3
"""Actual Linux TUI subscription release under bounded command-queue pressure."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time

from app_server_transport_regression import require
from runtime_fault_regression import Session, environment
from watch_ownership_regression import MOCK, identity, resources

CASES = ('healthy-close', 'pressure-close', 'pressure-retain-active',
         'pressure-keep', 'pressure-reopen', 'pressure-disconnect')

# Reuse the maintained wire contract; only the held history read is new.
PRESSURE_MOCK = MOCK.replace("    if m=='turn/start':", """    if m=='thread/read' and (home/'block').exists() and not (home/'release').exists():
        (home/'waiting').touch()
        deadline = time.monotonic()+4.5
        while not (home/'release').exists() and time.monotonic()<deadline: time.sleep(.005)
        if (home/'disconnect').exists(): sys.exit(0)
    if m=='turn/start':""")


def check_counts(records, seq, expected):
    counts = {method: sum(r['seq'] == seq and r['method'] == method and
                         r['params'].get('threadId') == 'watch-a' for r in records)
              for method in ('thread/read', 'thread/queue/list')}
    require(list(counts.values()) == list(expected),
            'phase {} expected history/queue {}, observed {}'.format(seq, expected, counts))
    return counts


def case(binary, output, home, mode):
    log = home/'server.jsonl'
    active = mode in ('pressure-retain-active', 'pressure-keep')
    env = environment(home, log, 'registry-complete' if active else 'idle-back')
    (home/'bin/codex').write_text(PRESSURE_MOCK, encoding='utf-8')
    app = Session(binary, output, home, env)
    observations = []

    def records():
        require(log.stat().st_size < 512*1024, 'fixture trace exceeded 512 KiB')
        # A buffered final record may not yet include its newline.
        text = log.read_text()
        return [json.loads(line) for line in text[:text.rfind('\n')+1].splitlines()]

    def settled():
        until = time.monotonic()+4
        last = None
        quiet_since = time.monotonic()
        while time.monotonic() < until:
            size = log.stat().st_size
            if size != last:
                last, quiet_since = size, time.monotonic()
            elif time.monotonic()-quiet_since >= .25:
                return
            app.drain(.05)
        raise RuntimeError('accepted fixture reads did not settle')

    def boundary(seq, expected, events=True, idle=False):
        notifications = [
            {'method':'item/completed', 'params':{'threadId':'watch-a'}},
            {'method':'thread/queue/changed', 'params':{'threadId':'watch-a'}}
        ] if events else []
        if idle:
            notifications.insert(0, {'method':'thread/status/changed',
                                     'params':{'threadId':'watch-a', 'status':{'type':'idle'}}})
        value = {'seq':seq, 'events':notifications}
        tmp = home/'control.tmp'
        tmp.write_text(json.dumps(value)); tmp.replace(home/'control.json')
        app.wait(lambda: 'ACK_{}_'.format(seq) in app.screen(), 'ordered actor fence missing')
        app.drain(.05)
        counts = check_counts(records(), seq, expected) if expected is not None else {}
        observations.append({'seq':seq, 'reads':counts, 'resources':resources(app.proc.pid)})

    try:
        app.wait(lambda: '2 total' in app.screen(), 'Registry did not load')
        app.send(b'\r')
        app.wait(lambda: 'BODY_0' in app.screen(), 'initial history missing')
        pressure = mode != 'healthy-close'
        if pressure:
            app.send(b'\x1b')
            (home/'block').touch()
            app.send(b'\r')
            app.wait(lambda: (home/'waiting').exists(), 'the real history RPC was not held')
        if pressure:
            # Repeated navigation admits actual history loads, not Queue RPCs.
            # The fixture's history turn is completed, including in the active
            # registry-runtime case, so Ctrl+C cannot send an interrupt.
            app.send(b'\x03\r' * 100)
            app.wait(lambda: 'queue is full' in app.screen(), 'bounded admission refusal not observed')
            require(not any(r['method'] == 'turn/interrupt' for r in records()),
                    'pressure sent an unexpected interrupt')
            (output/'pressure-screen.txt').write_text(app.screen(), encoding='utf-8')
        if mode != 'pressure-keep':
            app.send(b'\x03' if pressure else b'\x1b')
            app.wait(lambda: 'Mission Control' in app.screen(), 'did not leave history')
        if mode == 'pressure-disconnect':
            (home/'disconnect').touch()
        (home/'release').touch()
        if mode == 'pressure-disconnect':
            app.wait(lambda: 'disconnected' in app.screen().lower() or
                     'closed' in app.screen().lower(), 'transport closure not visible')
            before = len(records()); app.drain(.3)
            require(len(records()) == before, 'disconnected backend continued requests')
            result = {'admissionRefused':True, 'disconnected':True,
                      'resources':resources(app.proc.pid)}
        else:
            boundary(1, None, events=False)
            settled()
            expected = (1, 0) if active else (0, 0)
            boundary(2, expected)
            boundary(3, expected)
            if mode == 'pressure-retain-active':
                # Background runtime owns history after leaving the view.
                # Settlement must release it without reading retired Queue.
                boundary(4, (0, 0), events=False, idle=True)
                boundary(5, (0, 0))
            elif mode == 'pressure-keep':
                boundary(4, (1, 0), idle=True)
                app.send(b'\x1b')
                boundary(5, (0, 0))
            elif mode == 'pressure-reopen':
                app.send(b'\r')
                app.wait(lambda: 'BODY_3' in app.screen(), 'reopening did not reload history')
                boundary(4, (1, 0))
                app.send(b'\x1b')
                boundary(5, (0, 0))
            result = {'admissionRefused':pressure, 'observations':observations}
        require(not any(r['method'].startswith('thread/queue/') for r in records()),
                'retired queue RPC observed')
        result['terminal'] = app.finish()
        return result
    finally:
        try:
            (output/'observations.json').write_text(json.dumps(observations, indent=2)+'\n')
            if log.exists(): (output/'server.jsonl').write_bytes(log.read_bytes())
        finally:
            app.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path); parser.add_argument('output', type=Path)
    parser.add_argument('--scenario', choices=CASES)
    args = parser.parse_args()
    require(sys.platform == 'linux', 'Linux PTY and /proc required')
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema':'codex-tui/watch-pressure-regression/v1', 'status':'failed',
               'sourceSha':identity(binary, args.output),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'realCodexQualification':False, 'realSshQualification':False,
               'resourceBudgetScope':'isolated two-thread fixture; not a global memory ceiling',
               'checks':[]}
    for mode in [args.scenario] if args.scenario else CASES:
        try:
            with tempfile.TemporaryDirectory(prefix='ctui-watch-pressure-') as directory:
                item = dict(case(binary, args.output/mode, Path(directory), mode), mode=mode, passed=True)
        except Exception as error:
            item = {'mode':mode, 'passed':False, 'error':str(error)}
        receipt['checks'].append(item); print(json.dumps(item), flush=True)
    if all(row['passed'] for row in receipt['checks']): receipt['status'] = 'passed'
    (args.output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')
    return 0 if receipt['status'] == 'passed' else 1


if __name__ == '__main__': sys.exit(main())
