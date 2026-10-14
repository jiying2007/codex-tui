#!/usr/bin/env python3
"""Run the nine existing Linux fixture suites; never attest real services or SSH."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from _compat import require_python38

# Counts are an explicit review boundary, not an automatic discovery of PASS.
SUITES = (
    ('terminal_restoration', 'automated-pty-regression/v2', 9),
    ('app_server_transport_regression', 'app-server-transport-regression/v1', 14),
    ('runtime_fault_regression', 'runtime-fault-regression/v1', 6),
    ('git_lifecycle_regression', 'git-lifecycle-regression/v1', 11),
    ('watch_ownership_regression', 'watch-ownership-regression/v1', 21),
    ('watch_pressure_regression', 'watch-pressure-regression/v1', 6),
    ('forge_revision_regression', 'forge-revision-regression/v1', 22),
    ('watch_cache_interruption_regression', 'cache-interruption-regression/v1', 6),
    ('watch_cache_regression', 'cache-workload-regression/v1', 13),
)
TERMINAL_CASES = {'normal', 'failed-save', 'raw-ctrl-c', 'SIGTERM', 'SIGHUP',
                  'SIGINT', 'SIGTERM-failed-save', 'SIGHUP-failed-save',
                  'SIGINT-failed-save'}
TERMINAL_CHECKS = {'rawModeObserved', 'terminalAttributesRestored', 'expectedExitCode',
                   'alternateScreenRestored', 'bracketedPasteRestored', 'cursorRestored'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def read_receipt(path):
    require(not path.is_symlink() and path.is_file(), 'missing/linked receipt')
    raw = path.read_bytes()
    value = json.loads(raw.decode('utf-8'))
    require(isinstance(value, dict), 'receipt must be an object')
    return value, hashlib.sha256(raw).hexdigest()


def validate_suite(output, suite, source, binary_digest):
    name, schema, count = suite
    folder = output / name
    if name == 'terminal_restoration':
        paths = sorted(folder.glob('*/receipt.json'))
        require({p.parent.name for p in paths} == TERMINAL_CASES, 'terminal case set drift')
    else:
        paths = [folder / 'receipt.json']
    hashes = {}
    for path in paths:
        row, sha256 = read_receipt(path)
        require(row.get('schema') == 'codex-tui/' + schema, 'receipt schema drift')
        require(row.get('status') == 'passed', 'receipt did not pass')
        require(row.get('sourceSha') == source, 'receipt source mismatch')
        require(row.get('binarySha256') == binary_digest, 'receipt binary mismatch')
        require(row.get('realSshQualification') is False, 'fixture SSH authority drift')
        for flag in ('realCodexQualification', 'realForgeQualification', 'manualQualification'):
            require(flag not in row or row[flag] is False, 'fixture authority drift')
        checks = row.get('checks')
        if name == 'terminal_restoration':
            require(isinstance(checks, dict), 'terminal checks missing')
            require(TERMINAL_CHECKS <= set(checks), 'terminal assertions missing')
            require(all(value is True for value in checks.values()), 'terminal assertion failed')
            if 'failed-save' in path.parent.name:
                require(checks.get('saveErrorAfterRestoration') is True, 'save order missing')
        else:
            require(isinstance(checks, list) and len(checks) == count, 'scenario count drift')
            require(all(isinstance(c, dict) and c.get('passed') is True for c in checks),
                    'scenario did not pass')
        hashes[path.relative_to(output).as_posix()] = sha256
    return hashes


def verify_source(root, source):
    def git(*args):
        return subprocess.check_output(['git', '-C', str(root), *args],
                                       stderr=subprocess.PIPE, timeout=15).decode('utf-8').strip()
    require(git('rev-parse', 'HEAD') == source, 'source HEAD mismatch')
    require(not git('status', '--porcelain'), 'source worktree must be clean')
    return git('rev-parse', 'HEAD^{tree}')


def main():
    require_python38()
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('source_root', type=Path)
    p.add_argument('binary', type=Path)
    p.add_argument('source_sha')
    p.add_argument('new_output_dir', type=Path)
    a = p.parse_args()
    require(sys.platform == 'linux', 'Linux required')
    require(re.fullmatch('[0-9a-f]{40}', a.source_sha), 'invalid source SHA')
    root, binary = a.source_root.resolve(strict=True), a.binary.resolve(strict=True)
    require(root == Path(__file__).resolve().parents[2], 'use the helper from the selected source')
    require(binary.is_file() and os.access(str(binary), os.X_OK), 'binary not executable')
    tree = verify_source(root, a.source_sha)
    before = digest(binary)
    out = a.new_output_dir.absolute()
    require(not out.exists() and not out.is_symlink(), 'use a new output directory')
    os.umask(0o077)
    out.mkdir()  # Parent is deliberately not created; never reuse a prior result.
    env = os.environ.copy()
    env.update(PYTHONDONTWRITEBYTECODE='1', CODEX_TUI_PTY_SOURCE_SHA=a.source_sha)
    summary = {'schema': 'codex-tui/device-fixture-run/v1', 'status': 'failed',
               'sourceSha': a.source_sha, 'sourceTree': tree, 'binarySha256': before,
               'suites': [], 'receiptSha256': {}, 'realCodexQualification': False,
               'realSshQualification': False, 'stablePublicationAuthority': False}
    try:
        # Reuse the maintained isolated doctor probe; no real account or model call.
        code = ('import sys; from pathlib import Path; '
                'sys.path.insert(0, sys.argv[1]); '
                'from watch_ownership_regression import identity; '
                'identity(Path(sys.argv[2]), Path(sys.argv[3]))')
        with (out / 'identity.log').open('xb') as log:
            subprocess.run([sys.executable, '-O', '-c', code, str(root/'scripts/testing'),
                            str(binary), str(out)], cwd=root, env=env,
                           stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                           timeout=45, check=True)
        identity, identity_hash = read_receipt(out / 'identity.json')
        require(identity.get('schema') == 'codex-tui/compat/v2' and
                identity.get('sourceSha') == a.source_sha, 'binary source not verified')
        summary['receiptSha256']['identity.json'] = identity_hash
        for suite in SUITES:
            name, _, count = suite
            command = [sys.executable, '-O', str(root/'scripts/testing'/ (name+'.py')),
                       str(binary), str(out/name)]
            if name == 'watch_cache_regression':
                command += ['--duration-seconds', '300']
            result = {'name': name, 'status': 'failed', 'expectedScenarios': count}
            summary['suites'].append(result)
            print('Running ' + name, flush=True)
            with (out/(name+'.log')).open('xb') as log:
                # One execution per suite; no timeout increase in repository fixtures.
                proc = subprocess.run(command, cwd=root, env=env, stdin=subprocess.DEVNULL,
                                      stdout=log, stderr=log, timeout=900)
            result['exitCode'] = proc.returncode
            require(proc.returncode == 0, 'suite failed; inspect its private log')
            summary['receiptSha256'].update(validate_suite(out, suite, a.source_sha, before))
            result['status'] = 'passed'
        require(digest(binary) == before, 'binary changed during validation')
        require(verify_source(root, a.source_sha) == tree, 'source changed during validation')
        summary['status'] = 'passed'
        summary['totalScenarios'] = sum(item[2] for item in SUITES)
    except (OSError, ValueError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        summary['failureType'] = type(error).__name__  # Never echo arbitrary child diagnostics.
    with (out/'field-summary.json').open('x', encoding='utf-8') as stream:
        json.dump(summary, stream, indent=2); stream.write('\n')
    print(json.dumps(summary, indent=2))
    return 0 if summary['status'] == 'passed' else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.SubprocessError):
        sys.exit('Device regression preflight failed; check source, binary and fresh output path.')
