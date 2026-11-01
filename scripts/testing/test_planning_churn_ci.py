"""Negative-fixture tests for source-bound offline planning churn."""
import json
from pathlib import Path
import tempfile
import unittest

import planning_churn_ci as probe


class PlanningChurnContracts(unittest.TestCase):
    def records(self):
        return [
            {'kind': 'start'},
            {'kind': 'request', 'method': 'initialize'},
            {'kind': 'page', 'start': 0, 'end': 2},
            {'kind': 'request', 'method': 'thread/resume', 'excludeTurns': True},
            {'kind': 'page', 'start': 2, 'end': 4},
            {'kind': 'target', 'value': 'A_TARGET_DURING'},
            {'kind': 'churn-end', 'events': 3},
        ]

    def test_valid_query_only_pagination(self):
        probe.verify_events(self.records(), 4)

    def test_unknown_rpc_never_passes(self):
        records = self.records() + [{'kind': 'request', 'method': 'turn/start'}]
        with self.assertRaises(ValueError):
            probe.verify_events(records, 4)

    def test_resume_must_not_hydrate_turns(self):
        records = self.records()
        records[3]['excludeTurns'] = False
        with self.assertRaises(ValueError):
            probe.verify_events(records, 4)

    def test_incomplete_or_gapped_pagination_is_rejected(self):
        records = self.records()
        records.pop(4)
        with self.assertRaises(ValueError):
            probe.verify_events(records, 4)
        records = self.records()
        records[4]['start'] = 3
        with self.assertRaises(ValueError):
            probe.verify_events(records, 4)

    def test_backend_restart_and_missing_churn_end_are_rejected(self):
        with self.assertRaises(ValueError):
            probe.verify_events(self.records() + [{'kind': 'start'}], 4)
        with self.assertRaises(ValueError):
            probe.verify_events(self.records()[:-1], 4)

    def test_partially_written_trace_is_not_fabricated(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'trace.jsonl'
            path.write_bytes(b'{"kind":"start"}\n{"kind":"noise"')
            self.assertEqual(probe.trace_events(path), [{'kind': 'start'}])

    def test_invalid_sha_is_rejected_before_running_binary(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / 'not-a-real-program'
            binary.write_bytes(b'no execution allowed')
            for sha in ('invalid', 'F' * 40, 'f' * 39, 'f' * 41, ''):
                with self.subTest(sha=sha), self.assertRaises(ValueError):
                    probe.verify_source(binary, sha, {}, Path(tmp))

    def test_binary_symlink_is_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            binary = Path(tmp) / 'not-a-real-program'
            binary.write_bytes(b'no execution allowed')
            alias = Path(tmp) / 'alias'
            alias.symlink_to(binary)
            with self.assertRaises(ValueError):
                probe.verify_source(alias, 'a' * 40, {}, Path(tmp))

    def test_oversized_scenarios_are_rejected_without_processes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for rows, hz, seconds in ((20_001, 50, 8), (20_000, 51, 8),
                                      (20_000, 50, 16), (1, 10, 4)):
                with self.subTest(rows=rows, hz=hz, seconds=seconds), self.assertRaises(ValueError):
                    probe.run(root / 'absent', 'f' * 40, root / 'not-created',
                              rows, hz, seconds)
                self.assertFalse((root / 'not-created').exists())

    def test_output_directory_must_not_be_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output = root / 'already-has-evidence'
            output.mkdir()
            with self.assertRaises(ValueError):
                probe.run(root / 'absent', 'f' * 40, output, 20_000, 50, 8)
            self.assertEqual(list(output.iterdir()), [])


if __name__ == '__main__':
    unittest.main()
