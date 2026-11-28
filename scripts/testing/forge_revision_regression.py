#!/usr/bin/env python3
"""Real Linux PTY: retired Forge write routes, retained review and external handoff."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tempfile
import time

from app_server_transport_regression import require
from runtime_fault_regression import Session, environment
from watch_ownership_regression import identity, resources

SHA_A = 'a' * 40
SHA_B = 'b' * 40
PROVIDERS = ('github', 'gitlab')
CASES = ['{}-approve-{}'.format(p, mode)
         for p in PROVIDERS for mode in ('control', 'missing')]

# No network client is invoked. Unexpected commands fail the fixture rather than
# delegating to installed gh/glab. A file lock serializes separate CLI processes.
FORGE = r'''#!/usr/bin/python3
import fcntl, json, os, sys
from pathlib import Path
from urllib.parse import urlsplit
home = Path(os.environ['HOME'])
args = sys.argv[1:]
provider, _, mode = os.environ['FORGE_CASE'].split('-', 2)
# Record every invocation, including attempts that fail argument validation.
with open(home/'forge-invocations.jsonl', 'a') as log:
 log.write(json.dumps(args)+'\n')
if args == ['--version']:
 print('isolated-forge 0.0.0'); sys.exit(0)
if args[:2] == ['auth','status']:
 sys.exit(0)
if not args or args[0] != 'api':
 sys.exit(90)
method = args[args.index('--method')+1] if '--method' in args else 'GET'
endpoints = [a for a in args if a.startswith('/')]
if len(endpoints) != 1: sys.exit(91)
endpoint = endpoints[0]; path = urlsplit(endpoint).path
fields = dict(a.split('=',1) for a in args if '=' in a and a not in endpoints)
with open(home/'forge.lock', 'a') as lock:
 fcntl.flock(lock, fcntl.LOCK_EX)
 sha = None if mode == 'missing' else 'a' * 40
 merged = False
 repo = {'id':123,'full_name':'fixture/isolated','html_url':'https://github.com/fixture/isolated',
         'path_with_namespace':'fixture/isolated','web_url':'https://gitlab.com/fixture/isolated',
         'default_branch':'main'}
 if provider == 'github':
  repo.update({'allow_merge_commit':True,'allow_squash_merge':False,'allow_rebase_merge':False})
 review = {'id':80,'state':'APPROVED','user':{'id':7,'login':'fixture-user'},
           'commit_id':'a'*40}
 pull = {'number':7,'title':'FIXTURE_CHANGE','state':'closed' if merged else 'open',
         'html_url':'https://github.com/fixture/isolated/pull/7','merged':merged,'mergeable':True,
         'head':{'ref':'feature','sha':sha,'repo':repo},'base':{'ref':'main','sha':'c'*40,'repo':repo}}
 mr = {'iid':7,'title':'FIXTURE_CHANGE','state':'merged' if merged else 'opened',
       'source_branch':'feature','target_branch':'main','source_project_id':123,'target_project_id':123,
       'sha':sha,'web_url':'https://gitlab.com/fixture/isolated/-/merge_requests/7',
       'detailed_merge_status':'mergeable','blocking_discussions_resolved':True,
       'head_pipeline':{'status':'success'}}
 approved = False
 approval = {'approvals_left':0,'approvals_required':0,'approved_by':[{'user':{'id':7}}] if approved else []}
 out = None
 if method != 'GET':
  # Never emulate a successful write after the route has been retired.
  with open(home/'forge.jsonl','a') as log:
   log.write(json.dumps({'method':method,'endpoint':endpoint,'supported':False})+'\n')
  sys.exit(92)
 elif path == '/user': out = {'id':7,'login':'fixture-user'}
 elif path in ('/repos/fixture/isolated','/projects/fixture%2Fisolated','/projects/123'): out = repo
 elif path == '/repos/fixture/isolated/issues' or path.endswith('/issues'): out = []
 elif path == '/repos/fixture/isolated/pulls': out = [pull]
 elif path == '/projects/123/merge_requests': out = [mr]
 elif path == '/repos/fixture/isolated/pulls/7': out = pull
 elif path == '/projects/123/merge_requests/7': out = mr
 elif path == '/repos/fixture/isolated/pulls/7/reviews': out = [review] if approved else []
 elif path == '/projects/123/merge_requests/7/approvals': out = approval
 elif path == '/projects/123/merge_requests/7/approval_state': out = {'rules':[]}
 elif path.endswith('/discussions'): out = []
 elif path == '/repos/fixture/isolated/actions/runs': out = {'workflow_runs':[]}
 elif path == '/projects/123/pipelines': out = []
 with open(home/'forge.jsonl','a') as log:
  log.write(json.dumps({'method':method,'endpoint':endpoint,'fields':fields,'head':sha,
                        'writes':0,'supported':out is not None})+'\n')
 if out is None: sys.exit(93)
 print(json.dumps(out))
'''


def sqlite_busy(error):
    # sqlite_errorcode is not available on our Python 3.8 baseline. Match only
    # SQLite's busy/locked messages there; corrupt/missing-schema errors fail.
    code = getattr(error, 'sqlite_errorcode', None)
    if code is not None:
        return isinstance(code, int) and code & 0xff in (5, 6)
    message = str(error).lower()
    return message in ('database is locked', 'database table is locked',
                       'database schema is locked') or message.startswith(
                           ('database table is locked: ', 'database schema is locked: '))


def receipt_rows(home, timeout_seconds=1.0):
    require(0 < timeout_seconds <= 1.0, 'invalid receipt observation deadline')
    files = list(home.rglob('state-v2.sqlite3'))
    if not files:
        return []
    require(len(files) == 1, 'ambiguous fixture database')
    deadline = time.monotonic() + timeout_seconds
    while True:
        try:
            # Connection.__exit__ manages transactions; closing() additionally
            # releases the reader on success AND on a busy/error observation.
            with closing(sqlite3.connect(files[0].as_uri() + '?mode=ro',
                                         uri=True, timeout=0.0)) as db:
                db.row_factory = sqlite3.Row
                rows = db.execute('SELECT operation_id,plan_json,state,verification,failure,updated_at_unix_ms '
                                  'FROM forge_mutation_receipts').fetchall()
            return [dict(row) for row in rows]
        except sqlite3.OperationalError as error:
            if not sqlite_busy(error):
                raise
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('SQLite receipt observation remained busy within {:.3f}s'.format(
                    timeout_seconds)) from error
            time.sleep(min(.025, remaining))


def validate_read_only(rows, trace, invocations, provider):
    require(not rows, 'retired route created a durable operation')
    require(bool(trace), 'Forge read projection was never exercised')
    require(all(r['method'] == 'GET' and r['supported'] for r in trace),
            'unexpected Forge write or unsupported API request')
    require(provider in PROVIDERS, 'unknown fixture provider')
    suffix = '/pulls' if provider == 'github' else '/merge_requests'
    require(any(r['endpoint'].split('?')[0].endswith(suffix) for r in trace),
            'change-request list was never read')
    require(bool(invocations), 'missing Forge invocation trace')
    for args in invocations:
        if args == ['--version'] or args[:2] == ['auth', 'status']:
            continue
        require(bool(args) and args[0] == 'api', 'unexpected Forge CLI operation')
        for i, arg in enumerate(args):
            if arg in ('--method', '-X'):
                require(i + 1 < len(args) and args[i + 1] == 'GET',
                        'unexpected Forge write method')
            elif arg.startswith(('--method=', '-X=')):
                require(arg.split('=', 1)[1] == 'GET', 'unexpected Forge write method')
            elif arg.startswith('-X') and arg != '-X':
                require(arg[2:] == 'GET', 'unexpected Forge write method')
    return {'writeCount':0, 'receiptCount':0, 'readCount':len(trace)}


def registry_visible(screen):
    return 'Mission Control' in screen and re.search(r'\b1 total\b', screen) is not None


def wait_registry(app):
    # q only exits Registry. A fixed drain after Esc can leave both bytes queued
    # while the TUI is busy, where they are parsed together as Alt+q.
    app.wait(lambda: registry_visible(app.screen()), 'Registry not observed before q exit')
    (app.output/'registry-before-exit.txt').write_text(app.screen(), encoding='utf-8')


def case(binary, output, home, scenario):
    provider, _, _ = scenario.split('-', 2)
    env = environment(home, home/'server.log', 'forge-revision')
    env['FORGE_CASE'] = scenario
    for tool in ('gh','glab'):
        shim = home/'bin'/tool
        shim.write_text(FORGE, encoding='utf-8'); shim.chmod(0o755)
    # Store every fixture/config outside the actual repo, so its clean checkout
    # remains independent of the mock's trace/phase files.
    repo = home/'repo'; repo.mkdir()
    for args in (['init','-q'], ['symbolic-ref','HEAD','refs/heads/feature'],
                 ['-c','user.name=Fixture','-c','user.email=fixture@example.invalid',
                  '-c','commit.gpgsign=false','commit','--allow-empty','-m','fixture'],
                 ['remote','add','origin','https://{}.com/fixture/isolated.git'.format(provider)]):
        subprocess.run(['/usr/bin/git']+args, cwd=repo, env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=10)
    # The normal transport fixture's one thread must point at this real checkout.
    codex = home/'bin/codex'
    codex.write_text(codex.read_text().replace("'cwd':os.environ['HOME']", "'cwd':os.environ['HOME']+'/repo'"))
    browser = home/'bin'/'fixture-browser'
    browser.write_text(
        "#!/usr/bin/python3\nimport json, os, sys\nfrom pathlib import Path\n"
        "with open(Path(os.environ['HOME'])/'browser.jsonl', 'a') as log:\n"
        " log.write(json.dumps(sys.argv[1:])+'\\n')\n", encoding='utf-8')
    browser.chmod(0o755)
    env['CODEX_TUI_BROWSER'] = str(browser)
    app = Session(binary, output, home, env)

    def read_trace(name):
        path = home/name
        if not path.exists():
            return []
        require(path.stat().st_size < 256*1024, 'fixture trace exceeded budget')
        return [json.loads(s) for s in path.read_text().splitlines()]

    def trace():
        return read_trace('forge.jsonl')

    def verify_read_only():
        require(len(list(home.rglob('state-v2.sqlite3'))) == 1,
                'durable fixture store not observed')
        return validate_read_only(receipt_rows(home), trace(),
                                  read_trace('forge-invocations.jsonl'), provider)

    def check_actions():
        app.send(b'.')
        app.wait(lambda: 'j/k move' in app.screen() and
                 'Edit local note' in app.screen(), 'context menu not observed')
        screen = app.screen()
        for label in ('Forge · ', 'GitHub · Merge commit', 'GitLab · Merge',
                      'Approve merge request', 'Merge merge request',
                      'Create worktree', 'Remove worktree'):
            require(label not in screen, 'retired mutation action: '+label)
        app.send(b'\x1b')
        app.wait(lambda: 'j/k move' not in app.screen(), 'context menu did not close')
        return screen

    try:
        app.wait(lambda: any('/pulls?' in r['endpoint'] or '/merge_requests?' in r['endpoint']
                             for r in trace()), 'Forge projection not exercised')
        wait_registry(app)
        app.send(b'w')
        app.wait(lambda: 'Workspace · Git + Forge' in app.screen() and
                 'FIXTURE_CHANGE' in app.screen(), 'read-only workspace evidence not observed')
        (output/'workspace-actions.txt').write_text(check_actions(), encoding='utf-8')
        app.send(b'r')
        app.wait(lambda: 'Changed files' in app.screen() and
                 'FIXTURE_CHANGE' in app.screen(), 'read-only Review evidence not observed')
        (output/'review-actions.txt').write_text(check_actions(), encoding='utf-8')
        app.send(b'o')
        app.wait(lambda: bool(read_trace('browser.jsonl')), 'external review handoff not observed')
        expected_url = ('https://github.com/fixture/isolated/pull/7' if provider == 'github'
                        else 'https://gitlab.com/fixture/isolated/-/merge_requests/7')
        require(read_trace('browser.jsonl') == [[expected_url]],
                'external handoff targeted the wrong URL or ran more than once')
        result = verify_read_only()
        result['externalUrl'] = expected_url
        result['resources'] = resources(app.proc.pid)
        app.send(b'\x1b')
        app.wait(lambda: 'Workspace · Git + Forge' in app.screen(), 'workspace return not observed')
        app.send(b'\x1b')
        wait_registry(app)
        result['registryReturnObserved'] = True
        result['terminal'] = app.finish()
        app.close()
        app = None
        app = Session(binary, output/'restart', home, env)
        wait_registry(app)
        app.send(b'w')
        app.wait(lambda: 'FIXTURE_CHANGE' in app.screen(), 'restart Forge projection not observed')
        app.send(b'\x1b')
        wait_registry(app)
        result['restart'] = verify_read_only()
        require(read_trace('browser.jsonl') == [[expected_url]], 'restart repeated external handoff')
        result['restartRegistryObserved'] = True
        result['restartTerminal'] = app.finish()
        return result
    finally:
        try:
            for name in ('forge.jsonl', 'forge-invocations.jsonl', 'browser.jsonl'):
                (output/name).write_text(''.join(json.dumps(r)+'\n' for r in read_trace(name)))
            (output/'operation.json').write_text(json.dumps(receipt_rows(home), indent=2)+'\n')
        finally:
            if app is not None:
                app.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path); parser.add_argument('output', type=Path)
    parser.add_argument('--scenario', choices=CASES)
    args = parser.parse_args()
    require(sys.platform=='linux', 'Linux PTY and /proc required')
    binary = args.binary.resolve(strict=True); args.output.mkdir(parents=True, exist_ok=False)
    result = {'schema':'codex-tui/forge-readonly-retirement/v1', 'status':'failed',
              'sourceSha':identity(binary,args.output), 'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
              'realForgeQualification':False,'realCodexQualification':False,'realSshQualification':False,
              'resourceBudgetScope':'isolated one-thread fixture; no global RSS guarantee','checks':[]}
    for scenario in [args.scenario] if args.scenario else CASES:
        try:
            with tempfile.TemporaryDirectory(prefix='ctui-forge-revision-') as directory:
                item = dict(case(binary,args.output/scenario,Path(directory),scenario), mode=scenario,passed=True)
        except Exception as error:
            item = {'mode':scenario,'passed':False,'error':str(error)}
        result['checks'].append(item); print(json.dumps(item),flush=True)
    if all(x['passed'] for x in result['checks']): result['status']='passed'
    (args.output/'receipt.json').write_text(json.dumps(result,indent=2)+'\n')
    return 0 if result['status']=='passed' else 1

if __name__=='__main__': sys.exit(main())
