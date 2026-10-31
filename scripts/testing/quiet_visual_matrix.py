#!/usr/bin/env python3
"""Source-bound, offline 12-case Linux PTY regression; never human qualification."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

import quiet_visual_probe as probe

CASES = tuple((lang, width, height, mode)
              for lang in ('en', 'zh-CN')
              for width, height in ((60, 24), (100, 30))
              for mode in ('normal', 'quiet', 'screen-reader'))
CHECKS = {'idleIsNotSuccess', 'quietVisualActionVisible', 'overlayNotHijacked',
          'exactThreadJump', 'returnToRegistry', 'quietSystemErrorVisible',
          'noBell', 'querySubscriptionOnly'}
FALSE_FIELDS = ('realCodexQualification', 'realSshQualification',
                'screenReaderQualification', 'stableReady', 'publicationAllowed')


def require(value, message):
    if not value:
        raise ValueError(message)


def validate_identity_report(report, source_sha, exit_code):
    require(exit_code == 5, 'identity probe must report undersampled diagnostics')
    require(isinstance(report, dict), 'identity report must be an object')
    require(report.get('schema') == 'codex-tui/performance/v2', 'identity schema')
    require(report.get('sourceSha') == source_sha, 'embedded source SHA mismatch')
    require(report.get('fixture') == 'resident-planning-10k', 'identity fixture')
    require(report.get('sampleQualified') is False, 'identity is not performance qualification')
    for key in ('warmupIterations', 'iterations'):
        require(type(report.get(key)) is int and report[key] == 1, 'identity sample count')


def identify(binary, source_sha, output):
    require(bool(re.fullmatch(r'[0-9a-f]{40}', source_sha)), 'full source SHA required')
    require(binary.is_file() and not binary.is_symlink(), 'regular binary required')
    digest = hashlib.sha256(binary.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix='quiet-identity-') as tmp:
        home = Path(tmp)
        empty_path = home / 'empty-path'
        empty_path.mkdir()
        env = {'HOME': tmp, 'XDG_CONFIG_HOME': str(home / 'config'),
               'XDG_DATA_HOME': str(home / 'data'), 'XDG_STATE_HOME': str(home / 'state'),
               'CODEX_HOME': str(home / 'codex'), 'PATH': str(empty_path), 'LANG': 'C.UTF-8'}
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            process = subprocess.Popen(
                [str(binary.resolve()), 'release', 'benchmark', '--warmup', '1',
                 '--iterations', '1', '--source', 'quiet-visual-identity', '--json'],
                cwd=home, env=env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr,
                start_new_session=True)
            try:
                process.wait(timeout=15)
                require(stdout.tell() <= 65536 and stderr.tell() <= 65536, 'identity output bound')
                stdout.seek(0)
                report = json.loads(stdout.read(65536))
            finally:
                probe.stop(process)
        require(set(home.iterdir()) == {empty_path}, 'identity probe wrote application state')
    validate_identity_report(report, source_sha, process.returncode)
    require(hashlib.sha256(binary.read_bytes()).hexdigest() == digest, 'binary changed during identity probe')
    metadata = {'schema': 'codex-tui/fixture-binary-identity/v1', 'platform': 'linux',
                'commitSha': source_sha, 'binarySha256': digest, 'fixtureOnly': True,
                'realCodexQualification': False, 'stableReady': False, 'publicationAllowed': False}
    path = output / 'binary-identity.json'
    path.write_text(json.dumps(metadata, indent=2) + '\n', encoding='utf-8')
    return path, digest


def validate_case(receipt, case, source_sha, digest, exit_code):
    require(isinstance(receipt, dict), 'missing receipt object')
    require(exit_code == 0 and receipt.get('status') == 'passed', 'case did not pass')
    require(receipt.get('schema') == 'codex-tui/quiet-visual-probe/v1', 'case schema')
    require(receipt.get('sourceSha') == source_sha and receipt.get('binarySha256') == digest,
            'case identity mismatch')
    fields = ('language', 'width', 'height', 'presentation')
    require(tuple(receipt.get(k) for k in fields) == case, 'case matrix identity mismatch')
    checks = receipt.get('checks')
    require(isinstance(checks, dict) and set(checks) == CHECKS
            and all(value is True for value in checks.values()), 'case checks incomplete')
    require(receipt.get('failure') is None and type(receipt.get('exitCode')) is int
            and receipt['exitCode'] == 0, 'case failure or exit')
    require(receipt.get('terminalRestored') is True and type(receipt.get('backendStarts')) is int
            and receipt['backendStarts'] == 1,
            'terminal restoration or backend lifecycle failed')
    require(isinstance(receipt.get('traps'), list) and set(receipt['traps']) <= {'git'},
            'external program trap fired')
    require(receipt.get('fixtureOnly') is True and 'humanTimeSavings' in receipt
            and receipt['humanTimeSavings'] is None,
            'fixture authority mismatch')
    require(all(receipt.get(key) is False for key in FALSE_FIELDS), 'unsupported qualification claim')


def run(binary, source_sha, output):
    require(sys.platform == 'linux', 'Linux PTY regression only')
    output = output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    summary = {'schema': 'codex-tui/quiet-visual-matrix/v1', 'sourceSha': source_sha,
               'expectedCases': len(CASES), 'status': 'failed', 'cases': [],
               'fixtureOnly': True, 'humanTimeSavings': None}
    summary.update({field: False for field in FALSE_FIELDS})
    def checkpoint():
        (output / 'matrix.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    checkpoint()
    try:
        metadata, digest = identify(binary, source_sha, output)
        summary['binarySha256'] = digest
        checkpoint()
        for index, case in enumerate(CASES):
            language, width, height, mode = case
            directory = output / ('%02d-%s-%dx%d-%s' % (index + 1, language, width, height, mode))
            code = probe.run(binary.resolve(), metadata, directory, language, width, height, mode)
            receipt_path = directory / 'receipt.json'
            raw = receipt_path.read_bytes()
            receipt = json.loads(raw)
            summary['cases'].append({'path': directory.name + '/receipt.json',
                                     'receiptSha256': hashlib.sha256(raw).hexdigest(),
                                     'status': receipt.get('status'), 'exitCode': code})
            checkpoint()
            validate_case(receipt, case, source_sha, digest, code)
        require(len(summary['cases']) == len(CASES), 'incomplete matrix')
        summary['status'] = 'passed'
    except Exception as error:
        # A partial matrix stays failed; original bounded fixture evidence is retained.
        summary['failure'] = str(error)
    finally:
        checkpoint()
    print(json.dumps(summary))
    return 0 if summary['status'] == 'passed' else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    return run(args.binary, args.source_sha, args.output)


if __name__ == '__main__':
    raise SystemExit(main())
