#!/usr/bin/env python3
"""Bounded Linux PTY check of quiet visual alerts and exact-thread navigation.

Uses an explicit absolute mock executable, an empty synthetic profile and only
query/subscription RPCs. This is NOT real Codex, SSH, screen-reader or ROI evidence.
No dependency outside Python's standard library is required.
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
import unicodedata

MAX_OUTPUT = 1048576
MAX_TRACE = 131072
READ_METHODS = {'initialize', 'initialized', 'thread/list', 'thread/loaded/list',
                'thread/read', 'thread/turns/list', 'thread/items/list',
                'thread/resume', 'thread/unsubscribe', 'thread/goal/get'}
MOCK = r'''
import json, os, select, sys, time
from pathlib import Path
root = Path(os.environ['VISUAL_ROOT'])
trace = Path(os.environ['VISUAL_TRACE'])
rows = {'alpha': {'id':'alpha','name':'ALPHA_TASK','cwd':str(root),'updatedAt':200,
                  'status':{'type':'idle'}},
        'beta': {'id':'beta','name':'BETA_TASK','cwd':str(root),'updatedAt':100,
                 'status':{'type':'active','activeFlags':[]}}}
seen = 0
pending = b''
def record(**fields):
    with trace.open('a',encoding='utf-8') as f:
        f.write(json.dumps(fields,separators=(',',':'))+'\n')
def emit(value):
    print(json.dumps(value,separators=(',',':')),flush=True)
record(kind='start',pid=os.getpid())
while True:
    command = root/'command.json'
    if command.exists():
        change = json.loads(command.read_text())
        if change['seq'] > seen:
            seen = change['seq']
            rows['beta']['status'] = {'type':change['status']}
            emit({'method':'thread/status/changed',
                  'params':{'threadId':'beta','status':rows['beta']['status']}})
            record(kind='event',seq=seen,status=change['status'])
    ready,_,_ = select.select([sys.stdin],[],[],.02)
    if not ready:
        continue
    chunk = os.read(sys.stdin.fileno(),65536)
    if not chunk:
        break
    pending += chunk
    if len(pending)>131072:
        raise SystemExit(96)
    while b'\n' in pending:
        line,pending=pending.split(b'\n',1)
        request=json.loads(line)
        method=request.get('method'); params=request.get('params') or {}
        record(kind='request',method=method,threadId=params.get('threadId'),
               excludeTurns=params.get('excludeTurns'))
        if request.get('id') is None:
            continue
        if method=='initialize': result={'serverInfo':{'name':'quiet-visual-fixture','version':'0.0.0'}}
        elif method=='thread/list': result={'data':list(rows.values()),'nextCursor':None}
        elif method=='thread/loaded/list': result={'data':[],'nextCursor':None}
        elif method in ('thread/resume','thread/read'):
            ident=params.get('threadId')
            if ident not in rows: raise SystemExit(93)
            result={'thread':rows[ident]}
        elif method=='thread/turns/list': result={'data':[{'id':'turn-fixture','status':'completed'}],'nextCursor':None}
        elif method=='thread/items/list':
            ident=params.get('threadId')
            result={'data':[{'turnId':'turn-fixture','item':{'id':'body-fixture','type':'agentMessage',
                    'text':'TARGET_'+str(ident).upper()+'_BODY'}}],'nextCursor':None}
        elif method=='thread/unsubscribe': result={'status':'unsubscribed'}
        else:
            emit({'id':request['id'],'error':{'code':-32601,'message':'unsupported fixture query'}})
            continue
        emit({'id':request['id'],'result':result})
'''


def require(value, message):
    if not value:
        raise ValueError(message)


def screen_text(data, width, height):
    """Small fail-closed observer for Ratatui's bounded CSI output, not a full VT."""
    require(1 <= width <= 200 and 1 <= height <= 80, 'viewport bounds')
    require(len(data) <= MAX_OUTPUT, 'output bound')
    grid = [[' '] * width for _ in range(height)]
    row = col = offset = 0
    text = bytes(data).decode('utf-8', errors='replace')
    while offset < len(text):
        char = text[offset]
        if char == '\x1b':
            match = re.match(r'\x1b\[([0-?]*)([ -/]*)([@-~])', text[offset:])
            if not match:
                require(text[offset:] == '\x1b' or text[offset:].startswith('\x1b['),
                        'unsupported terminal control')
                break
            args, intermediate, op = match.groups()
            require(not intermediate, 'unsupported CSI intermediate')
            if op in ('H', 'f'):
                values = (args or '1;1').split(';')
                row = int(values[0] or '1') - 1
                col = int(values[1] or '1') - 1 if len(values)>1 else 0
                require(0 <= row < height and 0 <= col < width, 'cursor outside viewport')
            elif op == 'J':
                require(args in ('2','3'), 'unsupported partial erase')
                grid = [[' '] * width for _ in range(height)]
            elif op == 'K':
                require(args in ('','0','1','2'), 'unsupported line erase')
                start = 0 if args in ('1','2') else col
                stop = col+1 if args == '1' else width
                for x in range(start,min(stop,width)): grid[row][x]=' '
            elif op not in ('m','h','l'):
                raise ValueError('unsupported CSI: '+op)
            offset += len(match.group())
            continue
        if char == '\r': col=0
        elif char == '\n': row=min(row+1,height-1)
        elif ord(char)>=32 and not unicodedata.combining(char):
            if col < width:
                size=2 if unicodedata.east_asian_width(char) in ('W','F') else 1
                for x in range(col,min(col+size,width)):
                    if grid[row][x]=='' and x>0: grid[row][x-1]=' '
                    if x+1<width and grid[row][x+1]=='': grid[row][x+1]=' '
                    grid[row][x]=' '
                grid[row][col]=char
                if size==2 and col+1<width: grid[row][col+1]=''
                col += size
        offset += 1
    return '\n'.join(''.join(line) for line in grid)


