#!/usr/bin/env python3
"""Actual Linux program regressions: exact editor targets and bounded owned I/O.

No account, network, publication, or real-SSH qualification. All helper processes
and files belong to an isolated disposable controlling terminal session.
"""
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

GIT_MOCK = r'''#!/usr/bin/python3
import os, sys
from pathlib import Path
mode = os.environ['PROBE_KIND']
args = sys.argv[1:]
if 'rev-parse' in args:
    if '--show-toplevel' in args and mode.startswith('stderr'):
        size = 16384 if mode == 'stderr-control' else 8 * 1024 * 1024
        block = b'x' * 16384
        for _ in range(size // len(block)):
            os.write(2, block)
    print(str(Path.cwd()) + ('/.git' if '--git-common-dir' in args else ''), flush=True)
elif 'status' in args:
    size = {'status-control': 10, 'status-records': 8193, 'status-bytes': 10000}.get(mode, 0)
    for index in range(size):
        path = ('%06d' % index) + ('x' * 240 if mode == 'status-bytes' else '')
        os.write(1, b'? ' + path.encode() + b'\0')
else:
    sys.exit(1)
'''

SEARCH_MOCK = r'''#!/usr/bin/python3
import json, os, subprocess, sys
from pathlib import Path
home = Path(os.environ['HOME'])
log = home / 'sessions.jsonl'
index = len(log.read_text().splitlines()) if log.exists() else 0
child = None
if os.environ['HOLD'] == '1' and (index or os.environ.get('BROKEN_STARTUP') == '1'):
    child = subprocess.Popen(['/usr/bin/python3', '-c', 'import time;time.sleep(60)'],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL)
with log.open('a') as out:
    out.write(json.dumps({'pid':os.getpid(), 'child':child.pid if child else None,
                          'stderr':os.readlink('/proc/self/fd/2'), 'index':index}) + '\n')
def emit(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    method, ident = request.get('method'), request.get('id')
    if ident is None:
        continue
    if os.environ.get('BROKEN_STARTUP') == '1':
        print('{invalid-json}', flush=True)
        break
    if method == 'initialize':
        result = {'serverInfo':{'name':'local-lifecycle-fixture','version':'0.0.0'}}
    elif method == 'thread/list':
        result = {'data':[{'id':'audit-thread','name':'AUDIT_HEALTHY','cwd':str(home),
                           'status':{'type':'idle'}}], 'nextCursor':None}
    elif method == 'thread/loaded/list':
        result = {'data':['audit-thread'], 'nextCursor':None}
    elif method == 'thread/search':
        result = {'data':[], 'nextCursor':None}
        with (home/'searches.log').open('a') as out:
            out.write(request['params']['searchTerm']+'\n')
    else:
        emit({'id':ident,'error':{'code':-32601,'message':'unsupported in fixture'}})
        continue
    emit({'id':ident,'result':result})
'''


def check_editor(call, expected):
    require(call.get('argv') == [str(expected)], 'editor received a different file identity')
    require(call.get('content') == 'CORRECT-FILE\n', 'editor opened the wrong file content')


def check_budget(result, should_succeed):
    if should_succeed:
        require(result.returncode == 0, 'healthy Git control was rejected')
        require(b'git-repository: true' in result.stdout, 'healthy repository was not reported')
    else:
        require(result.returncode != 0, 'over-budget Git probe was reported successful')
        require(b'budget' in result.stdout + result.stderr, 'probe failed for an unrelated reason')


def pipe_links(pid):
    result = []
    for fd in Path('/proc/{}/fd'.format(pid)).glob('*'):
        try:
            result.append(os.readlink(fd))
        except (FileNotFoundError, ProcessLookupError):
            pass
    return result


def check_readers(samples, expected):
    require(len(samples) == expected, 'missing reader-lifecycle observations')
    for item in samples:
        require(item['auxiliaryProcessesGone'], 'auxiliary process has not been released')
        require(item['heldStderrPipes'] == 0, 'released connection still owns its exact stderr pipe')
        require(item['helperConditionHeld'], 'writer-lifetime control was not exercised')


def editor(binary, output, home, mode):
    suffix = {'editor-control':'', 'editor-spaces':'  ', 'editor-newline':'\nline'}[mode]
    repo = home / ('repo' + suffix)
    repo.mkdir()
    subprocess.run(['/usr/bin/git','init','-q',str(repo)], check=True)
    (repo/'changed.txt').write_text('initial\n')
    subprocess.run(['/usr/bin/git','-C',str(repo),'add','changed.txt'], check=True)
    subprocess.run(['/usr/bin/git','-C',str(repo),'-c','user.name=Fixture',
                    '-c','user.email=fixture@example.invalid','commit','-qm','init'], check=True)
    (repo/'changed.txt').write_text('CORRECT-FILE\n')
    if suffix:
        (home/'repo').mkdir()
        (home/'repo/changed.txt').write_text('WRONG-SIBLING\n')
    env = environment(home, home/'server.log', 'audit')
    env['REVIEW_CWD'] = str(repo)
    mock = home/'bin/codex'
    original = mock.read_text()
    require(original.count("'cwd':os.environ['HOME']") == 1, 'shared fixture contract changed')
    mock.write_text(original.replace("'cwd':os.environ['HOME']", "'cwd':os.environ['REVIEW_CWD']"))
    probe = home/'bin/editor-probe'
    probe.write_text("#!/usr/bin/python3\nimport sys,os,json\nfrom pathlib import Path\n"
                     "call={'argv':sys.argv[1:],'content':Path(sys.argv[1]).read_text()}\n"
                     "Path(os.environ['HOME'],'editor-call.json').write_text(json.dumps(call))\n")
    probe.chmod(0o755)
    env['CODEX_TUI_EDITOR'] = str(probe)
    app = Session(binary, output, home, env)
    try:
        app.wait(lambda:'1 total' in app.screen(), 'Registry did not load')
        app.drain(.7)
        app.send(b'r')
        app.wait(lambda:'changed.txt' in app.screen() and 'CORRECT-FILE' in app.screen(),
                 'Review did not show the real Git diff')
        app.send(b'e')
        call_path = home/'editor-call.json'
        app.wait(call_path.exists, 'editor did not launch')
        call = json.loads(call_path.read_text())
        (output/'editor-call.json').write_text(json.dumps(call, indent=2)+'\n')
        app.send(b'\x1b')
        restored = app.finish()
        check_editor(call, repo/'changed.txt')
        return {'expectedPath':str(repo/'changed.txt'), 'editor':call, 'terminal':restored}
    finally:
        app.close()


