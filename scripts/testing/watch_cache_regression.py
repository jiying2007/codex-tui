#!/usr/bin/env python3
"""Actual Linux TUI cache-return and multi-session RPC/resource qualification.

No real Codex account or network is used. The normal cache capacity is unchanged.
Observed completions of 32 distinct background histories establish the eviction
precondition; a source test separately asserts actual LRU eviction.
"""
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
from watch_ownership_regression import identity

WORKERS = tuple('fleet-%03d' % i for i in range(32))
CASES = ('registered-search-resident', 'provisional-search-resident',
         'registered-search-evicted', 'provisional-search-evicted',
         'registered-board-evicted', 'provisional-board-evicted',
         'registered-search-retired', 'provisional-search-retired',
         'provisional-search-read-error')
RSS_BUDGET_KIB = 256 * 1024
FD_BUDGET = 96
TRACE_BUDGET = 12 * 1024 * 1024

MOCK = "#!/usr/bin/python3 -S\nimport json, os, sys, threading, time\nfrom pathlib import Path\nhome=Path(os.environ['HOME']); lock=threading.RLock(); seq=0; started=False; subscriptions=set()\nlog=open(os.environ['MOCK_LOG']+'.'+str(os.getpid()),'a',buffering=1)\nrows={name:{'id':name,'name':name,'cwd':str(home),'updatedAt':2000000000-i,'status':{'type':'active' if name.startswith('fleet-') else 'idle'}} for i,name in enumerate(['anchor']+['fleet-%03d'%i for i in range(32)])}\ndef emit(x):\n with lock:print(json.dumps(x,separators=(',',':')),flush=True)\ndef deliver(event):\n ident=event['params'].get('threadId')\n scoped=('id' in event or event['method'].startswith(('item/','turn/')) or event['method'] in ('thread/queue/changed','serverRequest/resolved'))\n if not scoped or ident in subscriptions:emit(event)\ndef control():\n global seq\n while True:\n  path=home/'control.json'\n  if path.exists():\n   v=json.loads(path.read_text())\n   if v['seq']>seq:\n    with lock:\n     seq=v['seq']\n     for event in v.get('events',[]):\n      ident=event['params'].get('threadId')\n      if event['method'] in ('thread/deleted','thread/archived'):rows.pop(ident,None)\n      if event['method']=='thread/status/changed' and ident in rows:rows[ident]['status']=event['params']['status']\n      if event['method'] in ('thread/archived','thread/deleted','thread/closed'):subscriptions.discard(ident)\n      deliver(event)\n     for ident,row in rows.items():\n      row['name']='FENCE_%d_%s'%(seq,ident)\n      emit({'method':'thread/name/updated','params':{'threadId':ident,'threadName':row['name']}})\n  time.sleep(.005)\nfor line in sys.stdin:\n r=json.loads(line)\n if 'id' not in r:continue\n m=r.get('method');p=r.get('params',{});ident=p.get('threadId')\n with lock:\n  log.write(json.dumps({'seq':seq,'method':m,'params':p})+'\\n')\n  if m=='initialize':out={'serverInfo':{'name':'cache-fixture','version':'0.0.0'}}\n  elif m=='thread/list':out={'data':list(rows.values()),'nextCursor':None}\n  elif m=='thread/loaded/list':out={'data':list(rows),'nextCursor':None}\n  elif m=='thread/resume':\n   assert p=={'threadId':ident,'excludeTurns':True}\n   subscriptions.add(ident);out={'thread':dict(rows.get(ident,{'id':ident}))}\n  elif m=='thread/unsubscribe':\n   status='notLoaded' if ident not in rows else ('unsubscribed' if ident in subscriptions else 'notSubscribed')\n   subscriptions.discard(ident);out={'status':status}\n  elif m=='thread/read' and ident in ('ghost','anchor') and (home/'read-error').exists():\n   log.write(json.dumps({'seq':seq,'method':'fixture/read-rejected','params':p})+'\\n')\n   emit({'id':r['id'],'error':{'code':-32000,'message':'cache fixture read rejected'}});continue\n  elif m=='thread/read' and (home/'disconnect').exists():sys.exit(0)\n  elif m=='thread/read':out={'thread':dict(rows.get(ident,{'id':ident}),name='History '+str(seq))}\n  elif m=='thread/turns/list':out={'data':[{'id':'turn-1','status':'completed'}],'nextCursor':None}\n  elif m=='thread/items/list' and ident=='ghost' and p.get('cursor') is None:out={'data':[{'turnId':'turn-new','item':{'id':'item-new','type':'agentMessage','text':'LATEST_IS_NOT_SEARCH_HIT'}}],'nextCursor':'hitpage'}\n  elif m=='thread/items/list':out={'data':[{'turnId':'turn-1','item':{'id':'item-1','type':'agentMessage','text':'CACHE_BODY_%d_%s'%(seq,ident)}}],'nextCursor':None}\n  elif m=='thread/queue/list':out={'data':[],'nextCursor':None}\n  elif m=='thread/search':out={'data':[{'thread':{'id':'ghost'},'snippet':'GHOST_HIT'}],'nextCursor':None}\n  elif m=='thread/searchOccurrences':out={'data':[{'turnId':'turn-1','itemId':'item-1','snippet':'GHOST_HIT'}],'nextCursor':None}\n  else:\n   emit({'id':r['id'],'error':{'code':-32601,'message':'unsupported fixture'}});continue\n  emit({'id':r['id'],'result':out})\n  if m=='thread/list' and not started:started=True;threading.Thread(target=control,daemon=True).start()\n"


