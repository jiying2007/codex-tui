"""Field helper regressions: isolated fixtures, never human or live-service PASS."""
import contextlib
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

import backup_device_sqlite as backup
import run_device_regressions as runner
import sample_device_process as sampler

SHA, DIGEST = 'a'*40, 'b'*64
ROOT = Path(runner.__file__).resolve().parents[2]


class FixtureValidation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()

    def receipt(self, suite):
        return {'schema': 'codex-tui/'+suite[1], 'status': 'passed', 'sourceSha': SHA,
                'binarySha256': DIGEST, 'realSshQualification': False,
                'realCodexQualification': False,
                'checks': [{'passed': True, 'mode': str(i)} for i in range(suite[2])]}

    def write(self, suite, row):
        path = self.root/suite[0]/'receipt.json'
        path.parent.mkdir(exist_ok=True)
        path.write_text(json.dumps(row), encoding='utf-8')
        return path

    def test_all_eight_aggregate_receipt_shapes_and_exact_hashes(self):
        for suite in runner.SUITES[1:]:
            path = self.write(suite, self.receipt(suite))
            self.assertEqual(runner.validate_suite(self.root, suite, SHA, DIGEST),
                {path.relative_to(self.root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()})

    def test_missing_mismatched_or_truthy_fields_cannot_pass_under_optimized_python(self):
        suite = runner.SUITES[1]
        for field, value in [('sourceSha', None), ('sourceSha', 'c'*40),
                             ('binarySha256', None), ('binarySha256', 'c'*64),
                             ('status', 'failed'), ('schema', 'unknown'),
                             ('realSshQualification', None), ('realSshQualification', 0),
                             ('realCodexQualification', True), ('manualQualification', True),
                             ('realForgeQualification', True), ('checks', []), ('checks', {})]:
            with self.subTest(field=field, value=value):
                row = self.receipt(suite); row[field] = value
                self.write(suite, row)
                with self.assertRaises(ValueError):
                    runner.validate_suite(self.root, suite, SHA, DIGEST)
        row = self.receipt(suite); row['checks'][0]['passed'] = 1
        self.write(suite, row)
        with self.assertRaises(ValueError):
            runner.validate_suite(self.root, suite, SHA, DIGEST)

    def test_terminal_requires_all_nine_named_cases_and_restoration_assertions(self):
        suite = runner.SUITES[0]
        for name in runner.TERMINAL_CASES:
            path = self.root/suite[0]/name/'receipt.json'; path.parent.mkdir(parents=True)
            row = self.receipt(suite)
            row['checks'] = dict.fromkeys(runner.TERMINAL_CHECKS, True)
            if 'failed-save' in name:
                row['checks']['saveErrorAfterRestoration'] = True
            path.write_text(json.dumps(row))
        self.assertEqual(len(runner.validate_suite(self.root, suite, SHA, DIGEST)), 9)
        path = self.root/suite[0]/'failed-save'/'receipt.json'
        row = json.loads(path.read_text()); del row['checks']['saveErrorAfterRestoration']
        path.write_text(json.dumps(row))
        with self.assertRaises(ValueError):
            runner.validate_suite(self.root, suite, SHA, DIGEST)
        path.unlink()
        with self.assertRaises(ValueError):
            runner.validate_suite(self.root, suite, SHA, DIGEST)

    @unittest.skipUnless(os.name == 'posix', 'real symlink fixture')
    def test_receipt_symlink_is_not_accepted(self):
        suite = runner.SUITES[1]; path = self.write(suite, self.receipt(suite))
        target = path.with_suffix('.saved'); path.rename(target); path.symlink_to(target)
        with self.assertRaises(ValueError):
            runner.validate_suite(self.root, suite, SHA, DIGEST)

    def test_source_gate_rejects_wrong_head_and_dirty_tree(self):
        with mock.patch.object(runner.subprocess, 'check_output', return_value=b'wrong'):
            with self.assertRaisesRegex(ValueError, 'HEAD mismatch'):
                runner.verify_source(self.root, SHA)
        with mock.patch.object(runner.subprocess, 'check_output', side_effect=[SHA.encode(), b'?? x']):
            with self.assertRaisesRegex(ValueError, 'clean'):
                runner.verify_source(self.root, SHA)
        with mock.patch.object(runner.subprocess, 'check_output', side_effect=[SHA.encode(), b'', b'c'*40]):
            self.assertEqual(runner.verify_source(self.root, SHA), 'c'*40)

    def run_orchestrator(self, fail=None, identity_source=SHA, after_digest=DIGEST):
        binary = self.root/'binary'; binary.write_bytes(b'fixture'); binary.chmod(0o700)
        out = self.root/'run'
        suites = runner.SUITES[1:3]
        calls = []
        def child(command, **kwargs):
            calls.append(command)
            self.assertEqual(kwargs['cwd'], ROOT)
            self.assertEqual(kwargs['env']['CODEX_TUI_PTY_SOURCE_SHA'], SHA)
            if '-c' in command:
                (out/'identity.json').write_text(json.dumps({'schema':'codex-tui/compat/v2',
                                                           'sourceSha':identity_source}))
                return subprocess.CompletedProcess(command, 0)
            name = Path(command[2]).stem
            if fail == 'timeout':
                raise subprocess.TimeoutExpired(command, kwargs['timeout'])
            if fail == 'nonzero':
                return subprocess.CompletedProcess(command, 9)
            suite = next(s for s in suites if s[0] == name)
            path = out/name/'receipt.json'; path.parent.mkdir()
            path.write_text(json.dumps(self.receipt(suite)))
            return subprocess.CompletedProcess(command, 0)
        with mock.patch.object(sys, 'argv', ['runner',str(ROOT),str(binary),SHA,str(out)]), \
             mock.patch.object(runner.sys, 'platform', 'linux'), \
             mock.patch.object(runner, 'verify_source', return_value='c'*40), \
             mock.patch.object(runner, 'digest', side_effect=[DIGEST,after_digest]), \
             mock.patch.object(runner, 'SUITES', suites), \
             mock.patch.object(runner.subprocess, 'run', side_effect=child), \
             contextlib.redirect_stdout(io.StringIO()):
            result = runner.main()
        return result, json.loads((out/'field-summary.json').read_text()), calls

    def test_driver_success_keeps_false_live_and_publication_authority(self):
        result, row, calls = self.run_orchestrator()
        self.assertEqual(result, 0); self.assertEqual(len(calls), 3)
        self.assertFalse(row['realSshQualification']); self.assertFalse(row['realCodexQualification'])
        self.assertFalse(row['stablePublicationAuthority'])
        self.assertEqual(len(row['receiptSha256']), 3)

    def test_driver_nonzero_retains_failed_summary_and_does_not_retry(self):
        result, row, calls = self.run_orchestrator(fail='nonzero')
        self.assertEqual(result, 1); self.assertEqual(len(calls), 2)
        self.assertEqual(row['suites'][0]['exitCode'], 9)
        self.assertEqual(row['status'], 'failed'); self.assertNotIn('totalScenarios', row)

    def test_driver_timeout_never_claims_complete(self):
        result, row, calls = self.run_orchestrator(fail='timeout')
        self.assertEqual(result, 1); self.assertEqual(len(calls), 2)
        self.assertEqual(row['failureType'], 'TimeoutExpired')

    def test_driver_rejects_wrong_executable_source_before_suites(self):
        result, row, calls = self.run_orchestrator(identity_source='c'*40)
        self.assertEqual(result, 1); self.assertEqual(len(calls), 1)
        self.assertEqual(row['suites'], [])

    def test_driver_rechecks_executable_bytes_after_suites(self):
        result, row, _ = self.run_orchestrator(after_digest='c'*64)
        self.assertEqual(result, 1); self.assertEqual(row['status'], 'failed')


class ProcessObservation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        # Synthetic /proc parser tests also run on Windows; production remains Linux-only.
        uid = mock.patch.object(sampler.os, 'getuid', return_value=self.root.stat().st_uid, create=True)
        uid.start(); self.addCleanup(uid.stop)

    def process_files(self):
        root = self.root/'123'; root.mkdir()
        fields = ['0']*22
        fields[0]='S'; fields[1]='1'; fields[11]='2'; fields[12]='3'; fields[19]='99'
        (root/'stat').write_text('123 (private ) process name) '+' '.join(fields))
        (root/'status').write_text('VmRSS:\t12 kB\nVmHWM:\t15 kB\n')
        (root/'io').write_text('read_bytes: 8\nwrite_bytes: 9\n')
        (root/'fd').mkdir()
        return root

    def test_stat_parser_does_not_retain_name_and_reads_correct_identity(self):
        self.process_files()
        with mock.patch.object(sampler, 'PROC', self.root):
            row = sampler.proc_row(123)
            self.assertEqual(row['startTicks'], 99); self.assertEqual(row['cpuTicks'], 5)
            self.assertNotIn('comm', row)
            self.assertEqual(sampler.metrics(row)['rssKiB'], 12)

    def test_unreadable_metrics_are_unknown_not_zero(self):
        root = self.process_files(); (root/'status').write_text('State: S\n')
        with mock.patch.object(sampler, 'PROC', self.root):
            self.assertIsNone(sampler.metrics(sampler.proc_row(123)))

    def test_metrics_discard_reused_pid(self):
        self.process_files()
        with mock.patch.object(sampler, 'PROC', self.root), \
             mock.patch.object(sampler, 'proc_row', return_value={'startTicks':100}):
            self.assertIsNone(sampler.metrics({'pid':123,'startTicks':99}))

    def test_family_prunes_exits_retains_observed_orphans_and_excludes_reuse(self):
        table = {1:{'startTicks':10,'ppid':0}, 2:{'startTicks':20,'ppid':1},
                 3:{'startTicks':30,'ppid':2}, 4:{'startTicks':41,'ppid':99},
                 5:{'startTicks':50,'ppid':99}}
        result = sampler.active_family(table, {1:10,4:40,5:50,6:60})
        self.assertEqual(result, {1:10,2:20,3:30,5:50})

    def test_pid_image_change_is_not_accepted(self):
        with mock.patch.object(sampler.Path, 'stat', side_effect=FileNotFoundError):
            self.assertFalse(sampler.same_image(123,self.root/'binary',(1,2,3,4)))

    @unittest.skipUnless(sys.platform == 'linux', 'real Linux procfs')
    def test_real_process_sampling_has_source_assertion_and_no_qualification(self):
        child = subprocess.Popen([sys.executable,'-c','import sys; sys.stdin.buffer.read(1)'],
                                 stdin=subprocess.PIPE)
        try:
            out = self.root/'observe.jsonl'
            proc = subprocess.run([sys.executable,str(Path(sampler.__file__)),
                '--pid',str(child.pid),'--binary',str(Path(sys.executable).resolve()),
                '--source-sha',SHA,'--seconds','.12','--interval','.05','--output',str(out)],
                capture_output=True,timeout=15)
            self.assertEqual(proc.returncode,0,proc.stderr.decode())
            row = json.loads(Path(str(out)+'.summary.json').read_text())
            self.assertFalse(row['qualificationGranted']); self.assertFalse(row['sourceShaIndependentlyVerified'])
            self.assertEqual(row['endReason'],'duration-complete')
            self.assertGreaterEqual(row['sampleCount'],2)
            samples = [json.loads(line) for line in out.read_text().splitlines()][1:]
            self.assertTrue(all(r['measurementComplete'] for r in samples))
            self.assertTrue(all('comm' not in p for r in samples for p in r['processes']))
        finally:
            child.terminate(); child.wait(timeout=5); child.stdin.close()

    @unittest.skipUnless(sys.platform == 'linux', 'real Linux procfs')
    def test_root_exit_is_not_successful_duration(self):
        child = subprocess.Popen([sys.executable,'-c','import sys; sys.stdin.buffer.read(1)'],stdin=subprocess.PIPE)
        out = self.root/'short.jsonl'
        collector = subprocess.Popen([sys.executable,str(Path(sampler.__file__)),
            '--pid',str(child.pid),'--binary',str(Path(sys.executable).resolve()),
            '--source-sha',SHA,'--seconds','5','--interval','.05','--output',str(out)],
            stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic()+5
            while not (out.exists() and out.stat().st_size > 0):
                self.assertIsNone(collector.poll()); self.assertLess(time.monotonic(),deadline)
                time.sleep(.01)
            child.terminate(); child.wait(timeout=5)
            stdout,stderr = collector.communicate(timeout=10)
            self.assertEqual(collector.returncode,3,stderr.decode())
            self.assertEqual(json.loads(stdout)['endReason'],'root-exited-or-identity-lost')
        finally:
            if child.poll() is None: child.kill(); child.wait()
            child.stdin.close()
            if collector.poll() is None: collector.kill(); collector.communicate()


@unittest.skipUnless(sys.platform == 'linux', 'same-directory link publication is Linux-only')
class SqliteSnapshot(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.source = self.root/'state #?.sqlite3'; self.output = self.root/'copy.sqlite3'
        with sqlite3.connect(str(self.source)) as db:
            db.execute('CREATE TABLE data(value TEXT)'); db.execute("INSERT INTO data VALUES('base')")
        db.close()

    def test_real_backup_contains_committed_wal_and_keeps_original(self):
        db = sqlite3.connect(str(self.source))
        try:
            db.execute('PRAGMA journal_mode=WAL'); db.execute('PRAGMA wal_autocheckpoint=0')
            db.execute("INSERT INTO data VALUES('wal')"); db.commit()
            self.assertTrue(Path(str(self.source)+'-wal').is_file())
            report = backup.snapshot(self.source,self.output)
            self.assertEqual(report['sha256'],hashlib.sha256(self.output.read_bytes()).hexdigest())
            self.assertTrue(report['sourceConnectionReadOnly']); self.assertFalse(report['qualificationGranted'])
            copied = sqlite3.connect(str(self.output))
            try:
                self.assertEqual(copied.execute('SELECT value FROM data').fetchall(),[('base',),('wal',)])
            finally: copied.close()
            self.assertEqual(db.execute('SELECT COUNT(*) FROM data').fetchone(),(2,))
            self.assertEqual(list(self.root.glob('.snapshot-*')),[])
        finally: db.close()

    def test_existing_destination_is_never_changed(self):
        self.output.write_bytes(b'retained')
        with self.assertRaises(FileExistsError): backup.snapshot(self.source,self.output)
        self.assertEqual(self.output.read_bytes(),b'retained')

    def test_dangling_destination_link_and_source_link_are_refused(self):
        self.output.symlink_to(self.root/'missing')
        with self.assertRaises(FileExistsError): backup.snapshot(self.source,self.output)
        self.assertFalse((self.root/'missing').exists())
        link = self.root/'source-link'; link.symlink_to(self.source)
        with self.assertRaises(ValueError): backup.snapshot(link,self.root/'other')

    def test_competing_writer_survives_failed_publication(self):
        def competing(stage, output):
            Path(output).write_bytes(b'other writer'); raise FileExistsError('fixture')
        with mock.patch.object(backup.os,'link',side_effect=competing):
            with self.assertRaises(FileExistsError): backup.snapshot(self.source,self.output)
        self.assertEqual(self.output.read_bytes(),b'other writer')
        self.assertEqual(list(self.root.glob('.snapshot-*')),[])

    def test_corrupt_source_not_modified_no_final_backup(self):
        self.source.write_bytes(b'corrupt original')
        with self.assertRaises(sqlite3.DatabaseError): backup.snapshot(self.source,self.output)
        self.assertEqual(self.source.read_bytes(),b'corrupt original')
        self.assertFalse(self.output.exists()); self.assertEqual(list(self.root.glob('.snapshot-*')),[])

    def test_busy_source_obeys_callback_deadline_without_publishing(self):
        db = sqlite3.connect(str(self.source)); db.execute('BEGIN EXCLUSIVE')
        try:
            with mock.patch.object(backup,'DEADLINE_SECONDS',.05):
                with self.assertRaises(TimeoutError): backup.snapshot(self.source,self.output)
        finally: db.rollback(); db.close()
        self.assertFalse(self.output.exists()); self.assertEqual(list(self.root.glob('.snapshot-*')),[])

    def test_no_raw_error_or_success_on_cli_failure(self):
        result = subprocess.run([sys.executable,str(Path(backup.__file__)),
            '--database',str(self.root/'private-secret-missing'),'--output',str(self.output)],
            capture_output=True,timeout=10)
        self.assertEqual(result.returncode,1); self.assertEqual(result.stdout,b'')
        self.assertNotIn(b'private-secret',result.stderr)


if __name__ == '__main__': unittest.main()