def trace_rows(path):
    raw=path.read_bytes()
    require(len(raw)<=MAX_TRACE, 'trace bound')
    return [json.loads(line) for line in raw.split(b'\n')[:-1] if line]


def verify_identity(binary, metadata):
    data=json.loads(metadata.read_text(encoding='utf-8'))
    require(data.get('platform')=='linux','Linux metadata required')
    source=data.get('commitSha'); expected=data.get('binarySha256')
    require(isinstance(source,str) and re.fullmatch('[0-9a-f]{40}',source),'exact source required')
    require(isinstance(expected,str) and re.fullmatch('[0-9a-f]{64}',expected),'exact digest required')
    require(binary.is_file() and not binary.is_symlink(),'regular binary required')
    require(hashlib.sha256(binary.read_bytes()).hexdigest()==expected,'binary digest mismatch')
    return source,expected


def prepare(home, trace, language, presentation):
    bindir=home/'bin';bindir.mkdir()
    require(not any(c in sys.executable for c in (' ','\n','\r')), 'Python shebang path')
    server=bindir/'absolute-visual-fixture'
    server.write_text('#!'+sys.executable+'\n'+MOCK,encoding='utf-8');server.chmod(0o700)
    for name in ('codex','gh','glab','git','notify-send','osascript','powershell.exe'):
        path=bindir/name
        path.write_text('#!/bin/sh\nprintf "%s\\n" "'+name+'" >> "$VISUAL_TRAP"\nexit 97\n')
        path.chmod(0o700)
    config=home/'config/codex-tui';config.mkdir(parents=True)
    (config/'config.toml').write_text('[ui]\nlanguage='+json.dumps(language)+
        '\npresentation='+json.dumps(presentation)+'\n[notifications]\nmode="off"\n'
        '[app_server]\nactive="visual-fixture"\n[app_server.targets.visual-fixture]\n'
        'transport="stdio"\ncodex_bin='+json.dumps(str(server))+'\n',encoding='utf-8')
    # No inherited credentials, proxies or user configuration.
    return {'HOME':str(home),'USERPROFILE':str(home),'XDG_CONFIG_HOME':str(home/'config'),
            'XDG_DATA_HOME':str(home/'data'),'XDG_STATE_HOME':str(home/'state'),
            'XDG_CACHE_HOME':str(home/'cache'),'CODEX_HOME':str(home/'codex'),
            'PATH':str(bindir),'TERM':'xterm-256color','LANG':'C.UTF-8',
            'VISUAL_ROOT':str(home),'VISUAL_TRACE':str(trace),'VISUAL_TRAP':str(home/'traps.log')}


