import csv
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
import evaluate_ux as ux
import check_frontend_budgets as perf

SHA = 'ceee30878d51f615e4b0b14e80a79cf7805d0238'
BINARY = 'b' * 64


def trials(count=100):
    return [dict(zip(ux.FIELDS, [
        'T%03d' % i, chain, 'needs-you', '60x24', 'warm',
        '10000' if chain == 'A' else '7000', 'true', 'false', 'na', 'PRIVATE_NOTE_DO_NOT_EXPORT'
    ])) for i in range(count) for chain in ('A', 'B')]


def encode(rows):
    out = io.StringIO(newline='')
    writer = csv.DictWriter(out, fieldnames=ux.FIELDS)
    writer.writeheader()
    writer.writerows(rows)
    return out.getvalue().encode()


def report():
    return {
        'schema': 'codex-tui/interaction-performance/v1', 'sourceSha': SHA,
        'sampleQualified': True, 'warmupIterations': 20, 'iterations': 200,
        'viewportWidth': 160, 'viewportHeight': 40, 'rows': 10000,
        'source': 'synthetic-test-only',
        'authority': 'synthetic-cpu-path-diagnostic-not-terminal-slo',
        'modelInferenceCalls': 0, 'humanTimeSavings': None, 'modelTokenSavings': None,
        'fixtures': [dict(fixture=name, rows=50000 if name.endswith('50k') else 10000,
                          p50Ms=1, p95Ms=2, p99Ms=3, maxMs=4) for name in sorted(perf.COVERAGE)]
    }


