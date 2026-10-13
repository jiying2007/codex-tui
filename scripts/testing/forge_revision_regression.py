#!/usr/bin/env python3
"""Linux real TUI confirmation/write/readback probes; isolated Forge shims only."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
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
OPERATIONS = ('approve', 'merge')
MODES = ('control', 'missing', 'before', 'after', 'uncertain')
CASES = ['{}-{}-{}'.format(p, op, mode)
         for p in PROVIDERS for op in OPERATIONS for mode in MODES]
CASES += ['github-approve-wrong-review', 'github-approve-stale-existing']

# No network client is invoked. Unexpected commands fail the fixture rather than
# delegating to installed gh/glab. A file lock serializes separate CLI processes.
FORGE = r'''#!/usr/bin/python3
import fcntl, json, os, sys
from pathlib import Path
from urllib.parse import urlsplit
home = Path(os.environ['HOME'])
args = sys.argv[1:]
provider, operation, mode = os.environ['FORGE_CASE'].split('-', 2)
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
 state_path = home/'forge-state.json'
 state = json.loads(state_path.read_text()) if state_path.exists() else {'writes':0}
 changed = ((mode == 'before' and (home/'confirmed').exists()) or
            (mode in ('after','uncertain') and state['writes'] > 0))
 sha = None if mode == 'missing' else ('b' if changed else 'a') * 40
 merged = operation == 'merge' and state['writes'] > 0
 repo = {'id':123,'full_name':'fixture/isolated','html_url':'https://github.com/fixture/isolated',
         'path_with_namespace':'fixture/isolated','web_url':'https://gitlab.com/fixture/isolated',
         'default_branch':'main'}
 review = {'id':80,'state':'APPROVED','user':{'id':7,'login':'fixture-user'},
           'commit_id':('b' if mode == 'wrong-review' or (mode == 'stale-existing' and not state['writes']) else 'a')*40}
 pull = {'number':7,'title':'FIXTURE_CHANGE','state':'closed' if merged else 'open',
         'html_url':'https://github.com/fixture/isolated/pull/7','merged':merged,'mergeable':True,
         'head':{'ref':'feature','sha':sha,'repo':repo},'base':{'ref':'main','sha':'c'*40,'repo':repo}}
 mr = {'iid':7,'title':'FIXTURE_CHANGE','state':'merged' if merged else 'opened',
       'source_branch':'feature','target_branch':'main','source_project_id':123,'target_project_id':123,
       'sha':sha,'web_url':'https://gitlab.com/fixture/isolated/-/merge_requests/7',
       'detailed_merge_status':'mergeable','blocking_discussions_resolved':True,
       'head_pipeline':{'status':'success'}}
 approved = state['writes'] > 0 or mode == 'stale-existing'
 approval = {'approvals_left':0,'approvals_required':0,'approved_by':[{'user':{'id':7}}] if approved else []}
 out = None
 if method != 'GET':
  allowed = ('/repos/fixture/isolated/pulls/7/' + ('reviews' if operation=='approve' else 'merge')) if provider=='github' else ('/projects/123/merge_requests/7/' + ('approve' if operation=='approve' else 'merge'))
  if path != allowed or method != ('POST' if operation=='approve' else 'PUT'): sys.exit(92)
  state['writes'] += 1
  state_path.write_text(json.dumps(state))
  out = review if operation=='approve' else {'merged':True,'message':'fixture','sha':'c'*40}
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
                        'writes':state['writes'],'supported':out is not None})+'\n')
 if out is None: sys.exit(93)
 if method != 'GET' and mode == 'uncertain':
  print('{invalid-json'); sys.exit(0)
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


def validate_outcome(rows, trace, mode):
    require(len(rows) == 1, 'expected exactly one durable operation')
    row = rows[0]
    plan = json.loads(row['plan_json'])
    require([x['expected'] for x in plan['preconditions'] if x['key']=='confirmed-head-sha'] == [SHA_A],
            'confirmed SHA changed or disappeared from persisted plan')
    writes = [r for r in trace if r['method'] != 'GET']
    wanted_writes = 0 if mode == 'before' else 1
    require(len(writes) == wanted_writes, 'expected {} writes, observed {}'.format(wanted_writes, len(writes)))
    for write in writes:
        actual = write['fields'].get('commit_id', write['fields'].get('sha'))
        require(actual == SHA_A, 'provider mutation did not bind the confirmed SHA')
    wanted = 'failed' if mode=='before' else ('succeeded' if mode in ('control','stale-existing') else 'outcome-unknown')
    require(row['state'] == wanted, 'expected {}, observed {}'.format(wanted, row['state']))
    require(all(r['supported'] for r in trace), 'unrecognized Forge request contaminated fixture')
    return {'state':row['state'],'writeCount':len(writes),'confirmedSha':SHA_A}


def case(binary, output, home, scenario):
    provider, operation, mode = scenario.split('-', 2)
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
    app = Session(binary, output, home, env)
    def trace():
        path=home/'forge.jsonl'
        if not path.exists(): return []
        require(path.stat().st_size < 256*1024, 'Forge trace exceeded fixture budget')
        return [json.loads(s) for s in path.read_text().splitlines()]
    try:
        app.wait(lambda: any('/pulls?' in r['endpoint'] or '/merge_requests?' in r['endpoint'] for r in trace()),
                 'Forge projection not exercised')
        app.drain(.25)
        app.send(b'w')
        app.send(b'.')
        label = 'Forge · ' + ('Approve' if operation=='approve' else 'Merge') + ' merge request'
        app.wait(lambda: label in app.screen(), 'Forge action not projected')
        for _ in range(25):
            if '> '+label in app.screen(): break
            app.send(b'j')
        require('> '+label in app.screen(), 'action could not be selected')
        app.send(b'\r')
        if mode == 'missing':
            app.wait(lambda: 'cannot create forge mutation plan' in app.screen(),
                     'missing revision did not refuse plan creation')
            require(not receipt_rows(home), 'missing revision created a durable operation')
            require(not any(r['method']!='GET' for r in trace()), 'missing revision invoked a write')
            app.send(b'\x1b')
            return {'planRefused':True, 'writeCount':0, 'resources':resources(app.proc.pid),
                    'terminal':app.finish()}
        app.wait(lambda: 'confirmed-head-sha' in app.screen() or 'Confirmed HEAD:' in app.screen(), 'confirmation did not display revision')
        require(SHA_A in app.screen(), 'full confirmed revision was not displayed')
        (output/'confirmation.txt').write_text(app.screen(), encoding='utf-8')
        require(not any(r['method']!='GET' for r in trace()), 'write occurred before confirmation')
        (home/'confirmed').touch()
        app.send(b'y')
        def terminal_receipt_observed():
            rows = receipt_rows(home)
            return bool(rows) and rows[0]['state'] in ('succeeded','failed','outcome-unknown')
        app.wait(terminal_receipt_observed, 'terminal durable receipt not observed', 15)
        app.drain(.25)
        rows = receipt_rows(home)
        (output/'operation.json').write_text(json.dumps(rows, indent=2)+'\n')
        result = validate_outcome(rows, trace(), mode)
        result['resources'] = resources(app.proc.pid)
        # Restart the actual binary with the same store; recover only through its
        # normal startup path. Unknown operations must not issue another write.
        app.send(b'\x1b')
        result['terminal'] = app.finish()
        app.close(); app = None
        recovered = Session(binary, output/'restart', home, env)
        app = recovered
        app.wait(lambda: '1 total' in app.screen(), 'restart did not load Registry')
        if rows[0]['state'] == 'outcome-unknown':
            app.wait(lambda: receipt_rows(home)[0]['updated_at_unix_ms'] > rows[0]['updated_at_unix_ms'],
                     'restart did not actually reconcile the unknown operation', 15)
        app.drain(.15)
        result['recovery'] = validate_outcome(receipt_rows(home), trace(), mode)
        result['restartTerminal'] = app.finish()
        return result
    finally:
        try:
            (output/'forge.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in trace()))
            rows = receipt_rows(home)
            (output/'operation.json').write_text(json.dumps(rows, indent=2)+'\n')
        finally:
            # Failed evidence collection must not leave a live fixture session.
            if app is not None: app.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path); parser.add_argument('output', type=Path)
    parser.add_argument('--scenario', choices=CASES)
    args = parser.parse_args()
    require(sys.platform=='linux', 'Linux PTY and /proc required')
    binary = args.binary.resolve(strict=True); args.output.mkdir(parents=True, exist_ok=False)
    result = {'schema':'codex-tui/forge-revision-regression/v1', 'status':'failed',
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