def stop(process):
    try: os.killpg(process.pid,signal.SIGTERM)
    except ProcessLookupError: pass
    try: process.wait(timeout=2)
    except subprocess.TimeoutExpired:
        try: os.killpg(process.pid,signal.SIGKILL)
        except ProcessLookupError: pass
        process.wait(timeout=2)


def validate_requests(records):
    requests=[r for r in records if r.get('kind')=='request']
    require(all(r.get('method') in READ_METHODS for r in requests),'unexpected or write RPC')
    require(all(r.get('excludeTurns') is True for r in requests if r['method']=='thread/resume'),
            'subscription must exclude turns')
    require(all(r.get('threadId')=='beta' for r in requests if r['method'] in
                ('thread/resume','thread/read','thread/turns/list','thread/items/list','thread/unsubscribe')),
            'wrong thread navigation')
    return requests


def run(binary, metadata, output, language, width, height, presentation):
    require(sys.platform=='linux','Linux controlling PTY only')
    require(language in ('en','zh-CN'),'unsupported language')
    require(presentation in ('normal','quiet','screen-reader'),'unsupported presentation')
    require((width,height) in ((60,24),(100,30)), 'unsupported matrix viewport')
    source,digest=verify_identity(binary,metadata)
    output.mkdir(parents=True,exist_ok=False)
    trace=output/'server.jsonl';trace.write_text('')
    receipt={'schema':'codex-tui/quiet-visual-probe/v1','sourceSha':source,'binarySha256':digest,
             'language':language,'width':width,'height':height,'presentation':presentation,
             'status':'failed','checks':{},'failure':None,'fixtureOnly':True,
             'probeSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
             'fixtureSha256':hashlib.sha256(MOCK.encode()).hexdigest(),
             'realCodexQualification':False,'realSshQualification':False,
             'screenReaderQualification':False,'humanTimeSavings':None,
             'stableReady':False,'publicationAllowed':False}
    raw=bytearray()
    with tempfile.TemporaryDirectory(prefix='ctui-visual-') as tmp:
        home=Path(tmp);env=prepare(home,trace,language,presentation)
        master,slave=pty.openpty()
        fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',height,width,0,0))
        before=termios.tcgetattr(slave)
        def setup():
            os.setsid();fcntl.ioctl(slave,termios.TIOCSCTTY,0)
        process=subprocess.Popen([str(binary),'--target','visual-fixture'],stdin=slave,stdout=slave,
                                 stderr=slave,cwd=home,env=env,preexec_fn=setup,close_fds=True)
        def pump(seconds):
            until=time.monotonic()+seconds
            while time.monotonic()<until:
                ready,_,_=select.select([master],[],[],min(.03,max(0,until-time.monotonic())))
                if ready:
                    try: chunk=os.read(master,65536)
                    except OSError as error:
                        if error.errno==errno.EIO: return
                        raise
                    if not chunk: return
                    raw.extend(chunk);require(len(raw)<=MAX_OUTPUT,'terminal output bound')
                if process.poll() is not None: return
        def screen(): return screen_text(raw,width,height)
        def wait(predicate,label):
            until=time.monotonic()+6
            while time.monotonic()<until:
                pump(.04)
                if predicate(): return
                require(process.poll() is None,'unexpected exit: '+label)
            raise ValueError(label)
        def snap(name):
            text=screen().replace(str(home),'<fixture-home>')
            (output/(name+'.screen.txt')).write_text(text,encoding='utf-8')
            return text
        def send(data): os.write(master,data);pump(.15)
        def trigger(seq,status):
            path=home/'command-next.json'
            path.write_text(json.dumps({'seq':seq,'status':status}))
            path.replace(home/'command.json')
        def resumes():
            return [r for r in trace_rows(trace) if r.get('method')=='thread/resume']
        idle='Now idle; check result' if language=='en' else '已空闲；请核对结果'
        error='System error; inspect details' if language=='en' else '系统错误；请查看详情'
        action='! open' if language=='en' else '! 跳转'
        help_label='Help' if language=='en' else '帮助'
        try:
            wait(lambda: 'ALPHA_TASK' in screen() and 'BETA_TASK' in screen(),'registry-not-visible')
            pump(.6)
            require(action not in snap('initial'),'startup-replayed-history')
            trigger(1,'idle')
            wait(lambda: idle in screen() and action in screen(),'quiet-idle-alert-not-visible')
            text=snap('quiet-idle-alert')
            require('Work completed' not in text and '工作已完成' not in text and
                    'Goal completed' not in text and 'Goal 已完成' not in text,'idle-claimed-success')
            require(b'\x07' not in raw,'quiet-mode-rang-bell')
            receipt['checks']['idleIsNotSuccess']=True
            receipt['checks']['quietVisualActionVisible']=True
            send(b'?');wait(lambda: help_label.lower() in screen().lower(),'help-not-visible')
            send(b'!');pump(.2)
            require(not resumes(),'alert-stole-overlay-focus')
            snap('help-kept-focus');receipt['checks']['overlayNotHijacked']=True
            send(b'\x1b');wait(lambda: action in screen(),'alert-lost-after-help')
            send(b'!');wait(lambda: 'TARGET_BETA_BODY' in screen(),'wrong-or-missing-thread-body')
            snap('exact-thread-opened')
            require(resumes() and all(r['threadId']=='beta' for r in resumes()),'wrong-thread-resumed')
            receipt['checks']['exactThreadJump']=True
            send(b'\x1b')
            wait(lambda: 'ALPHA_TASK' in screen() and 'TARGET_BETA_BODY' not in screen(),'return-not-visible')
            snap('returned-to-registry');receipt['checks']['returnToRegistry']=True
            trigger(2,'systemError')
            wait(lambda: error in screen() and action in screen(),'quiet-error-alert-not-visible')
            snap('quiet-error-alert');receipt['checks']['quietSystemErrorVisible']=True
            require(b'\x07' not in raw,'quiet-mode-rang-bell')
            receipt['checks']['noBell']=True
            validate_requests(trace_rows(trace));receipt['checks']['querySubscriptionOnly']=True
            receipt['status']='passed'
        except Exception as exc:
            receipt['failure']=str(exc)
            try: snap('failure')
            except Exception: pass
        finally:
            try:
                if process.poll() is None:
                    send(b'\x1b');send(b'q');pump(.5);process.wait(timeout=2)
            except Exception as exc:
                receipt['status']='failed';receipt['failure']=receipt['failure'] or 'exit: '+str(exc)
            finally:
                stop(process)
                receipt['exitCode']=process.returncode
                receipt['terminalRestored']=termios.tcgetattr(slave)==before
                records=trace_rows(trace)
                receipt['backendStarts']=sum(r.get('kind')=='start' for r in records)
                receipt['traps']=(home/'traps.log').read_text().splitlines() if (home/'traps.log').exists() else []
                try:
                    validate_requests(records)
                    require(set(receipt['traps'])<={'git'},'external service or notification command attempted')
                    require(receipt['backendStarts']==1,'unexpected reconnect')
                    require(receipt['terminalRestored'] and process.returncode==0,'terminal or exit failure')
                    require(b'\x07' not in raw,'quiet-mode-rang-bell')
                except Exception as exc:
                    receipt['status']='failed';receipt['failure']=receipt['failure'] or 'cleanup: '+str(exc)
                receipt['terminalOutputBytes']=len(raw)
                os.close(master);os.close(slave)
    (output/'terminal-output.bin').write_bytes(raw)
    receipt['terminalOutputSha256']=hashlib.sha256(raw).hexdigest()
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2,ensure_ascii=False)+'\n',encoding='utf-8')
    print(json.dumps(receipt,ensure_ascii=False))
    return 0 if receipt['status']=='passed' else 1


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--metadata',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--language',choices=['en','zh-CN'],default='en')
    p.add_argument('--width',type=int,default=60);p.add_argument('--height',type=int,default=24)
    p.add_argument('--presentation',choices=['normal','quiet','screen-reader'],default='normal')
    a=p.parse_args()
    return run(a.binary.resolve(),a.metadata.resolve(),a.output.resolve(),a.language,a.width,a.height,a.presentation)

if __name__=='__main__': raise SystemExit(main())
