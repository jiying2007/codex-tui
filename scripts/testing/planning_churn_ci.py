#!/usr/bin/env python3
"""Offline, source-bound Linux PTY gate for 100/12k/20k Registry churn.

Only runs an absolute-path mock server; no real Codex, prompts, or credentials.
The observed duration includes PTY polling and is not an SSH/user SLO.
"""
import argparse
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from quiet_visual_probe import prepare, require, screen_text, stop

MAX_BYTES = 1_048_576
READ_ONLY = {'initialize', 'initialized', 'thread/list', 'thread/loaded/list',
             'thread/read', 'thread/resume', 'thread/unsubscribe', 'thread/goal/get',
             'thread/turns/list', 'thread/items/list'}
MOCK = r'''
import json, os, select, sys, time
from pathlib import Path
root=Path(os.environ['VISUAL_ROOT']); trace=Path(os.environ['VISUAL_TRACE'])
n=int(os.environ['CHURN_ROWS']); hz=int(os.environ['CHURN_HZ']); duration=float(os.environ['CHURN_SECONDS'])
rows=[{'id':'item-%05d'%i,'name':'A_TARGET_ORIGINAL' if i==0 else 'NOISE_%05d'%i,
       'cwd':str(root),'updatedAt':n-i,'status':{'type':'idle'}} for i in range(n)]
pending=b''; begin=None; next_at=0; seq=0; ended=False

def record(**values):
    values['monotonicNs']=time.monotonic_ns()
    with trace.open('a',encoding='utf-8') as f: f.write(json.dumps(values,separators=(',',':'))+'\n')
def emit(value):print(json.dumps(value,separators=(',',':')),flush=True)
def rename(index,name):
    rows[index]['name']=name
    emit({'method':'thread/name/updated','params':{'threadId':rows[index]['id'],'name':name}})
record(kind='start')
while True:
    if begin is None and (root/'go').exists():
        begin=time.monotonic(); next_at=begin; record(kind='churn-start')
    now=time.monotonic()
    if begin is not None and not ended:
        if now-begin < duration and now>=next_at:
            seq+=1
            rename(1+seq%(n-1),'NOISE_%06d'%seq)
            if seq==1:
                rename(0,'A_TARGET_DURING');record(kind='target',value='A_TARGET_DURING')
            record(kind='noise',seq=seq);next_at=now+1/hz
        if now-begin>=duration:
            rename(0,'A_TARGET_FINAL');record(kind='target',value='A_TARGET_FINAL')
            record(kind='churn-end',events=seq);ended=True
    ready,_,_=select.select([sys.stdin],[],[],.005)
    if not ready:continue
    chunk=os.read(sys.stdin.fileno(),65536)
    if not chunk:break
    pending+=chunk
    if len(pending)>131072:raise SystemExit(96)
    while b'\n' in pending:
        line,pending=pending.split(b'\n',1);request=json.loads(line)
        method=request.get('method');params=request.get('params') or {}
        record(kind='request',method=method,excludeTurns=params.get('excludeTurns'))
        if request.get('id') is None:continue
        if method=='initialize': result={'serverInfo':{'name':'planning-churn-fixture','version':'0.0.0'}}
        elif method=='thread/list':
            start=int(params.get('cursor') or 0);limit=min(200,max(1,int(params.get('limit') or 100)))
            end=min(n,start+limit);result={'data':rows[start:end], 'nextCursor':str(end) if end<n else None}
            record(kind='page',start=start,end=end)
        elif method=='thread/loaded/list':result={'data':[],'nextCursor':None}
        elif method in ('thread/read','thread/resume'):
            idx=int(params['threadId'].split('-')[-1]);result={'thread':rows[idx]}
        elif method in ('thread/turns/list','thread/items/list'):result={'data':[],'nextCursor':None}
        elif method=='thread/unsubscribe':result={'status':'unsubscribed'}
        else:
            emit({'id':request['id'],'error':{'code':-32601,'message':'unsupported read'}});continue
        emit({'id':request['id'],'result':result})
'''


