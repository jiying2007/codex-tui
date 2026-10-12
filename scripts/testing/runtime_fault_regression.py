#!/usr/bin/env python3
"""Linux actual-program signal/pipe/churn regressions; isolated fixtures only."""
import argparse
import ctypes
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
from app_server_transport_regression import environment, screen_text, require
from terminal_restoration import CursorReplies, validate_terminal

BOARD_MOCK = "#!/usr/bin/python3\nimport os,sys,json,time,threading\nfrom pathlib import Path\nlock=threading.Lock();home=Path(os.environ['HOME']);log=open(os.environ['MOCK_LOG'],'w',buffering=1);started=False\nN=5000\nrows=[{'id':('audit-target' if i==0 else 'noise-%05d'%i),'name':('AAA_BEFORE' if i==0 else 'ZZZ_%05d'%i),'cwd':str(home),'updatedAt':1791791400+(1 if i==0 else 0),'status':{'type':'idle'}} for i in range(N)]\ndef emit(x):\n with lock:\n  sys.stdout.write(json.dumps(x,separators=(',',':'))+'\\n');sys.stdout.flush()\ndef churn():\n while not (home/'start-churn').exists():time.sleep(float(os.environ['NOISE_INTERVAL']))\n log.write('churn-start\\n')\n emit({'method':'thread/name/updated','params':{'threadId':'audit-target','threadName':'AAA_AFTER'}})\n k=0\n while not (home/'stop-churn').exists():\n  emit({'method':'thread/name/updated','params':{'threadId':'noise-00001','threadName':'ZZZ_noise_%d'%k}});k+=1;time.sleep(float(os.environ['NOISE_INTERVAL']))\n log.write('churn-stop '+str(k)+'\\n')\n emit({'method':'thread/name/updated','params':{'threadId':'noise-00001','threadName':'ZZZ_SETTLED'}})\nfor line in sys.stdin:\n r=json.loads(line);m=r.get('method');i=r.get('id')\n if i is None:continue\n if m=='initialize':out={'serverInfo':{'name':'mock','version':'0.0.0'}}\n elif m=='thread/list':\n  start=int(r.get('params',{}).get('cursor') or 0);end=min(start+200,N);out={'data':rows[start:end],'nextCursor':str(end) if end<N else None}\n  if end==N:log.write('list-complete\\n')\n elif m=='thread/loaded/list':\n  out={'data':['audit-target'],'nextCursor':None};log.write('loaded-complete\\n')\n  if not started:started=True;threading.Thread(target=churn,daemon=True).start()\n else:\n  emit({'id':i,'error':{'code':-32601,'message':'unsupported fixture'}});continue\n emit({'id':i,'result':out})\n"
FORGE_MOCK = r'''#!/usr/bin/python3
import json, os, subprocess, sys
from pathlib import Path
child = subprocess.Popen(['/usr/bin/python3', '-c', 'import time; time.sleep(30)'])
with open(Path(os.environ['HOME'])/'helpers.jsonl', 'a') as out:
    out.write(json.dumps({'pid':os.getpid(), 'child':child.pid,
                         'stdout':os.readlink('/proc/self/fd/1'),
                         'stderr':os.readlink('/proc/self/fd/2')})+'\n')
print('{}', flush=True)
'''