def validate_counts(records, seq, ids, expected, method='thread/read'):
    counts = {ident: sum(row['seq'] == seq and row['method'] == method and
                        row['params'].get('threadId') == ident for row in records)
              for ident in ids}
    require(all(count == expected for count in counts.values()),
            '{} phase {}: expected {} per target, observed {}'.format(method, seq, expected, counts))
    return counts


def validate_return(kind, body, reads):
    expected_reads = 0 if kind in ('resident', 'retired') else 1
    require(reads == expected_reads, 'return expected {} reads, observed {}'.format(expected_reads, reads))
    require(body == (kind not in ('retired', 'read-error')),
            'cache return restored incorrect or missing history')


def sample(pid):
    result = {}
    for line in Path('/proc/{}/status'.format(pid)).read_text().splitlines():
        key, _, value = line.partition(':')
        if key in ('VmRSS', 'VmHWM', 'Threads'):
            result[key] = int(value.split()[0])
    result['fdCount'] = len(list(Path('/proc/{}/fd'.format(pid)).iterdir()))
    require(result.get('VmHWM', 0) > 0, 'missing live process RSS')
    require(result['VmHWM'] < RSS_BUDGET_KIB, 'cache fixture RSS diagnostic budget exceeded')
    require(result['fdCount'] < FD_BUDGET, 'cache fixture descriptor budget exceeded')
    return result


class CachedScreenSession(Session):
    """Avoid reparsing an unchanged transcript during repeated assertions."""
    def screen(self):
        size = len(self.raw)
        if getattr(self, '_screen_size', -1) != size:
            self._screen_value = super().screen()
            self._screen_size = size
        return self._screen_value