def verify_source(binary, sha, env, home):
    require(re.fullmatch('[0-9a-f]{40}',sha or '') is not None,'exact source SHA required')
    require(binary.is_file() and not binary.is_symlink(),'regular binary required')
    digest=hashlib.sha256(binary.read_bytes()).hexdigest()
    check=subprocess.run([str(binary),'release','benchmark','--warmup','1',
                          '--iterations','1','--source','offline-churn-identity', '--json'],
                         cwd=home,env=env,stdin=subprocess.DEVNULL,
                         capture_output=True,timeout=20)
    require(check.returncode==5 and len(check.stdout)<65536,'offline identity diagnostic contract')
    data=json.loads(check.stdout)
    require(data.get('sourceSha')==sha and data.get('sampleQualified') is False,
            'program source SHA does not match checkout')
    require(hashlib.sha256(binary.read_bytes()).hexdigest()==digest,'binary changed during preflight')
    return digest


def trace_events(file):
    data=file.read_bytes()
    require(len(data)<=MAX_BYTES,'trace byte budget')
    return [json.loads(line) for line in data.split(b'\n')[:-1] if line]


def verify_events(records, rows):
    require(sum(r.get('kind')=='start' for r in records)==1,'unexpected backend restart')
    requests=[r for r in records if r.get('kind')=='request']
    require(requests and all(r.get('method') in READ_ONLY for r in requests),'non-read-only RPC observed')
    require(all(r.get('excludeTurns') is True for r in requests if r.get('method')=='thread/resume'),
            'subscription unexpectedly requested turns')
    cursor=0
    for page in (r for r in records if r.get('kind')=='page'):
        if page['start']==0:cursor=0
        require(page['start']==cursor and cursor<page['end']<=rows,'nonsequential Registry pages')
        cursor=page['end']
    require(cursor==rows,'incomplete Registry pagination')
    require(any(r.get('kind')=='churn-end' for r in records),'churn never ended')


