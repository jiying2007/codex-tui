#!/usr/bin/env python3
"""Actual-program cache eviction across cancellation and closed admission; Linux only."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from app_server_transport_regression import require
from watch_cache_regression import Fleet, MOCK, WORKERS, identity, sample, validate_counts

CASES = ('registered-search-cancel', 'provisional-board-cancel',
         'registered-board-disconnect', 'provisional-search-disconnect',
         'registered-board-refused', 'provisional-board-refused')


def held_mock(target):
    old = "  if m=='initialize':"
    require(MOCK.count(old)==1, 'mock dispatch contract changed')
    addition = """  if m=='thread/read' and ident==%r and (home/'hold').exists() and not (home/'release').exists():
   (home/'held').touch()
   until=time.monotonic()+4
   while not (home/'release').exists() and time.monotonic()<until:time.sleep(.005)
   if (home/'disconnect').exists():sys.exit(0)
""" % target
    mock = MOCK.replace(old, addition+old)
    return mock.replace("  path=home/'control.json'",
                        "  if (home/'shutdown').exists():os._exit(0)\n  path=home/'control.json'")


def case(binary, output, home, scenario):
    provisional, route, ending = scenario.split('-')
    target='ghost' if provisional=='provisional' else 'anchor'
    fleet=Fleet(binary,output,home,held_mock(target))
    try:
        fleet.prime()
        fleet.open_target(provisional=='provisional')
        if route=='board':fleet.app.send(b'b')
        fleet.app.send(b'/')
        fleet.app.send(b'\x7f'*len(fleet.filter))
        phase=fleet.refresh(WORKERS)
        validate_counts(fleet.records(),phase,[target],0)
        if ending=='refused':
            # Cancel search to Board first: its status footer is visible and
            # its Thread return is still owned without starting hydration.
            fleet.app.send(b'\x1b')
            (home/'shutdown').touch()
            # Board intentionally has no backend-status footer. Observe the
            # exact fixture process exiting instead, then require the actual
            # closed-actor admission error (not a later RPC read error).
            primary = [int(f.name.split('.')[-1]) for f in home.glob('rpc.*')
                       if 'fleet-000' in f.read_text()]
            require(len(primary)==1, 'ambiguous primary fixture process')
            fleet.app.wait(lambda:not Path('/proc/{}'.format(primary[0])).exists(),
                           'fixture process did not exit before return',8)
        else:
            (home/'hold').touch()
        fleet.app.send(b'\x1b')
        if route=='board' and ending!='refused':fleet.app.send(b'\x1b')
        if ending=='refused':
            fleet.app.wait(lambda:'Conversation unavailable: App Server actor is not available' in fleet.app.screen(),
                           'closed-actor admission failure was not visible',3)
            validate_counts(fleet.records(),phase,[target],0)
            fleet.app.drain(.2)
            validate_counts(fleet.records(),phase,[target],0)
            fleet.app.send(b'\x1b')
            return {'mode':scenario,'passed':True,'loadAdmissionRefused':True,
                    'noReplay':True,'distinctBackgroundCompletions':32,
                    'resources':sample(fleet.app.proc.pid),'terminal':fleet.app.finish()}
        fleet.app.wait(lambda:(home/'held').exists(),'evicted return was not admitted into a real read',3)
        validate_counts(fleet.records(),phase,[target],1)
        fleet.app.send(b'\x1b')
        if ending=='disconnect':(home/'disconnect').touch()
        (home/'release').touch()
        fleet.clear_filter()
        if ending=='disconnect':
            fleet.app.wait(lambda:'disconnected' in fleet.app.screen().lower() or
                           'peer closed the connection' in fleet.app.screen().lower(),
                           'transport closure was not surfaced',8)
            before=len(fleet.records());fleet.app.drain(.2)
            require(len(fleet.records())==before,'closed backend continued requests')
            result={'disconnected':True,'noReplay':True}
        else:
            final=fleet.boundary([{'method':'item/completed','params':{'threadId':target}},
                                  {'method':'thread/queue/changed','params':{'threadId':target}}])
            validate_counts(fleet.records(),final,[target],0)
            validate_counts(fleet.records(),final,[target],0,'thread/queue/list')
            # The delayed read may finish, but it is not a new subscription.
            result={'admittedReadCount':1,'lateCompletionRetainedNoOwner':True,'allReleased':True}
        result.update(mode=scenario,passed=True,distinctBackgroundCompletions=32,
                      resources=sample(fleet.app.proc.pid),terminal=fleet.app.finish())
        return result
    finally:
        fleet.close()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('binary',type=Path);p.add_argument('output',type=Path)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    require(sys.platform=='linux', 'Linux controlling PTY and /proc required')
    b=a.binary.resolve(strict=True)
    r={'schema':'codex-tui/cache-interruption-regression/v1','sourceSha':identity(b,a.output),
       'binarySha256':hashlib.sha256(b.read_bytes()).hexdigest(),
       'realCodexQualification':False,'realSshQualification':False,'checks':[]}
    for name in CASES:
        o=a.output/name;o.mkdir()
        try:
            with tempfile.TemporaryDirectory(prefix='ctui-cache-interrupt-') as d:
                c=case(b,o,Path(d),name)
        except Exception as e:c={'mode':name,'passed':False,'error':str(e)}
        r['checks'].append(c);print(json.dumps(c),flush=True)
    r['status']='passed' if all(c['passed'] for c in r['checks']) else 'failed'
    (a.output/'receipt.json').write_text(json.dumps(r,indent=2)+'\n')
    return 0 if r['status']=='passed' else 1
if __name__=='__main__':sys.exit(main())