class Fleet:
    def __init__(self, binary, output, home, mock=MOCK):
        self.home, self.output = home, output
        self.seq, self.filter = 0, ''
        self.observations = []
        env = environment(home, home/'rpc', 'cache-workload')
        (home/'bin/codex').write_text(mock, encoding='utf-8')
        self.app = CachedScreenSession(binary, output/'session', home, env)

    def records(self):
        files = list(self.home.glob('rpc.*'))
        require(sum(f.stat().st_size for f in files) < TRACE_BUDGET, 'cache trace budget exceeded')
        result = []
        for path in files:
            text = path.read_text()
            result.extend(json.loads(row) for row in text[:text.rfind('\n')+1].splitlines())
        return result

    def body(self, target, seq):
        return 'CACHE_BODY_{}_{}'.format(seq, target) in self.app.screen()

    def choose(self, target):
        self.app.send(b'/')
        query = 'thread:'+target
        self.app.send(b'\x7f'*len(self.filter)+query.encode()+b'\r')
        self.filter = query
        self.app.send(b'\r')
        self.app.wait(lambda: self.body(target, self.seq), 'initial body missing for '+target)

    def open_target(self, provisional):
        target = 'ghost' if provisional else 'anchor'
        if provisional:
            self.app.send(b'\x06'); self.app.send(b'ghost\r')
            self.app.wait(lambda: 'GHOST_HIT' in self.app.screen(), 'transcript hit missing')
            self.app.send(b'\r')
            self.app.wait(lambda: self.body(target, self.seq), 'provisional body missing')
        else:
            self.choose(target)
        return target

    def boundary(self, events):
        self.seq += 1
        path = self.home/'control.tmp'
        path.write_text(json.dumps({'seq':self.seq,'events':events}), encoding='utf-8')
        path.replace(self.home/'control.json')
        self.app.wait(lambda: 'FENCE_{}_'.format(self.seq) in self.app.screen(),
                      'ordered cache actor fence missing', 15)
        self.observations.append(dict(seq=self.seq, resources=sample(self.app.proc.pid)))
        return self.seq

    def refresh(self, workers):
        seq = self.boundary([{'method':'item/completed','params':{'threadId':x}} for x in workers])
        rows = self.records()
        validate_counts(rows, seq, workers, 1)
        validate_counts(rows, seq, workers, 1, 'thread/items/list')
        return seq

    def prime(self):
        self.app.wait(lambda: '33 total' in self.app.screen(), 'cache registry missing')
        for target in WORKERS:
            self.choose(target)
            self.app.send(b'\x1b')
        self.refresh(WORKERS)

    def clear_filter(self):
        self.app.send(b'/')
        self.app.send(b'\x7f'*len(self.filter)+b'\r')
        self.filter = ''

    def check_case(self, name):
        provisional, route, kind = name.split('-', 2)
        target = self.open_target(provisional == 'provisional')
        initial_seq = self.seq
        if route == 'board':
            self.app.send(b'b')
        self.app.send(b'/')
        # Keep the return route, but expose surviving Registry rows even when
        # the originally filtered target is deleted during the test.
        self.app.send(b'\x7f'*len(self.filter))
        refreshed = WORKERS[:4] if kind == 'resident' else WORKERS
        phase = self.refresh(refreshed)
        # No target event was sent while other histories replaced the LRU.
        validate_counts(self.records(), phase, [target], 0)
        if kind == 'retired':
            phase = self.boundary([{'method':'thread/deleted','params':{'threadId':target}}])
        elif kind == 'read-error':
            (self.home/'read-error').touch()
        self.app.send(b'\x1b')
        if route == 'board':
            self.app.send(b'\x1b')
        if kind not in ('resident', 'retired', 'read-error'):
            self.app.wait(lambda: self.body(target, phase), 'evicted return did not reload body', 3)
        elif kind == 'read-error':
            self.app.wait(lambda: 'Conversation unavailable:' in self.app.screen(),
                          'rejected read was not visible', 3)
        self.app.drain(.1)
        rows = self.records()
        if kind == 'read-error':
            validate_counts(rows, phase, [target], 1, 'fixture/read-rejected')
        reads = sum(r['seq'] == phase and r['method']=='thread/read' and
                    r['params'].get('threadId')==target for r in rows)
        body = self.body(target, initial_seq if kind=='resident' else phase)
        (self.output/(name+'.screen.txt')).write_text(self.app.screen(), encoding='utf-8')
        validate_return(kind, body, reads)
        result = {'mode':name,'passed':True,'distinctBackgroundCompletions':len(refreshed),
                  'phase':phase,'returnReads':reads,'bodyVisible':body,
                  'resources':sample(self.app.proc.pid)}
        # Leaving an idle return must release it, even after rejected hydration.
        if kind != 'retired':
            self.app.send(b'\x1b')
        (self.home/'read-error').unlink(missing_ok=True)
        self.clear_filter()
        released = self.boundary([{'method':'item/completed','params':{'threadId':target}},
                                  {'method':'thread/queue/changed','params':{'threadId':target}}])
        validate_counts(self.records(), released, [target], 0)
        validate_counts(self.records(), released, [target], 0, 'thread/queue/list')
        result['eventualRelease'] = True
        return result

    def workload(self, seconds):
        start = time.monotonic()
        samples, rounds = [], 0
        while time.monotonic()-start < seconds:
            phase = self.refresh(WORKERS)
            samples.append(dict(elapsedSeconds=round(time.monotonic()-start, 3),
                                phase=phase, resources=sample(self.app.proc.pid)))
            rounds += 1
            self.app.drain(.25)
        elapsed = time.monotonic()-start
        require(rounds >= 1 and elapsed >= seconds, 'duration workload not exercised')
        idle = self.boundary([{'method':'thread/status/changed',
                              'params':{'threadId':x,'status':{'type':'idle'}}} for x in WORKERS])
        final = self.boundary([{'method':'item/completed','params':{'threadId':x}} for x in WORKERS])
        validate_counts(self.records(), final, WORKERS, 0)
        return {'mode':'multi-session-workload','passed':True,'sessions':len(WORKERS),
                'minimumDurationSeconds':seconds,'elapsedSeconds':round(elapsed,3),
                'rounds':rounds,'expectedHistoryReads':rounds*len(WORKERS),
                'ownerCompletionPhase':idle,'allReleased':True,'samples':samples}

    def close(self):
        try:
            (self.output/'rpc.json').write_text(json.dumps(self.records(),indent=2)+'\n')
            (self.output/'observations.json').write_text(json.dumps(self.observations,indent=2)+'\n')
        finally:
            self.app.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path); parser.add_argument('output', type=Path)
    parser.add_argument('--duration-seconds', type=int, default=30)
    args = parser.parse_args()
    require(sys.platform=='linux', 'Linux controlling PTY and /proc required')
    require(1 <= args.duration_seconds <= 600, 'duration must be within 1..600 seconds')
    binary = args.binary.resolve(strict=True); args.output.mkdir(parents=True,exist_ok=False)
    receipt = {'schema':'codex-tui/cache-workload-regression/v1','status':'failed',
               'sourceSha':identity(binary,args.output),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'realCodexQualification':False,'realSshQualification':False,
               'resourceBudgetScope':'33 registered threads plus provisional hit; controlled workload only',
               'rssBudgetKiB':RSS_BUDGET_KIB,'descriptorBudget':FD_BUDGET,'checks':[]}
    try:
        with tempfile.TemporaryDirectory(prefix='ctui-cache-workload-') as d:
            fleet = Fleet(binary,args.output,Path(d))
            try:
                fleet.prime()
                # A failing case is retained, not retried. Recover only the UI
                # surface so the independent remaining controls can be observed.
                for name in CASES:
                    try:
                        result = fleet.check_case(name)
                    except Exception as error:
                        result = {'mode':name,'passed':False,'error':str(error)}
                        (fleet.home/'read-error').unlink(missing_ok=True)
                        fleet.app.send(b'\x1b');fleet.app.send(b'\x1b')
                        fleet.clear_filter()
                    receipt['checks'].append(result);print(json.dumps(result),flush=True)
                result = fleet.workload(args.duration_seconds)
                receipt['checks'].append(result);print(json.dumps(result),flush=True)
                receipt['terminal'] = fleet.app.finish()
            finally:
                fleet.close()
        from watch_active_history_regression import CASES as ACTIVE_CASES, run_cases
        receipt['checks'].extend(run_cases(binary, args.output/'active-history'))
        if all(x['passed'] for x in receipt['checks']) and len(receipt['checks']) == len(CASES)+1+len(ACTIVE_CASES):
            receipt['status']='passed'
    except Exception as error:
        receipt['error']=str(error)
    finally:
        (args.output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    return 0 if receipt['status']=='passed' else 1


if __name__ == '__main__': sys.exit(main())
