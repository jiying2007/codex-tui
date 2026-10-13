#!/usr/bin/env python3
"""Linux actual-program subscription/RPC probes with isolated protocol fixtures."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile

from runtime_fault_regression import Session, environment
from app_server_transport_regression import require

MOCK = r'''#!/usr/bin/python3
import json, os, sys, threading, time
from pathlib import Path
home = Path(os.environ['HOME'])
lock = threading.RLock()
log = open(os.environ['MOCK_LOG'], 'w', buffering=1)
mode = os.environ['MOCK_MODE']
seq = 0
submitted = False
turn_status = 'inProgress' if mode == 'history-complete' else 'completed'
rows = {name: {'id':name, 'name':'READY_'+name, 'cwd':str(home),
        'updatedAt':2000000000-(0 if name=='watch-a' else 1),
        'status':{'type':'active' if name=='watch-a' and mode in ('registry-complete','deleted') else 'idle'}}
        for name in ('watch-a', 'watch-b')}
def emit(value):
    with lock:
        print(json.dumps(value, separators=(',',':')), flush=True)
def control():
    global seq, turn_status
    while True:
        path = home/'control.json'
        if path.exists():
            value = json.loads(path.read_text())
            if value['seq'] > seq:
                with lock:
                    seq = value['seq']
                    turn_status = value.get('turnStatus', turn_status)
                    for event in value.get('events', []):
                        ident = event['params'].get('threadId')
                        if event['method']=='thread/deleted': rows.pop(ident, None)
                        if event['method']=='thread/status/changed' and ident in rows:
                            rows[ident]['status'] = event['params']['status']
                        emit(event)
                    for ident, row in rows.items():
                        row['name'] = 'ACK_%d_%s' % (seq, ident)
                        emit({'method':'thread/name/updated','params':{'threadId':ident,'threadName':row['name']}})
        time.sleep(.005)
threading.Thread(target=control, daemon=True).start()
for line in sys.stdin:
    r = json.loads(line)
    if 'id' not in r: continue
    m, p = r.get('method'), r.get('params', {})
    with lock: log.write(json.dumps({'seq':seq, 'method':m, 'params':p})+'\n')
    if m=='turn/start':
        deadline = time.monotonic()+4
        while not (home/'release-prompt').exists() and time.monotonic()<deadline:
            time.sleep(.005)
        with lock:
            submitted = True
            if mode=='prompt-failure':
                emit({'id':r['id'],'error':{'code':-32000,'message':'fixture submission failed'}})
            else:
                turn_status = 'inProgress'
                emit({'id':r['id'],'result':{'turn':{'id':'turn-1'}}})
        continue
    if m=='thread/read' and submitted and mode=='prompt-history-failure':
        emit({'id':r['id'],'error':{'code':-32000,'message':'fixture history failed'}})
        continue
    with lock:
        ident = p.get('threadId')
        if m=='initialize': out={'serverInfo':{'name':'watch-fixture','version':'0.0.0'}}
        elif m=='thread/list': out={'data':list(rows.values()),'nextCursor':None}
        elif m=='thread/loaded/list': out={'data':list(rows),'nextCursor':None}
        elif m=='thread/read': out={'thread':dict(rows.get(ident, {'id':ident}), name='History '+str(seq))}
        elif m=='thread/turns/list': out={'data':[{'id':'turn-1','status':turn_status}],'nextCursor':None}
        elif m=='thread/items/list': out={'data':[{'turnId':'turn-1','item':{'id':'item-1','type':'agentMessage','text':'BODY_'+str(seq)}}],'nextCursor':None}
        else:
            emit({'id':r['id'],'error':{'code':-32601,'message':'unsupported fixture'}})
            continue
        emit({'id':r['id'],'result':out})
'''

CASES = ['idle-back', 'search-cancel', 'search-commit', 'history-complete',
         'registry-complete', 'deleted', 'board-cancel', 'board-commit',
         'prompt-success', 'prompt-failure', 'prompt-history-failure']


def check_reads(actual, expected, label):
    require(actual == expected, '{}: expected {} historical read(s), observed {}'.format(label, expected, actual))


def resources(pid):
    values = {}
    for line in Path('/proc/{}/status'.format(pid)).read_text().splitlines():
        key, _, value = line.partition(':')
        if key in ('VmRSS', 'VmHWM', 'Threads'):
            values[key] = int(value.split()[0])
    values['fdCount'] = len(list(Path('/proc/{}/fd'.format(pid)).iterdir()))
    require(values.get('VmHWM', 0) > 0, 'missing live process RSS observation')
    require(values['VmHWM'] < 128*1024, 'two-thread fixture exceeded 128 MiB diagnostic budget')
    require(values['fdCount'] < 64, 'two-thread fixture exceeded 64 descriptors')
    return values


def case(binary, output, home, mode):
    log = home/'server.jsonl'
    env = environment(home, log, mode)
    (home/'bin/codex').write_text(MOCK, encoding='utf-8')
    app = Session(binary, output, home, env)
    observations = []
    def records():
        return [json.loads(line) for line in log.read_text().splitlines()]
    def boundary(seq, expected, events=None, **kwargs):
        if events is None:
            events = [{'method':'item/completed','params':{'threadId':'watch-a'}}]
        value = dict(kwargs, seq=seq, events=events)
        temporary = home/'control.tmp'
        temporary.write_text(json.dumps(value))
        temporary.replace(home/'control.json')
        # A later registry notification proves the actor consumed the earlier
        # boundary and completed any history RPC it initiated before the fence.
        app.wait(lambda:'ACK_{}_'.format(seq) in app.screen(), 'actor notification fence missing')
        reads = sum(r['seq']==seq and r['method']=='thread/read' and
                    r['params'].get('threadId')=='watch-a' for r in records())
        observation = dict(seq=seq, historyReads=reads, expectedReads=expected,
                           resources=resources(app.proc.pid))
        observations.append(observation)
        if expected is not None:
            check_reads(reads, expected, mode+' phase '+str(seq))
    try:
        app.wait(lambda:'2 total' in app.screen(), 'Registry did not load')
        app.send(b'\r')
        app.wait(lambda:'BODY_0' in app.screen(), 'initial history did not render')
        if mode.startswith('prompt-'):
            app.send(b'a')
            app.send(b'fixture prompt\r')
            app.wait(lambda:any(r['method']=='turn/start' for r in records()),
                     'production prompt submission was not exercised')
            app.send(b'\x1b')
            (home/'release-prompt').touch()
            # The command is still in flight when Back executes. The following
            # fence runs after its ack/failure and any authoritative history.
            boundary(1, None, [])  # Observe completion of the accepted command.
            boundary(2, 1 if mode=='prompt-success' else 0)
            if mode=='prompt-success':
                boundary(3, 1, turnStatus='completed')
                boundary(4, 0)
        elif mode.startswith('board-'):
            app.send(b'\x0b')
            app.send(b'board\r')
            app.wait(lambda:'Ready (2)' in app.screen(), 'Board did not open through palette')
            app.send(b'/')
            if mode=='board-cancel':
                app.send(b'\x1b')
                boundary(1, 1)
                app.send(b'\x1b')
                app.wait(lambda:'BODY_1' in app.screen(), 'Board return lost live history')
                app.send(b'\x1b')
                boundary(2, 0)
            else:
                app.send(b'\r')
                boundary(1, 0)
        elif mode.startswith('search-'):
            app.send(b'/')
            if mode=='search-cancel':
                app.send(b'\x1b')
                boundary(1, 1)
                app.wait(lambda:'BODY_1' in app.screen(), 'restored thread missed live update')
                app.send(b'\x1b')
                boundary(2, 0)
            else:
                app.send(b'\r')
                boundary(1, 0)
        else:
            app.send(b'\x1b')
            app.wait(lambda:'Mission Control' in app.screen(), 'Back did not return to registry')
            if mode=='history-complete':
                boundary(1, 1, turnStatus='completed')
                boundary(2, 0)
            elif mode in ('registry-complete','deleted'):
                boundary(1, 1)  # Active work must remain subscribed.
                event = {'method':'thread/deleted' if mode=='deleted' else 'thread/status/changed',
                         'params':{'threadId':'watch-a','status':{'type':'idle'}}}
                boundary(2, 0, [event])
                boundary(3, 0)
            else:
                boundary(1, 0)
        return {'observations':observations, 'terminal':app.finish()}
    finally:
        (output/'observations.json').write_text(json.dumps(observations, indent=2)+'\n')
        if log.exists(): (output/'server.jsonl').write_bytes(log.read_bytes())
        app.close()


def check_identity(report, expected):
    require(expected == 'unknown' or bool(re.fullmatch(r'[0-9a-f]{40}', expected or '')),
            'exact source SHA required (unknown permitted only for unqualified local builds)')
    require(report.get('sourceSha') == expected, 'executable source identity mismatch')


def identity(binary, output):
    with tempfile.TemporaryDirectory(prefix='codex-tui-watch-identity-') as directory:
        home = Path(directory)
        env = environment(home, home/'probe.log', 'identity')
        result = subprocess.run([str(binary), 'doctor', 'compat', '--json'], env=env, cwd=home,
                                stdin=subprocess.DEVNULL, capture_output=True, timeout=20)
        require(result.returncode in (0, 3), 'binary identity probe failed')
        report = json.loads(result.stdout)
        check_identity(report, os.environ.get('CODEX_TUI_PTY_SOURCE_SHA'))
        (output/'identity.json').write_text(json.dumps(report, indent=2)+'\n')
        return report['sourceSha']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--scenario', choices=CASES)
    args = parser.parse_args()
    require(sys.platform=='linux', 'Linux controlling PTY required')
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema':'codex-tui/watch-ownership-regression/v1', 'status':'failed',
               'sourceSha':os.environ.get('CODEX_TUI_PTY_SOURCE_SHA'),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'manualQualification':False, 'realCodexQualification':False,
               'realSshQualification':False, 'checks':[]}
    receipt['sourceSha'] = identity(binary, args.output)
    receipt['sourceIdentityVerified'] = receipt['sourceSha'] != 'unknown'
    receipt['resourceBudgetScope'] = 'two-thread isolated fixture only, not a global RSS limit'
    for mode in [args.scenario] if args.scenario else CASES:
        try:
            with tempfile.TemporaryDirectory(prefix='codex-tui-watch-') as directory:
                result = case(binary, args.output/mode, Path(directory), mode)
                item = dict(result, mode=mode, passed=True)
        except Exception as error:
            item = {'mode':mode, 'passed':False, 'error':str(error)}
        receipt['checks'].append(item)
        print(json.dumps(item), flush=True)
    if all(item['passed'] for item in receipt['checks']): receipt['status']='passed'
    (args.output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')
    return 0 if receipt['status']=='passed' else 1


if __name__=='__main__':
    sys.exit(main())
