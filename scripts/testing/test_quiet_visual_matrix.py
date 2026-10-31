import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import quiet_visual_matrix as matrix

SHA = 'a' * 40
DIGEST = 'b' * 64


def identity():
    return {'schema': 'codex-tui/performance/v2', 'sourceSha': SHA,
            'fixture': 'resident-planning-10k', 'sampleQualified': False,
            'warmupIterations': 1, 'iterations': 1}


def receipt(case):
    value = {'schema': 'codex-tui/quiet-visual-probe/v1', 'sourceSha': SHA,
             'binarySha256': DIGEST, 'status': 'passed', 'failure': None,
             'exitCode': 0, 'terminalRestored': True, 'backendStarts': 1,
             'checks': {key: True for key in matrix.CHECKS}, 'traps': ['git'],
             'fixtureOnly': True, 'humanTimeSavings': None}
    value.update(zip(('language', 'width', 'height', 'presentation'), case))
    value.update({field: False for field in matrix.FALSE_FIELDS})
    return value


class IdentityTests(unittest.TestCase):
    def test_valid_report_is_not_a_performance_claim(self):
        matrix.validate_identity_report(identity(), SHA, 5)

    def test_wrong_embedded_sha_is_rejected(self):
        with self.assertRaises(ValueError):
            matrix.validate_identity_report(identity(), 'c' * 40, 5)

    def test_exit_or_qualified_sample_is_rejected(self):
        for code in (0, 1, 3):
            with self.assertRaises(ValueError):
                matrix.validate_identity_report(identity(), SHA, code)
        value = identity()
        value['sampleQualified'] = True
        with self.assertRaises(ValueError):
            matrix.validate_identity_report(value, SHA, 5)

    def test_boolean_sample_count_is_rejected(self):
        value = identity()
        value['iterations'] = True
        with self.assertRaises(ValueError):
            matrix.validate_identity_report(value, SHA, 5)

    def test_identity_exec_uses_isolated_profile(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / 'fixture'
            binary.write_text('#!' + sys.executable + '\nimport json, os, sys\n'
                'assert "OPENAI_API_KEY" not in os.environ\n'
                'print(' + repr(json.dumps(identity())) + ')\nsys.exit(5)\n')
            binary.chmod(0o700)
            output = root / 'output'
            output.mkdir()
            with patch.dict('os.environ', {'OPENAI_API_KEY': 'not-inherited'}):
                metadata, digest = matrix.identify(binary, SHA, output)
            self.assertEqual(digest, hashlib.sha256(binary.read_bytes()).hexdigest())
            self.assertEqual(json.loads(metadata.read_text())['commitSha'], SHA)

    def test_symlink_and_bad_source_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / 'binary'
            binary.write_bytes(b'not-executable')
            link = root / 'link'
            link.symlink_to(binary)
            for path, sha in ((link, SHA), (binary, 'short')):
                with self.assertRaises(ValueError):
                    matrix.identify(path, sha, root)


class ReceiptTests(unittest.TestCase):
    def test_all_twelve_matrix_combinations_are_unique(self):
        self.assertEqual(len(matrix.CASES), 12)
        self.assertEqual(len(set(matrix.CASES)), 12)
        for case in matrix.CASES:
            matrix.validate_case(receipt(case), case, SHA, DIGEST, 0)

    def test_wrong_case_source_or_binary_is_rejected(self):
        case = matrix.CASES[0]
        for field, value in [('sourceSha', 'c' * 40), ('binarySha256', 'c' * 64),
                             ('language', 'zh-CN'), ('presentation', 'quiet')]:
            data = receipt(case)
            data[field] = value
            with self.assertRaises(ValueError):
                matrix.validate_case(data, case, SHA, DIGEST, 0)

    def test_missing_false_or_extra_check_is_rejected(self):
        case = matrix.CASES[0]
        for checks in ({}, {key: False for key in matrix.CHECKS},
                       dict(receipt(case)['checks'], extra=True)):
            data = receipt(case)
            data['checks'] = checks
            with self.assertRaises(ValueError):
                matrix.validate_case(data, case, SHA, DIGEST, 0)

    def test_qualification_claim_or_external_trap_is_rejected(self):
        case = matrix.CASES[0]
        changes = [(field, True) for field in matrix.FALSE_FIELDS]
        changes += [('traps', ['codex']), ('traps', ['notify-send']),
                    ('humanTimeSavings', 25), ('backendStarts', 2), ('backendStarts', True),
                    ('exitCode', False), ('terminalRestored', False)]
        for field, value in changes:
            data = receipt(case)
            data[field] = value
            with self.assertRaises(ValueError):
                matrix.validate_case(data, case, SHA, DIGEST, 0)

    def test_failed_process_cannot_be_masked_by_pass_receipt(self):
        with self.assertRaises(ValueError):
            matrix.validate_case(receipt(matrix.CASES[0]), matrix.CASES[0], SHA, DIGEST, 1)


class RunnerTests(unittest.TestCase):
    def fake_probe(self, binary, metadata, directory, language, width, height, mode):
        self.assertTrue(directory.is_absolute())
        self.assertTrue(metadata.is_absolute())
        directory.mkdir()
        (directory / 'receipt.json').write_text(json.dumps(receipt((language, width, height, mode))))
        return 0

    def test_complete_matrix_retains_all_receipt_digests(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'output'
            with patch.object(matrix, 'identify', return_value=(Path(tmp) / 'meta', DIGEST)), \
                 patch.object(matrix.probe, 'run', side_effect=self.fake_probe):
                self.assertEqual(matrix.run(Path(tmp) / 'bin', SHA, output), 0)
            report = json.loads((output / 'matrix.json').read_text())
            self.assertEqual(len(report['cases']), 12)
            self.assertEqual(report['status'], 'passed')
            for case in report['cases']:
                self.assertEqual(case['receiptSha256'], hashlib.sha256((output / case['path']).read_bytes()).hexdigest())

    def test_relative_output_is_resolved_before_child_changes_cwd(self):
        with tempfile.TemporaryDirectory(dir='.') as tmp:
            output = Path(tmp) / 'output'
            with patch.object(matrix, 'identify', side_effect=lambda b, s, o: (o / 'meta', DIGEST)), \
                 patch.object(matrix.probe, 'run', side_effect=self.fake_probe):
                self.assertEqual(matrix.run(Path(tmp) / 'bin', SHA, output), 0)

    def test_partial_matrix_fails_without_retrying(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'output'
            with patch.object(matrix, 'identify', return_value=(Path(tmp) / 'meta', DIGEST)), \
                 patch.object(matrix.probe, 'run', side_effect=ValueError('fixture failure')) as run:
                self.assertEqual(matrix.run(Path(tmp) / 'bin', SHA, output), 1)
                self.assertEqual(run.call_count, 1)
            report = json.loads((output / 'matrix.json').read_text())
            self.assertEqual(report['status'], 'failed')
            self.assertEqual(report['cases'], [])

    def test_existing_evidence_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp)
            keep = output / 'keep'
            keep.write_text('sentinel')
            with self.assertRaises(FileExistsError):
                matrix.run(output / 'bin', SHA, output)
            self.assertEqual(keep.read_text(), 'sentinel')


if __name__ == '__main__':
    unittest.main()
