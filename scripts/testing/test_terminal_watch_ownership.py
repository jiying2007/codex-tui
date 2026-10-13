"""The actual-program watch observer must reject missing or unexpected reads."""
import unittest
from watch_ownership_regression import CASES, check_identity, check_reads


class WatchAssertions(unittest.TestCase):
    def test_suite_has_exact_unique_cases(self):
        self.assertEqual(len(CASES), 11)
        self.assertEqual(len(set(CASES)), 11)

    def test_missing_live_read_and_retained_idle_read_are_both_rejected(self):
        for actual, expected in [(0, 1), (1, 0), (2, 1)]:
            with self.subTest(actual=actual, expected=expected), self.assertRaises(RuntimeError):
                check_reads(actual, expected, 'negative control')
        check_reads(0, 0, 'idle')
        check_reads(1, 1, 'active')

    def test_identity_mismatch_missing_and_unqualified_sources_are_not_exact(self):
        sha = 'a' * 40
        for report, expected in [({}, sha), ({'sourceSha':'b'*40}, sha),
                                 ({'sourceSha':sha}, None), ({'sourceSha':'unknown'}, sha)]:
            with self.subTest(report=report, expected=expected), self.assertRaises(RuntimeError):
                check_identity(report, expected)
        check_identity({'sourceSha':sha}, sha)
        check_identity({'sourceSha':'unknown'}, 'unknown')


if __name__ == '__main__':
    unittest.main()
