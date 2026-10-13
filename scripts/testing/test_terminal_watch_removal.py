"""Negative controls for the maintained real-program removal observer."""
import ast
import copy
import unittest
from watch_ownership_regression import MOCK
from watch_removal_regression import CASES, mock_source, validate_observations


def sample(keep=False):
    return [{'seq': seq, 'actualHistoryReads': reads, 'expectedHistoryReads': reads}
            for seq, reads in enumerate([1, 0, int(keep), int(keep)], 1)]


class RemovalObserver(unittest.TestCase):
    def test_cases_are_exact_and_unique(self):
        self.assertEqual(len(CASES), 6)
        self.assertEqual(len(set(CASES)), 6)

    def test_generated_mock_parses_on_python38_and_is_isolated(self):
        source = mock_source(MOCK)
        ast.parse(source, feature_version=8)
        self.assertIn("os.environ['MOCK_LOG']+'.'+str(os.getpid())", source)
        self.assertIn("if m=='thread/list' and not control_started:", source)
        self.assertIn("elif m=='thread/searchOccurrences':", source)
        self.assertIn("'id':'ghost'", source)

    def test_fixture_source_drift_cannot_silently_disable_the_scenario(self):
        for source in [MOCK.replace("if m=='turn/start':", "if False:"), MOCK + MOCK]:
            with self.subTest(source=source[:30]), self.assertRaises(RuntimeError):
                mock_source(source)

    def test_healthy_and_retired_controls_both_validate(self):
        validate_observations(sample(True), True)
        validate_observations(sample(False), False)

    def test_retained_reads_missing_live_reads_and_overreads_are_rejected(self):
        for keep, index, value in [(False, 2, 1), (False, 3, 1),
                                   (True, 2, 0), (True, 3, 2), (False, 0, 0)]:
            data = sample(keep)
            data[index]['actualHistoryReads'] = value
            with self.subTest(keep=keep, index=index), self.assertRaises(RuntimeError):
                validate_observations(data, keep)

    def test_missing_duplicate_reordered_and_rewritten_phases_are_rejected(self):
        original = sample(False)
        changed = copy.deepcopy(original)
        changed[2]['expectedHistoryReads'] = 1
        for data in [original[:-1], original + [original[-1]],
                     list(reversed(original)), changed]:
            with self.subTest(data=data), self.assertRaises(RuntimeError):
                validate_observations(data, False)


if __name__ == '__main__':
    unittest.main()