def run(binary, sha, output, rows, hz, seconds):
    require(sys.platform=='linux','Linux PTY only')
    require(type(rows)==int and 2<=rows<=20000,'row limit 2..20000')
    require(type(hz)==int and 1<=hz<=50,'rate limit 1..50')
    require(2<=seconds<=15,'duration limit 2..15s')
    require(not output.exists(),'output directory must not exist')
    output.mkdir(parents=True)
    trace=output/'server.jsonl';trace.write_text('')
    receipt={'schema':'codex-tui/planning-churn-ci/v1','sourceSha':sha,'rows':rows,
             'hz':hz,'durationSeconds':seconds,'status':'failed','reason':None,
             'duringChurn':False,'helpMs':[],'noiseEvents':0,'exitCode':None,
             'terminalRestored':False,'fixtureOnly':True,'realCodexQualification':False,
             'humanTimeSavings':None,'stableReady':False,'publicationAllowed':False}
    raw=bytearray(); width=160; height=40
    with tempfile.TemporaryDirectory(prefix='ctui-churn-') as temp:
        home=Path(temp);env=prepare(home,trace,'en','normal')
        server=home/'bin/absolute-visual-fixture'
        server.write_text('#!'+sys.executable+'\n'+MOCK,encoding='utf-8');server.chmod(0o700)
        env.update(CHURN_ROWS=str(rows),CHURN_HZ=str(hz),CHURN_SECONDS=str(seconds))
        digest=verify_source(binary,sha,env,home);receipt['binarySha256']=digest
        master,slave=pty.openpty()
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',height,width,0,0))
        original=termios.tcgetattr(slave)
        def setup():os.setsid();fcntl.ioctl(slave,termios.TIOCSCTTY,0)
        process=subprocess.Popen([str(binary),'--target','visual-fixture'],
                                 stdin=slave,stdout=slave,stderr=slave,
                                 cwd=home,env=env,preexec_fn=setup,close_fds=True)
        cached='';cached_len=-1
        def pump(seconds):
            end=time.monotonic()+seconds
            while time.monotonic()<end:
                ready,_,_=select.select([master],[],[],min(.02,max(0,end-time.monotonic())))
                if ready:
                    try:chunk=os.read(master,65536)
                    except OSError as exc:
                        if exc.errno==errno.EIO:break
                        raise
                    if not chunk:break
                    raw.extend(chunk);require(len(raw)<=MAX_BYTES,'terminal byte budget')
                if process.poll() is not None:break
        def screen():
            nonlocal cached,cached_len
            if cached_len!=len(raw):cached=screen_text(raw,width,height);cached_len=len(raw)
            return cached
        def until(test,label,seconds=12):
            start=time.monotonic()
            while time.monotonic()-start<seconds:
                pump(.03)
                if test():return round((time.monotonic()-start)*1000,2)
                require(process.poll() is None,'unexpected process exit: '+label)
            raise ValueError(label)
        def send(data):os.write(master,data)
        try:
            until(lambda:any(r.get('kind')=='page' and r.get('end')==rows for r in trace_events(trace)),
                  'registry-incomplete',seconds=25)
            until(lambda:'A_TARGET_ORIGINAL' in screen(),'target-not-visible',seconds=15)
            send(b'b');pump(.4)
            until(lambda:'A_TARGET_ORIGINAL' in screen() and 'Ready (%d)'%rows in screen(),
                  'board-not-visible',seconds=12)
            (home/'go').write_text('go')
            started=time.monotonic();first_seen=None
            for _ in range(3):
                pump(.25)
                send(b'?')
                receipt['helpMs'].append(until(lambda:'Help' in screen(),'help-not-responsive',seconds=5))
                send(b'\x1b');until(lambda:'Help' not in screen(),'help-wont-close',seconds=5)
                if 'A_TARGET_DURING' in screen() and first_seen is None:first_seen=time.monotonic_ns()
            while time.monotonic()-started<seconds+.4:
                pump(.04)
                if first_seen is None and 'A_TARGET_DURING' in screen():first_seen=time.monotonic_ns()
            records=trace_events(trace)
            end=next(r for r in records if r.get('kind')=='churn-end')
            receipt['duringChurn']=first_seen is not None and first_seen<end['monotonicNs']
            until(lambda:'A_TARGET_FINAL' in screen(),'final-target-not-observed',seconds=12)
            receipt['status']='passed' if receipt['duringChurn'] else 'failed'
            if not receipt['duringChurn']:receipt['reason']='target-not-projected-during-churn'
        except Exception as error:receipt['reason']=str(error)
        finally:
            try:
                send(b'\x1b');pump(.12);send(b'\x1b');pump(.12);send(b'q');pump(.25)
                process.wait(timeout=3)
            except Exception as error:
                receipt['status']='failed';receipt['reason']=receipt['reason'] or ('exit: '+str(error))
            finally:
                stop(process)
                receipt['exitCode']=process.returncode
                receipt['terminalRestored']=termios.tcgetattr(slave)==original
                os.close(master);os.close(slave)
            records=trace_events(trace);receipt['noiseEvents']=sum(r.get('kind')=='noise' for r in records)
            try:
                verify_events(records,rows)
                require(trace.read_bytes().endswith(b'\n'),'truncated trace')
                traps=(home/'traps.log').read_text().splitlines() if (home/'traps.log').exists() else []
                require(set(traps)<={'git'},'external process attempted')
                require(receipt['terminalRestored'] and receipt['exitCode']==0,'terminal not restored')
                require(b'\x07' not in raw,'unexpected terminal bell')
                require(receipt['noiseEvents']>=1,'missing source changes')
            except Exception as error:
                receipt['status']='failed';receipt['reason']=receipt['reason'] or str(error)
    (output/'terminal-output.bin').write_bytes(raw)
    receipt['terminalSha256']=hashlib.sha256(raw).hexdigest()
    receipt['traceSha256']=hashlib.sha256(trace.read_bytes()).hexdigest()
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(receipt))
    return 0 if receipt['status']=='passed' else 1


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--source-sha',required=True)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--rows',type=int,default=20000)
    parser.add_argument('--hz',type=int,default=50)
    parser.add_argument('--duration',type=float,default=8)
    args=parser.parse_args()
    return run(args.binary.absolute(),args.source_sha,args.output,args.rows,args.hz,args.duration)


if __name__=='__main__':raise SystemExit(main())
