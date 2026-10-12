#!/usr/bin/env python3
"""Actual Linux program: lossless Git targets, capture budgets, owned stderr.

All repositories, commands and descendants are disposable local fixtures. Nothing
uses a real account. Assertions remain active under Python -O.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from runtime_fault_regression import Session, environment
from app_server_transport_regression import require

GIT_MOCK = r'''#!/usr/bin/python3
import os,sys
kind=os.environ['BUDGET_KIND']; size=int(os.environ['BUDGET_SIZE'])
args=sys.argv[1:]
def write(fd, data):
 while data:
  n=os.write(fd,data[:8192]);data=data[n:]
if 'rev-parse' in args:
 if kind=='stderr' and '--show-toplevel' in args: write(2,b'x'*size)
 print(os.getcwd()+('/.git' if '--git-common-dir' in args else ''),flush=True)
elif 'status' in args:
 if kind=='stdout': write(1,b'? '+b'x'*(size-3)+b'\0')
 elif kind=='entries': write(1,b'? x\0'*size)
 elif kind=='partial': write(1,b'? unfinished')
 elif kind=='utf8': write(1,b'? \xff\0')
else: sys.exit(1)
'''
SEARCH_MOCK = r'''#!/usr/bin/python3
import json,os,subprocess,sys,time
from pathlib import Path
home=Path(os.environ['HOME']);log=home/'sessions.jsonl'
index=len(log.read_text().splitlines()) if log.exists() else 0
child=None
if index and os.environ['HOLD']=='1':
 child=subprocess.Popen(['/usr/bin/python3','-c','import time;time.sleep(60)'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL)
with log.open('a') as f:
 f.write(json.dumps({'pid':os.getpid(),'child':child.pid if child else None,'stderr':os.readlink('/proc/self/fd/2'),'index':index})+'\n')
def emit(v):print(json.dumps(v),flush=True)
for line in sys.stdin:
 r=json.loads(line);m=r.get('method');i=r.get('id')
 if i is None:continue
 if m=='initialize':out={'serverInfo':{'name':'owned-stderr-fixture','version':'0.0.0'}}
 elif m=='thread/list':out={'data':[{'id':'audit-thread','name':'AUDIT_HEALTHY','cwd':str(home),'status':{'type':'idle'}}],'nextCursor':None}
 elif m=='thread/loaded/list':out={'data':['audit-thread'],'nextCursor':None}
 elif m=='thread/search':
  term=r['params']['searchTerm']
  with (home/'searches.log').open('a') as f:f.write(term+'\n')
  if term.startswith('slow'):time.sleep(60)
  out={'data':[],'nextCursor':None}
 else:
  emit({'id':i,'error':{'code':-32601,'message':'unsupported fixture'}});continue
 emit({'id':i,'result':out})
'''
INIT_MOCK = r'''#!/usr/bin/python3
import json,os,subprocess,sys,time
from pathlib import Path
json.loads(sys.stdin.readline());child=None
if os.environ['HOLD']=='1':
 child=subprocess.Popen(['/usr/bin/python3','-c','import time;time.sleep(30)'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL)
Path(os.environ['HOME'],'pipe.json').write_text(json.dumps({'pid':os.getpid(),'child':child.pid if child else None,'stderr':os.readlink('/proc/self/fd/2')}))
os.write(1,b'{invalid-json}\n');time.sleep(.1)
'''


def links(pid):
    result = []
    for fd in Path('/proc/%d/fd' % pid).glob('*'):
        try:
            result.append(os.readlink(fd))
        except FileNotFoundError:
            pass
    return result


def gone(pid):
    return not Path('/proc/%d' % pid).exists()


def git(repo, args):
    subprocess.run(['/usr/bin/git', '-C', str(repo)] + args, check=True,
                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10)


def editor(binary, output, home, mode):
    suffix = {'editor-normal':'', 'editor-spaces':'  ', 'editor-newline':'\nline',
              'editor-linked':'  '}[mode]
    root = home / ('repo' + suffix)
    root.mkdir()
    git(root, ['init', '-q'])
    target_name = 'changed file.txt'
    (root / target_name).write_text('initial\n')
    git(root, ['add', '--', target_name])
    git(root, ['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
               'commit', '-qm', 'fixture'])
    if mode == 'editor-linked':
        linked = home / 'linked  '
        git(root, ['worktree', 'add', '--detach', str(linked)])
        root = linked
        (root / 'nested').mkdir()
    cwd = root / 'nested' if mode == 'editor-linked' else root
    target = root / target_name
    target.write_text('correct-file-target-change\n')
    if suffix:
        sibling = home / ('linked' if mode == 'editor-linked' else 'repo')
        sibling.mkdir()
        (sibling / target_name).write_text('WRONG-SIBLING-FILE\n')
    env = environment(home, home/'server.log', 'audit')
    env['REVIEW_CWD'] = str(cwd)
    mock = home/'bin/codex'
    mock.write_text(mock.read_text().replace("'cwd':os.environ['HOME']", "'cwd':os.environ['REVIEW_CWD']"))
    helper = home/'bin/editor-probe'
    helper.write_text("#!/usr/bin/python3\nimport sys,os,json\nfrom pathlib import Path\nPath(os.environ['HOME'],'editor.json').write_text(json.dumps({'argv':sys.argv[1:],'content':Path(sys.argv[1]).read_text()}))\n")
    helper.chmod(0o755)
    env['CODEX_TUI_EDITOR'] = str(helper)
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda: '1 total' in app.screen(), 'Registry did not load')
        app.drain(.6)
        app.send(b'r')
        app.wait(lambda: 'correct-file-target-change' in app.screen(), 'correct Review diff missing', 12)
        app.send(b'e')
        app.wait(lambda: (home/'editor.json').exists(), 'editor not launched', 8)
        observed = json.loads((home/'editor.json').read_text())
        (output/'editor.json').write_text(json.dumps(observed))
        require(observed['argv'] == [str(target.resolve())], 'editor target identity was rewritten')
        require(observed['content'] == 'correct-file-target-change\n', 'editor opened sibling file')
        app.send(b'\x1b')
        return {'argv':observed['argv'], 'terminalExit':app.finish()}
    finally:
        app.close()


def budget(binary, output, home, mode):
    kind, size, expected, count = {
        'stderr-exact':('stderr', 65536, True, 0),
        'stderr-over':('stderr', 65537, False, None),
        'stdout-exact':('stdout', 2*1024*1024, True, 1),
        'stdout-over':('stdout', 2*1024*1024+1, False, None),
        'entries-exact':('entries', 10000, True, 10000),
        'entries-over':('entries', 10001, False, None),
        'partial-record':('partial', 0, False, None),
        'invalid-utf8':('utf8', 0, False, None),
    }[mode]
    env = environment(home, home/'server.log', 'audit')
    (home/'.git').mkdir()
    env.update(BUDGET_KIND=kind, BUDGET_SIZE=str(size))
    shim = home/'bin/git'
    shim.write_text(GIT_MOCK)
    shim.chmod(0o755)
    run = subprocess.run([str(binary), 'doctor', 'git'], cwd=home, env=env,
                         capture_output=True, timeout=10)
    output.mkdir()
    (output/'stdout.txt').write_bytes(run.stdout)
    (output/'stderr.txt').write_bytes(run.stderr)
    require((run.returncode == 0) == expected, 'Git budget/integrity exit contract violated')
    if expected:
        require(('changed-files: %d' % count).encode() in run.stdout, 'complete inventory count lost')
    else:
        require(b'git-repository: true' not in run.stdout, 'partial Git result published as authoritative')
        require(bool(run.stderr), 'refusal diagnosis missing')
    return {'exitCode':run.returncode, 'kind':kind, 'size':size}


def initial_stderr(binary, output, home, mode):
    hold = mode == 'init-held'
    env = environment(home, home/'server.log', 'audit')
    env['HOLD'] = str(int(hold))
    (home/'bin/codex').write_text(INIT_MOCK)
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda: (home/'pipe.json').exists(), 'server fixture not started')
        info = json.loads((home/'pipe.json').read_text())
        app.wait(lambda: gone(info['pid']), 'failed server not reaped')
        app.wait(lambda: info['stderr'] not in links(app.proc.pid), 'failed connection retained stderr reader', 3)
        if hold:
            require(not gone(info['child']), 'writer helper exited before reader check')
        return {'fixture':info, 'readerReleased':True, 'terminalExit':app.finish()}
    finally:
        app.close()


def search_stderr(binary, output, home, mode):
    hold = mode != 'search-eof'
    replace = mode == 'search-replaced'
    env = environment(home, home/'server.log', 'audit')
    env['HOLD'] = str(int(hold))
    (home/'bin/codex').write_text(SEARCH_MOCK)
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda: '1 total' in app.screen(), 'primary not loaded')
        app.drain(.4)
        samples = []
        if replace:
            app.send(b'\x06')
            app.send(b'slow-first\r')
            app.wait(lambda: (home/'searches.log').exists(), 'slow search not started')
        for n in range(1, 6):
            app.send(b'\x06')
            term = 'query%d' % n
            app.send((term+'\r').encode())
            app.wait(lambda: (home/'searches.log').exists() and term in (home/'searches.log').read_text(), 'search not executed')
            info = [json.loads(line) for line in (home/'sessions.jsonl').read_text().splitlines()][1:]
            app.wait(lambda: all(gone(i['pid']) for i in info), 'auxiliary server not released')
            app.wait(lambda: all(i['stderr'] not in links(app.proc.pid) for i in info), 'completed/cancelled search retained stderr reader', 3)
            if hold:
                require(all(not gone(i['child']) for i in info), 'writer helper died before reader test')
            samples.append({'completed':n, 'retainedPipes':0, 'fdCount':len(links(app.proc.pid)), 'sessions':info})
            app.send(b'\x1b')
        return {'samples':samples, 'terminalExit':app.finish()}
    finally:
        app.close()


CASES = [(n, editor) for n in ['editor-normal','editor-spaces','editor-newline','editor-linked']]
CASES += [(n, budget) for n in ['stderr-exact','stderr-over','stdout-exact','stdout-over',
                              'entries-exact','entries-over','partial-record','invalid-utf8']]
CASES += [(n, initial_stderr) for n in ['init-eof','init-held']]
CASES += [(n, search_stderr) for n in ['search-eof','search-held','search-replaced']]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--case', choices=[name for name, _ in CASES])
    args = parser.parse_args()
    binary = args.binary.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    cases = []
    for name, fn in CASES:
        if args.case and name != args.case:
            continue
        case = {'name':name, 'passed':False}
        try:
            with tempfile.TemporaryDirectory(prefix='ctui-git-resource-') as td:
                case['observed'] = fn(binary, args.output/name, Path(td), name)
            case['passed'] = True
        except Exception as error:
            case['error'] = str(error)
        cases.append(case)
        print(json.dumps(case), flush=True)
    passed = all(case['passed'] for case in cases)
    receipt = {'schema':'codex-tui/git-resource-regression/v1',
               'sourceSha':os.environ.get('CODEX_TUI_PTY_SOURCE_SHA', 'unknown'),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'status':'passed' if passed else 'failed', 'cases':cases,
               'manualQualification':False, 'realAccountQualification':False}
    (args.output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