class UXTests(unittest.TestCase):
    def analyze(self, rows):
        return ux.analyze(encode(rows), SHA, BINARY, 'synthetic')

    def test_numeric_success_never_grants_live_authority(self):
        result = self.analyze(trials())
        self.assertTrue(result['quantitativeTargetsMet'])
        self.assertEqual(result['aggregate']['pairedMedianReductionPct'], 30)
        for field in ('stableReady', 'publicationAllowed', 'measurementAuthenticityVerified',
                      'experimentDesignVerified', 'inputIdentityVerifiedAgainstBinary'):
            self.assertIs(result[field], False)
        self.assertEqual(result['liveQualification'], 'NOT_VERIFIED')

    def test_private_notes_and_trial_ids_are_never_exported(self):
        output = json.dumps(self.analyze(trials()))
        self.assertNotIn('PRIVATE_NOTE', output)
        self.assertNotIn('T001', output)

    def test_sample_shortage_is_not_pass(self):
        result = self.analyze(trials(2))
        self.assertFalse(result['quantitativeTargetsMet'])
        self.assertIn('fewer-than-30-paired-tasks', result['quantitativeBlockers'])
        self.assertIn('fewer-than-100-candidate-jumps', result['quantitativeBlockers'])

    def test_failed_targets_are_counted_not_dropped(self):
        rows = trials()
        rows[1]['correct_target'] = rows[3]['correct_target'] = 'false'
        result = self.analyze(rows)
        self.assertEqual(result['aggregate']['chains']['B']['attempts'], 100)
        self.assertEqual(result['aggregate']['chains']['B']['incorrectTargets'], 2)
        self.assertIn('candidate-jump-correctness-below-99-percent', result['quantitativeBlockers'])

    def test_one_percent_error_boundary(self):
        rows = trials()
        rows[1]['correct_target'] = 'false'
        self.assertTrue(self.analyze(rows)['quantitativeTargetsMet'])

    def test_mutation_in_either_chain_blocks_numeric_success(self):
        for idx in (0, 1):
            rows = trials()
            rows[idx]['accidental_mutation'] = 'true'
            self.assertIn('accidental-mutation-observed', self.analyze(rows)['quantitativeBlockers'])

    def test_slowdown_is_not_improvement(self):
        rows = trials()
        for r in rows:
            if r['chain'] == 'B':
                r['duration_ms'] = '12000'
        result = self.analyze(rows)
        self.assertEqual(result['aggregate']['pairedMedianReductionPct'], -20)
        self.assertFalse(result['quantitativeTargetsMet'])

    def test_pairing_accepts_reordered_rows(self):
        rows = trials()
        self.assertEqual(self.analyze(rows)['aggregate'], self.analyze(list(reversed(rows)))['aggregate'])

    def test_duplicate_pair_rejected(self):
        rows = trials(1)
        rows.append(rows[0].copy())
        with self.assertRaises(ux.InvalidInput):
            self.analyze(rows)

    def test_unpaired_trial_rejected(self):
        with self.assertRaises(ux.InvalidInput):
            self.analyze(trials()[:-1])

    def test_mismatched_pair_context_rejected(self):
        for field, value in [('scenario', 'search'), ('terminal', '100x30'), ('session_state', 'cold')]:
            rows = trials(1)
            rows[1][field] = value
            with self.assertRaises(ux.InvalidInput):
                self.analyze(rows)

    def test_strict_booleans(self):
        for value in ('True', '1', '', 'yes', 'true '):
            rows = trials(1)
            rows[0]['correct_target'] = value
            with self.assertRaises(ux.InvalidInput):
                self.analyze(rows)

    def test_duration_rejects_zero_negative_nan_and_fraction(self):
        for value in ('0', '-1', 'NaN', '1.5', '86400001'):
            rows = trials(1)
            rows[0]['duration_ms'] = value
            with self.assertRaises(ux.InvalidInput):
                self.analyze(rows)

    def test_opaque_identifier_required(self):
        rows = trials(1)
        rows[0]['trial_id'] = 'PRIVATE_REPOSITORY'
        with self.assertRaises(ux.InvalidInput) as caught:
            self.analyze(rows)
        self.assertNotIn('PRIVATE_REPOSITORY', str(caught.exception))

    def test_notification_not_measured_is_preserved(self):
        rows = trials(1)
        for r in rows:
            r['scenario'] = 'notification'
        result = self.analyze(rows)
        self.assertEqual(result['aggregate']['chains']['B']['notificationObserved']['na'], 1)
        self.assertEqual(result['aggregate']['chains']['B']['jumpAttempts'], 0)

    def test_notification_field_outside_scenario_rejected(self):
        rows = trials(1)
        rows[0]['notification_observed'] = 'true'
        with self.assertRaises(ux.InvalidInput):
            self.analyze(rows)

    def test_header_and_empty_data_rejected(self):
        for raw in (b'', b'other\nvalue\n', encode([])):
            with self.assertRaises(ux.InvalidInput):
                ux.analyze(raw, SHA, BINARY)

    def test_invalid_utf8_and_nul_rejected(self):
        for raw in (b'\xff', encode(trials(1)) + b'\x00'):
            with self.assertRaises(ux.InvalidInput):
                ux.analyze(raw, SHA, BINARY)

    def test_source_identity_is_strict(self):
        for source, binary in [('main', BINARY), (SHA.upper(), BINARY), (SHA, 'none')]:
            with self.assertRaises(ux.InvalidInput):
                ux.analyze(encode(trials(1)), source, binary)

    def test_terminal_dimensions_are_bounded_before_integer_conversion(self):
        rows = trials(1)
        rows[0]['terminal'] = '9' * 6000 + 'x24'
        with self.assertRaises(ux.InvalidInput):
            self.analyze(rows)

    def test_groups_preserve_cold_warm_and_terminal(self):
        rows = trials(2)
        rows[2]['terminal'] = rows[3]['terminal'] = '100x30'
        rows[2]['session_state'] = rows[3]['session_state'] = 'cold'
        self.assertEqual(len(self.analyze(rows)['groups']), 2)

    def test_output_is_exclusive_and_does_not_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            src = path/'trials.csv'
            dst = path/'report.json'
            src.write_bytes(encode(trials()))
            cmd = [sys.executable, str(ROOT/'evaluate_ux.py'), str(src), '--source-sha', SHA,
                   '--binary-sha256', BINARY, '--data-kind', 'synthetic', '--output', str(dst)]
            first = subprocess.run(cmd, capture_output=True, timeout=10)
            self.assertEqual(first.returncode, 0, first.stderr)
            expected = dst.read_bytes()
            second = subprocess.run(cmd, capture_output=True, timeout=10)
            self.assertEqual(second.returncode, 2)
            self.assertEqual(dst.read_bytes(), expected)

    def test_malformed_csv_header_is_controlled(self):
        with self.assertRaises(ux.InvalidInput):
            ux.analyze(b'"unclosed\n', SHA, BINARY)

    def test_input_size_bound(self):
        with self.assertRaises(ux.InvalidInput):
            ux.analyze(b'x' * (ux.MAX_BYTES + 1), SHA, BINARY)

    def test_numeric_targets_do_not_hide_unmeasured_targets(self):
        result = self.analyze(trials())
        self.assertIn('needs-you-within-5s', result['otherTargetsNotQualified'])
        self.assertEqual(len(result['computedTargetScope']), 3)


