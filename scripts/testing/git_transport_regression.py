#!/usr/bin/env python3
"""Exact-program Git identity/budget and stdio ownership regressions, Linux mocks only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from runtime_fault_regression import Session, environment
from app_server_transport_regression import require

GIT = '/usr/bin/git'
SEARCH_MOCK = r'''#!/usr/bin/python3
import json,os,subprocess,sys
from pathlib import Path
home=Path(os.environ['HOME']);log=home/'sessions.jsonl'
index=len(log.read_text().splitlines()) if log.exists() else 0
held=os.environ.get('HOLD')=='1' and (index>0 or os.environ.get('STARTUP')=='1')
child=subprocess.Popen(['/usr/bin/python3','-c','import time;time.sleep(90)'],stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL) if held else None
with log.open('a') as out:
 out.write(json.dumps({'pid':os.getpid(),'child':child.pid if child else None,'stderr':os.readlink('/proc/self/fd/2'),'index':index})+'\n')
def emit(value):print(json.dumps(value),flush=True)
for line in sys.stdin:
 request=json.loads(line);method=request.get('method');ident=request.get('id')
 if ident is None:continue
 if os.environ.get('STARTUP')=='1':
  os.write(1,b'{invalid-json}\n');sys.exit(0)
 if method=='initialize':out={'serverInfo':{'name':'isolated-ownership-fixture','version':'0.0.0'}}
 elif method=='thread/list':out={'data':[{'id':'audit-thread','name':'AUDIT_HEALTHY','cwd':str(home),'status':{'type':'idle'}}],'nextCursor':None}
 elif method=='thread/loaded/list':out={'data':['audit-thread'],'nextCursor':None}
 elif method=='thread/search':
  query=request['params']['searchTerm']
  with (home/'searches.log').open('a') as outlog:outlog.write(query+'\n')
  if query=='stall':continue
  out={'data':[],'nextCursor':None}
 else:
  emit({'id':ident,'error':{'code':-32601,'message':'unsupported local fixture'}});continue
 emit({'id':ident,'result':out})
'''
GIT_MOCK = r'''#!/usr/bin/python3
import os,sys
from pathlib import Path
args=sys.argv[1:];home=Path(os.environ['HOME']);mode=os.environ['GIT_CASE']
if '--show-toplevel' in args:
 if mode=='root-mismatch':print(str(home/'sibling'));sys.exit(0)
 if mode=='identity-nonutf8':os.write(1,os.fsencode(home)+b'/bad\xff\n');sys.exit(0)
 if mode.startswith('stderr-'):os.write(2,b'x'*(65536+(mode=='stderr-over')))
 print(str(home))
elif '--git-common-dir' in args:print(str(home/'.git'))
elif 'status' in args:
 if mode.startswith('stdout-'):
  size=2*1024*1024+(mode=='stdout-over');os.write(1,b'? '+b'x'*(size-3)+b'\0')
 elif mode.startswith('entries-'):
  for i in range(10000+(mode=='entries-over')):os.write(1,('? file-%d\0'%i).encode())
 elif mode=='status-no-nul':os.write(1,b'? incomplete')
 elif mode=='status-nonutf8':os.write(1,b'? bad\xff\0')
 else:os.write(1,b'? healthy\0')
else:sys.exit(1)
'''
FANOUT_MOCK = r'''#!/usr/bin/python3
import json,os,sys
rows=[{'id':'t-%d'%i,'name':'THREAD-%03d'%i,'cwd':os.environ['DATA_REPO'],'status':{'type':'idle'}} for i in range(int(os.environ['NTHREADS']))]
for line in sys.stdin:
 r=json.loads(line);m=r.get('method');i=r.get('id')
 if i is None:continue
 if m=='initialize':out={'serverInfo':{'name':'local-git-fanout','version':'0.0.0'}}
 elif m=='thread/list':out={'data':rows,'nextCursor':None}
 elif m=='thread/loaded/list':out={'data':[rows[0]['id']],'nextCursor':None}
 else:
  print(json.dumps({'id':i,'error':{'code':-32601,'message':'unsupported'}}),flush=True);continue
 print(json.dumps({'id':i,'result':out}),flush=True)
'''


def editor(binary, output, home, variant):
    suffix = {'plain':'', 'trailing':'  ', 'newline':'\nline', 'cr':'\r', 'nested':''}[variant]
    repo = home / ('repo' + suffix)
    repo.mkdir()
    subprocess.run([GIT, 'init', '-q', str(repo)], check=True)
    (repo/'changed.txt').write_text('initial\n')
    subprocess.run([GIT, '-C', str(repo), 'add', 'changed.txt'], check=True)
    subprocess.run([GIT, '-C', str(repo), '-c', 'user.name=Fixture', '-c',
                    'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture'], check=True)
    (repo/'changed.txt').write_text('CORRECT-TARGET-CHANGE\n')
    if suffix:
        (home/'repo').mkdir()
        (home/'repo/changed.txt').write_text('WRONG-SIBLING-FILE\n')
    cwd = repo
    if variant == 'nested':
        cwd = repo/'nested'
        cwd.mkdir()
    env = environment(home, home/'server.log', 'audit')
    env['REVIEW_CWD'] = str(cwd)
    mock = home/'bin/codex'
    mock.write_text(mock.read_text().replace("'cwd':os.environ['HOME']", "'cwd':os.environ['REVIEW_CWD']"))
    probe = home/'bin/editor-probe'
    probe.write_text("#!/usr/bin/python3\nimport sys,os,json\nfrom pathlib import Path\nPath(os.environ['HOME'],'editor.json').write_text(json.dumps({'argv':sys.argv[1:],'content':Path(sys.argv[1]).read_text()}))\n")
    probe.chmod(0o755)
    env['CODEX_TUI_EDITOR'] = str(probe)
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda: '1 total' in app.screen(), 'Registry did not load')
        app.drain(.5)
        app.send(b'r')
        app.wait(lambda: 'changed.txt' in app.screen(), 'Review change list did not load')
        app.send(b'e')
        app.wait(lambda: (home/'editor.json').exists(), 'Editor did not launch')
        call = json.loads((home/'editor.json').read_text())
        (output/'editor.json').write_text(json.dumps(call, indent=2)+'\n')
        require(call['argv'] == [str((repo/'changed.txt').resolve())], 'Editor opened a different exact path')
        require(call['content'] == 'CORRECT-TARGET-CHANGE\n', 'Editor read the sibling instead')
        app.send(b'\x1b')
        return {'editor':call, 'terminal':app.finish()}
    finally:
        app.close()


def git_budget(binary, output, home, mode):
    env = environment(home, home/'server.log', 'audit')
    (home/'.git').mkdir()
    (home/'sibling').mkdir()
    env['GIT_CASE'] = mode
    mock = home/'bin/git'
    mock.write_text(GIT_MOCK)
    mock.chmod(0o755)
    output.mkdir(parents=True)
    result = subprocess.run([str(binary), 'doctor', 'git'], cwd=home, env=env,
                            capture_output=True, timeout=12)
    (output/'stdout.txt').write_bytes(result.stdout)
    (output/'stderr.txt').write_bytes(result.stderr)
    healthy = mode.endswith('-exact')
    require(result.returncode == (0 if healthy else 1), 'Git budget/identity exit contract: '+str(result.returncode))
    if healthy:
        count = 10000 if mode == 'entries-exact' else 1
        require(('changed-files: %d'%count).encode() in result.stdout, 'Healthy boundary did not preserve complete inventory')
    else:
        require(b'git-repository: true' not in result.stdout, 'Invalid probe claimed healthy authority')
        category = {'root-mismatch':b'does not contain current directory',
                    'identity-nonutf8':b'not UTF-8', 'stderr-over':b'budget',
                    'stdout-over':b'budget', 'entries-over':b'entry budget',
                    'status-no-nul':b'final NUL', 'status-nonutf8':b'not UTF-8'}[mode]
        require(category in result.stderr, 'Failure did not report the expected Git diagnostic')
    return {'exitCode':result.returncode, 'healthyBoundary':healthy}


def links(pid):
    values = []
    for fd in Path('/proc/%d/fd'%pid).glob('*'):
        try: values.append(os.readlink(fd))
        except FileNotFoundError: pass
    return values


def transport(binary, output, home, variant):
    env = environment(home, home/'server.log', 'audit')
    held = variant not in ('startup-eof', 'search-eof')
    env.update(HOLD=str(int(held)), STARTUP=str(int(variant.startswith('startup'))))
    (home/'bin/codex').write_text(SEARCH_MOCK)
    app = Session(binary, output, home, env)
    samples = []
    try:
        def inventory():
            log = home/'sessions.jsonl'
            return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        if variant.startswith('startup'):
            app.wait(lambda: bool(inventory()), 'Initialization did not run')
            infos = inventory()
            app.wait(lambda: all(not Path('/proc/%d'%item['pid']).exists() for item in infos), 'Failed initializer not released')
            app.wait(lambda: all(item['stderr'] not in links(app.proc.pid) for item in infos), 'Failed initialization retained stderr pipe', 3)
            if held: require(Path('/proc/%d'%infos[0]['child']).exists(), 'Held writer did not remain alive')
        else:
            app.wait(lambda: '1 total' in app.screen(), 'Primary Registry did not load')
            def search(query):
                app.send(b'\x06')
                app.send((query+'\r').encode())
                app.wait(lambda: (home/'searches.log').exists() and query in (home/'searches.log').read_text().splitlines(), 'Search was not exercised')
            if variant == 'search-replaced':
                search('stall')
                app.send(b'\x1b')
            for index in range(1, 6):
                search('query%d'%index)
                infos = inventory()[1:]
                app.wait(lambda: all(not Path('/proc/%d'%item['pid']).exists() for item in infos), 'Search connection not dropped')
                app.wait(lambda: all(item['stderr'] not in links(app.proc.pid) for item in infos), 'Completed search retained stderr pipes', 3)
                if held: require(all(Path('/proc/%d'%item['child']).exists() for item in infos), 'Fixture writer unexpectedly exited')
                samples.append({'completedSearches':index, 'connections':len(infos), 'heldStderrPipes':0, 'fdCount':len(links(app.proc.pid))})
                app.send(b'\x1b')
        return {'samples':samples, 'terminal':app.finish(), 'readersReleased':True}
    finally:
        if (home/'sessions.jsonl').exists():
            (output/'sessions.jsonl').write_bytes((home/'sessions.jsonl').read_bytes())
        app.close()


def fanout(binary, output, home):
    samples = []
    output.mkdir(parents=True)
    for count in (1, 32):
        h = home/str(count)
        h.mkdir()
        repo = h/'repo'
        repo.mkdir()
        subprocess.run([GIT, 'init', '-q', str(repo)], check=True)
        for index in range(2000): (repo/('%04d-'%index+'x'*200)).touch()
        env = environment(h, h/'server.log', 'audit')
        env.update(DATA_REPO=str(repo), NTHREADS=str(count))
        (h/'bin/codex').write_text(FANOUT_MOCK)
        app = Session(binary, output/str(count), h, env)
        try:
            app.wait(lambda: ('%d total'%count) in app.screen(), 'Fanout Registry did not load')
            app.drain(1.5)
            app.send(b'r')
            app.wait(lambda: '0000-' in app.screen(), 'Real Git detail not ready')
            fields = dict(line.split(':',1) for line in Path('/proc/%d/status'%app.proc.pid).read_text().splitlines() if ':' in line)
            app.send(b'\x1b')
            samples.append({'threads':count, 'changedFiles':2000, 'peakRSSKiB':int(fields['VmHWM'].split()[0]), 'terminal':app.finish()})
        finally: app.close()
    return {'samples':samples, 'rssIsDiagnosticOnly':True}


CASES = (['editor-'+name for name in ['plain','trailing','newline','cr','nested']]
         + ['git-'+name for name in ['root-mismatch','identity-nonutf8','stderr-exact','stderr-over',
                                     'stdout-exact','stdout-over','entries-exact','entries-over',
                                     'status-no-nul','status-nonutf8']]
         + ['transport-'+name for name in ['startup-eof','startup-held','search-eof','search-held','search-replaced']]
         + ['git-fanout'])

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--scenario', choices=CASES)
    args = parser.parse_args()
    require(sys.platform == 'linux', 'Linux PTY and /proc required')
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema':'codex-tui/git-transport-regression/v1', 'status':'failed',
               'sourceSha':os.environ.get('CODEX_TUI_PTY_SOURCE_SHA'),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'manualQualification':False, 'realCodexQualification':False,
               'realSshQualification':False, 'checks':[]}
    try:
        for name in ([args.scenario] if args.scenario else CASES):
            with tempfile.TemporaryDirectory(prefix='ctui-git-transport-') as temporary:
                home = Path(temporary)
                output = args.output/name
                try:
                    if name.startswith('editor-'): result = editor(binary, output, home, name[7:])
                    elif name.startswith('transport-'): result = transport(binary, output, home, name[10:])
                    elif name == 'git-fanout': result = fanout(binary, output, home)
                    else: result = git_budget(binary, output, home, name[4:])
                    result.update(mode=name, passed=True)
                except Exception as error: result = {'mode':name, 'passed':False, 'error':str(error)}
                receipt['checks'].append(result)
                print(json.dumps(result), flush=True)
        require(all(case['passed'] for case in receipt['checks']), 'Git/transport regression failed')
        receipt['status'] = 'passed'
    finally:
        (args.output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')

if __name__ == '__main__': main()