class Session:
    def __init__(self, binary, output, home, env):
        self.output, self.raw = output, bytearray()
        output.mkdir(parents=True, exist_ok=False)
        self.master, self.slave = pty.openpty()
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 180, 0, 0))
        self.before = termios.tcgetattr(self.slave)
        parent = os.getpid()
        def controlling():
            libc = ctypes.CDLL(None, use_errno=True)
            if libc.prctl(1, signal.SIGKILL, 0, 0, 0) or os.getppid() != parent:
                os._exit(125)
            os.setsid()
            fcntl.ioctl(self.slave, termios.TIOCSCTTY, 0)
        self.proc = subprocess.Popen([str(binary)], cwd=home, env=env,
                                     stdin=self.slave, stdout=self.slave, stderr=self.slave,
                                     preexec_fn=controlling, close_fds=True)
        self.replies = CursorReplies()
        self.raw_observed = False
    def drain(self, seconds):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if not select.select([self.master], [], [], min(.03, max(0, until-time.monotonic())))[0]:
                continue
            try:
                chunk = os.read(self.master, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not chunk:
                break
            self.raw.extend(chunk)
            require(len(self.raw) <= 8*1024*1024, 'PTY stream exceeded 8 MiB')
            reply = self.replies.feed(chunk)
            if reply and self.proc.poll() is None:
                os.write(self.master, reply)
            self.raw_observed |= not bool(termios.tcgetattr(self.slave)[3] & (termios.ICANON|termios.ECHO))
    def wait(self, predicate, message, seconds=10):
        until = time.monotonic()+seconds
        while not predicate() and self.proc.poll() is None and time.monotonic()<until:
            self.drain(.05)
        require(predicate(), message)
    def send(self, data):
        os.write(self.master, data)
        self.drain(.15)
    def screen(self):
        return screen_text(self.raw)
    def finish(self, sig=None):
        if sig is None:
            self.send(b'q')
        else:
            os.kill(self.proc.pid, sig)
        self.wait(lambda: self.proc.poll() is not None, 'application did not exit', 12)
        self.drain(.1)
        return validate_terminal(bytes(self.raw), self.before, termios.tcgetattr(self.slave),
                                 self.raw_observed, self.proc.returncode, False)
    def close(self):
        # Kill only the session we created, including disposable Forge helpers.
        try:
            os.killpg(self.proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        self.proc.wait(timeout=5)
        (self.output/'terminal.bin').write_bytes(self.raw)
        (self.output/'screen.txt').write_text(self.screen(), encoding='utf-8')
        termios.tcsetattr(self.slave, termios.TCSANOW, self.before)
        os.close(self.master)
        os.close(self.slave)


def drawer(binary, output, home, name):
    env = environment(home, home/'server.log', 'drawer')
    env['SHELL'] = '/bin/sh'
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda: '1 total' in app.screen(), 'Registry did not load')
        app.send(b't')
        app.wait(lambda: 'Terminal' in app.screen(), 'Drawer did not open')
        # Octal escape keeps the output marker out of the echoed command.
        app.send(b"printf '\\104RAWER_READY\\n'\r")
        app.wait(lambda: 'DRAWER_READY' in app.screen(), 'Drawer shell did not execute')
        return {'mode':name, 'checks':app.finish(getattr(signal, name.split('-')[-1]))}
    finally:
        app.close()


def forge(binary, output, home):
    env = environment(home, home/'server.log', 'forge')
    shim = home/'bin/gh'
    shim.write_text(FORGE_MOCK, encoding='utf-8')
    shim.chmod(0o755)
    for args in (['init','-q'], ['-c','user.name=Fixture','-c','user.email=fixture@example.invalid',
                               'commit','--allow-empty','-m','fixture'],
                 ['remote','add','origin','https://github.com/fixture/isolated.git']):
        subprocess.run(['/usr/bin/git']+args, cwd=home, env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    app = Session(binary, output, home, env)
    try:
        log = home/'helpers.jsonl'
        app.wait(lambda: log.exists() and bool(log.read_text().strip()), 'Forge was not exercised')
        helper = json.loads(log.read_text().splitlines()[0])
        app.drain(6.3)
        links=[]
        for fd in Path('/proc/{}/fd'.format(app.proc.pid)).glob('*'):
            try: links.append(os.readlink(fd))
            except FileNotFoundError: pass
        require(Path('/proc/{}'.format(helper['child'])).exists(), 'helper did not retain writer')
        require(not Path('/proc/{}'.format(helper['pid'])).exists(), 'direct child was not reaped')
        require(helper['stdout'] not in links and helper['stderr'] not in links,
                'timed-out Forge readers still retain their exact pipes')
        return {'mode':'forge-pipes', 'checks':app.finish(), 'helper':helper,
                'readerPipesReleased':True, 'sampleAfterSeconds':6.3}
    finally:
        app.close()


def board(binary, output, home, interval):
    env = environment(home, home/'server.log', 'churn')
    env['NOISE_INTERVAL'] = str(interval)
    (home/'bin/codex').write_text(BOARD_MOCK, encoding='utf-8')
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda: '5000 total' in app.screen(), '5k Registry did not finish', 30)
        app.drain(.5)
        app.send(b'b')
        app.wait(lambda: 'Ready (5000)' in app.screen() and 'AAA_BEFORE' in app.screen(),
                 'Board baseline not observed', 15)
        (home/'start-churn').touch()
        started = time.monotonic()
        app.wait(lambda: 'AAA_AFTER' in app.screen(), 'Board starved during unrelated churn', 8)
        latency = time.monotonic()-started
        app.drain(.5)
        require('AAA_AFTER' in app.screen(), 'Board reverted to the old title')
        require(not (home/'stop-churn').exists(), 'churn stopped before success')
        app.send(b'\x1b')
        app.wait(lambda: 'Mission Control' in app.screen() and 'AAA_AFTER' in app.screen(),
                 'Registry/Board current-source cross-check failed')
        app.send(b'b')
        app.wait(lambda: 'Ready (5000)' in app.screen() and 'AAA_AFTER' in app.screen(),
                 'reentered Board is obsolete')
        (home/'stop-churn').touch()
        app.drain(.3)
        app.send(b'\x1b')
        return {'mode':'board-{}ms'.format(round(interval*1000)), 'checks':app.finish(),
                'rows':5000, 'updateVisibleSeconds':round(latency, 4),
                'sourceUpdateVisibleDuringChurn':True, 'crossViewCurrent':True}
    finally:
        (home/'stop-churn').touch()
        app.close()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--scenario')
    args=parser.parse_args()
    require(sys.platform=='linux', 'Linux controlling PTY required')
    binary=args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    receipt={'schema':'codex-tui/runtime-fault-regression/v1','status':'failed',
             'sourceSha':os.environ.get('CODEX_TUI_PTY_SOURCE_SHA'),
             'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
             'manualQualification':False, 'realSshQualification':False, 'checks':[]}
    cases=['drawer-SIGTERM','drawer-SIGHUP','drawer-SIGINT','forge-pipes','board-10ms','board-100ms']
    if args.scenario:
        require(args.scenario in cases, 'unknown scenario')
        cases=[args.scenario]
    try:
        for name in cases:
            with tempfile.TemporaryDirectory(prefix='codex-tui-runtime-') as directory:
                try:
                    home=Path(directory)
                    output=args.output/name
                    if name.startswith('drawer-'): result=drawer(binary,output,home,name)
                    elif name=='forge-pipes': result=forge(binary,output,home)
                    else: result=board(binary,output,home,.01 if name=='board-10ms' else .1)
                    result['passed']=True
                except Exception as error:
                    result={'mode':name,'passed':False,'error':str(error)}
                receipt['checks'].append(result)
                print(json.dumps(result), flush=True)
        require(all(case['passed'] for case in receipt['checks']), 'runtime regression failed')
        receipt['status']='passed'
    finally:
        (args.output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')

if __name__=='__main__':
    main()