class PerformanceTests(unittest.TestCase):
    def evaluate(self, data):
        return perf.evaluate(json.dumps(data).encode(), SHA)

    def test_pass_is_only_a_proposed_budget_analysis(self):
        result = self.evaluate(report())
        self.assertTrue(result['withinAllProposedBudgets'])
        for field in ('benchmarkExecutedByThisTool', 'terminalSloVerified', 'stableReady', 'publicationAllowed'):
            self.assertIs(result[field], False)

    def test_slow_path_cannot_hide_behind_valid_structure(self):
        data = report()
        f = next(x for x in data['fixtures'] if x['fixture'] == 'planning-ui-stale-retry-50k')
        f.update(p95Ms=92.332083, p99Ms=116.303523, maxMs=118.59302)
        result = self.evaluate(data)
        self.assertFalse(result['withinAllProposedBudgets'])
        self.assertEqual(sum(not r['withinProposedBudget'] for r in result['results']), 1)

    def test_missing_or_duplicate_fixture_is_rejected(self):
        for data in (report(), report()):
            data['fixtures'][-1] = data['fixtures'][0]
            with self.assertRaises(ValueError):
                self.evaluate(data)
        data = report()
        data['fixtures'].pop()
        with self.assertRaises(ValueError):
            self.evaluate(data)

    def test_nonfinite_negative_boolean_timing_is_rejected(self):
        for value in (float('nan'), float('inf'), -1, True, 10 ** 1000):
            data = report()
            data['fixtures'][0]['p95Ms'] = value
            with self.assertRaises(ValueError):
                self.evaluate(data)

    def test_unordered_distribution_rejected(self):
        data = report()
        data['fixtures'][0]['p95Ms'] = 10
        with self.assertRaises(ValueError):
            self.evaluate(data)

    def test_mismatched_source_rejected(self):
        data = report()
        data['sourceSha'] = 'a' * 40
        with self.assertRaises(ValueError):
            self.evaluate(data)

    def test_insufficient_samples_rejected(self):
        for field, value in [('warmupIterations', 19), ('iterations', 199), ('sampleQualified', False)]:
            data = report()
            data[field] = value
            with self.assertRaises(ValueError):
                self.evaluate(data)

    def test_model_call_or_claimed_savings_rejected(self):
        for field, value in [('modelInferenceCalls', 1), ('humanTimeSavings', 12), ('modelTokenSavings', 0)]:
            data = report()
            data[field] = value
            with self.assertRaises(ValueError):
                self.evaluate(data)

    def test_boolean_identity_fields_rejected(self):
        data = report()
        data['modelInferenceCalls'] = False
        with self.assertRaises(ValueError):
            self.evaluate(data)

    def test_missing_source_label_rejected(self):
        data = report()
        data['source'] = ''
        with self.assertRaises(ValueError):
            self.evaluate(data)

    def test_wrong_fixture_rows_rejected(self):
        data = report()
        data['fixtures'][0]['rows'] = 1
        with self.assertRaises(ValueError):
            self.evaluate(data)

    def test_duplicate_json_keys_rejected(self):
        raw = json.dumps(report()).encode()
        raw = b'{"sourceSha":"' + SHA.encode() + b'",' + raw[1:]
        with self.assertRaises(ValueError):
            perf.evaluate(raw, SHA)

    def test_exact_proposed_budget_boundary(self):
        data = report()
        f = next(x for x in data['fixtures'] if x['fixture'] == 'planning-ui-stale-retry-50k')
        f.update(p95Ms=50, p99Ms=50, maxMs=50)
        self.assertTrue(self.evaluate(data)['withinAllProposedBudgets'])


if __name__ == '__main__':
    unittest.main()