def budget(binary, output, home, mode):
    output.mkdir()
    env = environment(home, home/'server.log', 'audit')
    (home/'.git').mkdir()
    env['PROBE_KIND'] = mode
    shim = home/'bin/git'
    shim.write_text(GIT_MOCK)
    shim.chmod(0o755)
    result = subprocess.run([str(binary),'doctor','git'], cwd=home, env=env,
                            capture_output=True, timeout=10)
    (output/'stdout.txt').write_bytes(result.stdout)
    (output/'stderr.txt').write_bytes(result.stderr)
    check_budget(result, mode.endswith('-control'))
    return {'exitCode':result.returncode, 'overBudgetRejected':not mode.endswith('-control')}


def readers(binary, output, home, mode):
    env = environment(home, home/'server.log', 'audit')
    held, startup = mode != 'search-eof-control', mode == 'startup-held'
    env.update(HOLD=str(int(held)), BROKEN_STARTUP=str(int(startup)))
    (home/'bin/codex').write_text(SEARCH_MOCK)
    app = Session(binary, output, home, env)
    log = home/'sessions.jsonl'
    samples = []
    try:
        if startup:
            app.wait(log.exists, 'initial App Server did not start')
        else:
            app.wait(lambda:'1 total' in app.screen(), 'primary App Server did not connect')
            app.drain(.4)
        for query in range(1, 2 if startup else 6):
            if not startup:
                text = 'query{}'.format(query)
                app.send(b'\x06')
                app.send((text+'\r').encode())
                searches = home/'searches.log'
                app.wait(lambda:searches.exists() and text in searches.read_text().splitlines(),
                         'normal isolated search did not execute')
            app.drain(.4)
            infos = [json.loads(line) for line in log.read_text().splitlines()]
            auxiliary = infos if startup else infos[1:]
            require(len(auxiliary) == query, 'search did not create the expected isolated connection')
            gone = lambda:all(not Path('/proc/{}'.format(i['pid'])).exists() for i in auxiliary)
            app.wait(gone, 'auxiliary App Server still running', 5)
            # Cancellation is scheduled by JoinSet::drop, not synchronously finished.
            # Poll for a bounded interval while retaining the writer-lifetime control.
            for _ in range(20):
                links = pipe_links(app.proc.pid)
                if all(i['stderr'] not in links for i in auxiliary):
                    break
                app.drain(.05)
            helpers = sum(i['child'] is not None and Path('/proc/{}'.format(i['child'])).exists()
                          for i in auxiliary)
            samples.append({'completed':query, 'auxiliaryProcessesGone':gone(),
                            'heldStderrPipes':sum(i['stderr'] in links for i in auxiliary),
                            'helperConditionHeld':helpers == (len(auxiliary) if held else 0),
                            'fdCount':len(links)})
            if not startup:
                require(Path('/proc/{}'.format(infos[0]['pid'])).exists(), 'primary connection was terminated')
                app.send(b'\x1b')
        (output/'samples.json').write_text(json.dumps(samples, indent=2)+'\n')
        restored = app.finish()
        check_readers(samples, 1 if startup else 5)
        return {'samples':samples, 'terminal':restored}
    finally:
        app.close()


CASES = ['editor-control', 'editor-spaces', 'editor-newline', 'stderr-control',
         'stderr-overflow', 'status-control', 'status-bytes', 'status-records',
         'startup-held', 'search-held', 'search-eof-control']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--scenario', choices=CASES)
    args = parser.parse_args()
    require(sys.platform == 'linux', 'Linux controlling PTY required')
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    receipt = {'schema':'codex-tui/git-lifecycle-regression/v1', 'status':'failed',
               'sourceSha':os.environ.get('CODEX_TUI_PTY_SOURCE_SHA'),
               'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
               'manualQualification':False, 'realSshQualification':False, 'checks':[]}
    cases = [args.scenario] if args.scenario else CASES
    try:
        for mode in cases:
            try:
                with tempfile.TemporaryDirectory(prefix='codex-tui-git-lifecycle-') as directory:
                    home, output = Path(directory), args.output/mode
                    if mode.startswith('editor-'):
                        data = editor(binary, output, home, mode)
                    elif mode.startswith(('startup-', 'search-')):
                        data = readers(binary, output, home, mode)
                    else:
                        data = budget(binary, output, home, mode)
                    case = dict(data, mode=mode, passed=True)
            except Exception as error:
                case = {'mode':mode, 'passed':False, 'error':str(error)}
            receipt['checks'].append(case)
            print(json.dumps(case), flush=True)
        if all(item['passed'] for item in receipt['checks']):
            receipt['status'] = 'passed'
    finally:
        (args.output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n')
    return 0 if receipt['status'] == 'passed' else 1


if __name__ == '__main__':
    sys.exit(main())
